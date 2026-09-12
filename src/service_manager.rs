//! Service lifecycle management: spawn processes (through the system shell,
//! like the original `spawn(cmd, { shell: true })`), kill process trees,
//! automatically restart on abnormal exit and report per-service status.
//! Mirrors src/service-manager.js.

use crate::config::{ConfigLoader, ServiceConfig};
use crate::logger::ServiceLogger;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::task::JoinHandle;

/// Simple operation result, mirroring the original `{ success, message }`.
#[derive(Debug)]
pub struct ServiceResult {
    pub success: bool,
    pub message: String,
}

impl ServiceResult {
    fn ok(message: String) -> Self {
        ServiceResult { success: true, message }
    }
    fn fail(message: String) -> Self {
        ServiceResult { success: false, message }
    }
}

/// Snapshot of a service's runtime state, returned by the status endpoints.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub running: bool,
    pub operation: Option<String>,
    pub duration: Option<u64>,
    pub abnormal_exit_count: u32,
}

/// Kill a process and all of its descendants.
///
/// Windows: `taskkill /pid <pid> /T` (plus `/F` when forcing).
/// Unix: recursively collect children with `ps -o pid= --ppid <pid>`,
/// signal them, then signal the root process.
fn kill_process_tree(pid: u32, force: bool) {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/pid", &pid.to_string(), "/T"]);
        if force {
            cmd.arg("/F");
        }
        let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }

    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps")
            .args(["-o", "pid=", "--ppid", &pid.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        if let Ok(out) = out {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Ok(child_pid) = line.trim().parse::<u32>() {
                    kill_process_tree(child_pid, force);
                }
            }
        }
        let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
        unsafe {
            libc::kill(pid as i32, sig);
        }
    }
}

/// Unix-only: the signal that terminated the process, if any.
/// On Windows there is no signal concept, so this is always `None`.
fn exit_signal(st: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        // `signal()` comes from the Unix `ExitStatusExt` trait.
        use std::os::unix::process::ExitStatusExt;
        st.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = st;
        None
    }
}

/// Build the shell command that launches a service, matching the original
/// `spawn(cmd, [], { shell: true })` behaviour.
fn shell_command(command: &str) -> (&'static str, Vec<String>) {
    #[cfg(windows)]
    {
        ("cmd", vec!["/C".to_string(), command.to_string()])
    }
    #[cfg(not(windows))]
    {
        ("sh", vec!["-c".to_string(), command.to_string()])
    }
}

pub struct ServiceManager {
    /// name -> pid of the currently running process
    processes: Mutex<HashMap<String, u32>>,
    loggers: Mutex<HashMap<String, Arc<Mutex<ServiceLogger>>>>,
    retry_counts: Mutex<HashMap<String, u32>>,
    start_times: Mutex<HashMap<String, Instant>>,
    pending_ops: Mutex<HashMap<String, String>>,
    stopping: Mutex<HashSet<String>>,
    retry_timers: Mutex<HashMap<String, JoinHandle<()>>>,
    config_loader: Arc<ConfigLoader>,
    logs_base: PathBuf,
}

impl ServiceManager {
    pub fn new(config_loader: Arc<ConfigLoader>, logs_base: PathBuf) -> Self {
        ServiceManager {
            processes: Mutex::new(HashMap::new()),
            loggers: Mutex::new(HashMap::new()),
            retry_counts: Mutex::new(HashMap::new()),
            start_times: Mutex::new(HashMap::new()),
            pending_ops: Mutex::new(HashMap::new()),
            stopping: Mutex::new(HashSet::new()),
            retry_timers: Mutex::new(HashMap::new()),
            config_loader,
            logs_base,
        }
    }

    pub fn get_detailed_status(&self, name: &str) -> ServiceStatus {
        let running = self.processes.lock().unwrap().contains_key(name);
        let operation = self.pending_ops.lock().unwrap().get(name).cloned();
        let abnormal_exit_count = self.retry_counts.lock().unwrap().get(name).copied().unwrap_or(0);
        let duration = if running {
            self.start_times.lock().unwrap().get(name).map(|t| t.elapsed().as_secs())
        } else {
            None
        };
        ServiceStatus {
            running,
            operation,
            duration,
            abnormal_exit_count,
        }
    }

    fn cancel_retry_timer(&self, name: &str) {
        if let Some(handle) = self.retry_timers.lock().unwrap().remove(name) {
            handle.abort();
        }
    }

    /// Start a service and return immediately. Two background tasks take over:
    /// one pumps stdout/stderr into the log file, the other waits for the
    /// process to exit and schedules a restart if the exit was abnormal.
    pub fn start(self: &Arc<Self>, name: &str, config: &ServiceConfig, is_retry: bool) -> ServiceResult {
        {
            let procs = self.processes.lock().unwrap();
            if procs.contains_key(name) {
                return ServiceResult::fail(format!("Service {name} is already running"));
            }
        }

        self.pending_ops.lock().unwrap().insert(name.to_string(), "starting".to_string());

        let logger = Arc::new(Mutex::new(ServiceLogger::new(name, &config.log, &self.logs_base)));
        logger.lock().unwrap().open();
        self.loggers.lock().unwrap().insert(name.to_string(), logger.clone());

        let (shell, args) = shell_command(&config.command);
        let mut cmd = Command::new(shell);
        cmd.args(&args)
            .current_dir(&config.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            // Equivalent of Node's windowsHide: true
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        #[cfg(unix)]
        {
            // Equivalent of Node's setpgid: true - own process group
            cmd.process_group(0);
        }

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                self.loggers.lock().unwrap().remove(name);
                self.pending_ops.lock().unwrap().remove(name);
                return ServiceResult::fail(format!("Failed to start {name}: {e}"));
            }
        };
        let pid = child.id().expect("spawned process should have a pid");

        let stdout = child.stdout.take().expect("stdout pipe");
        let stderr = child.stderr.take().expect("stderr pipe");
        spawn_output_pump(stdout, logger.clone());
        spawn_output_pump(stderr, logger);

        self.processes.lock().unwrap().insert(name.to_string(), pid);
        if !is_retry {
            self.retry_counts.lock().unwrap().insert(name.to_string(), 0);
        }
        self.start_times.lock().unwrap().insert(name.to_string(), Instant::now());
        self.pending_ops.lock().unwrap().remove(name);

        // Watch the child: clean up on exit and retry abnormal terminations.
        let this = self.clone();
        let name_owned = name.to_string();
        let config_owned = config.clone();
        tokio::spawn(async move {
            let mut child = child;
            let status = child.wait().await;
            // `wait()` returns io::Result; a wait error is treated as no exit info.
            let (code, signal) = match status {
                Ok(st) => (st.code(), exit_signal(&st)),
                Err(_) => (None, None),
            };

            // A stop/restart was requested while we were still running:
            // drop the flag and give up.
            if this.stopping.lock().unwrap().remove(&name_owned) {
                return;
            }
            // Ignore exits from a process that has already been replaced.
            if this.processes.lock().unwrap().get(&name_owned) != Some(&pid) {
                return;
            }

            this.processes.lock().unwrap().remove(&name_owned);
            this.start_times.lock().unwrap().remove(&name_owned);

            let code_str = code.map(|c| c.to_string()).unwrap_or_else(|| "null".to_string());
            let signal_str = signal.map(|s| s.to_string()).unwrap_or_else(|| "null".to_string());
            let log_line = format!("Process exited with code {code_str}, signal {signal_str}\n");
            if let Some(l) = this.loggers.lock().unwrap().remove(&name_owned) {
                l.lock().unwrap().write(&log_line);
            }

            // Abnormal = non-zero exit that was not caused by SIGTERM/SIGKILL.
            let abnormal = code != Some(0) && signal != Some(15) && signal != Some(9);
            if abnormal && config_owned.retry_on_abnormal_exit {
                let count = {
                    let mut counts = this.retry_counts.lock().unwrap();
                    let c = counts.get(&name_owned).copied().unwrap_or(0) + 1;
                    counts.insert(name_owned.clone(), c);
                    c
                };
                if count <= config_owned.max_retries {
                    let this2 = this.clone();
                    let name2 = name_owned.clone();
                    let handle = tokio::spawn(async move {
                        // 1 s delay, then reload a fresh config and restart.
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        this2.retry_timers.lock().unwrap().remove(&name2);
                        if this2.stopping.lock().unwrap().remove(&name2) {
                            return;
                        }
                        if let Some(fresh) = this2.config_loader.load_service_config(&name2) {
                            this2.start(&name2, &fresh, true);
                        }
                    });
                    this.retry_timers.lock().unwrap().insert(name_owned.clone(), handle);
                }
            }
        });

        ServiceResult::ok(format!("Service {name} started"))
    }

    /// Gracefully stop a service by terminating the whole process tree.
    pub fn stop(&self, name: &str) -> ServiceResult {
        self.stopping.lock().unwrap().insert(name.to_string());
        self.cancel_retry_timer(name);

        let existing = self.processes.lock().unwrap().get(name).copied();
        let Some(pid) = existing else {
            // Nothing was running; do not leave the stopping flag behind.
            self.stopping.lock().unwrap().remove(name);
            return ServiceResult::fail(format!("Service {name} is not running"));
        };

        self.pending_ops.lock().unwrap().insert(name.to_string(), "stopping".to_string());

        if let Some(l) = self.loggers.lock().unwrap().remove(name) {
            l.lock().unwrap().write("Process stopped by daemon\n");
        }

        kill_process_tree(pid, false);
        kill_process_tree(pid, true);

        self.processes.lock().unwrap().remove(name);
        self.start_times.lock().unwrap().remove(name);
        self.pending_ops.lock().unwrap().remove(name);

        ServiceResult::ok(format!("Service {name} stopped"))
    }

    /// Kill a service immediately (no graceful phase).
    pub fn force_stop(&self, name: &str) -> ServiceResult {
        self.stopping.lock().unwrap().insert(name.to_string());
        self.cancel_retry_timer(name);

        let existing = self.processes.lock().unwrap().get(name).copied();
        let Some(pid) = existing else {
            self.stopping.lock().unwrap().remove(name);
            return ServiceResult::fail(format!("Service {name} is not running"));
        };

        self.pending_ops.lock().unwrap().insert(name.to_string(), "force-stopping".to_string());
        self.loggers.lock().unwrap().remove(name);

        kill_process_tree(pid, true);

        self.processes.lock().unwrap().remove(name);
        self.start_times.lock().unwrap().remove(name);
        self.pending_ops.lock().unwrap().remove(name);

        ServiceResult::ok(format!("Service {name} force stopped"))
    }

    /// Stop the running process (if any) and start it again.
    pub fn restart(self: &Arc<Self>, name: &str, config: &ServiceConfig) -> ServiceResult {
        self.pending_ops.lock().unwrap().insert(name.to_string(), "restarting".to_string());
        self.stopping.lock().unwrap().insert(name.to_string());
        self.cancel_retry_timer(name);

        // Copy the pid first so the processes lock is released before the
        // blocking kill calls and the lock taken again below.
        let existing = self.processes.lock().unwrap().get(name).copied();
        if let Some(pid) = existing {
            if let Some(l) = self.loggers.lock().unwrap().remove(name) {
                l.lock().unwrap().write("Process stopped by daemon\n");
            }
            kill_process_tree(pid, false);
            kill_process_tree(pid, true);
            self.processes.lock().unwrap().remove(name);
            self.start_times.lock().unwrap().remove(name);
        }

        let result = self.start(name, config, false);
        self.stopping.lock().unwrap().remove(name);
        result
    }

    /// Stop every running service (used during daemon shutdown).
    pub fn stop_all(&self) {
        let names: Vec<String> = self.processes.lock().unwrap().keys().cloned().collect();
        for name in names {
            self.cancel_retry_timer(&name);
            self.stop(&name);
        }
    }
}

/// Copy chunks from a process pipe into the service logger.
fn spawn_output_pump<R>(mut reader: R, logger: Arc<Mutex<ServiceLogger>>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    logger.lock().unwrap().write(&String::from_utf8_lossy(&buf[..n]));
                }
            }
        }
    });
}
