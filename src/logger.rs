//! Per-service file logging. Each service writes to `logs/<name>/current.log`;
//! on a date rollover the content is merged into `<dateStr>.log` and the file
//! is truncated. Old dated logs are removed after `retention_days`.
//! Mirrors src/logger.js.

use crate::config::LogConfig;
use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub struct ServiceLogger {
    enabled: bool,
    retention_days: u32,
    dir: PathBuf,
}

/// `YYYYMMDD` string for the given SystemTime.
fn date_str(t: std::time::SystemTime) -> String {
    let dt: chrono::DateTime<Local> = t.into();
    dt.format("%Y%m%d").to_string()
}

impl ServiceLogger {
    pub fn new(service_name: &str, log_cfg: &LogConfig, logs_base: &Path) -> Self {
        ServiceLogger {
            enabled: log_cfg.enabled,
            retention_days: log_cfg.retention_days,
            dir: logs_base.join(service_name),
        }
    }

    fn current_log_path(&self) -> PathBuf {
        self.dir.join("current.log")
    }

    fn date_log_path(&self, date: &str) -> PathBuf {
        self.dir.join(format!("{date}.log"))
    }

    fn ensure_dir(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)
    }

    /// Merge `current.log` into `<date>.log` and truncate `current.log`.
    fn merge_to_date(&self, date: &str) {
        let src = self.current_log_path();
        let Ok(content) = fs::read_to_string(&src) else {
            return;
        };
        if !content.is_empty() {
            if let Ok(mut dst) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.date_log_path(date))
            {
                let _ = dst.write_all(content.as_bytes());
            }
        }
        let _ = fs::write(&src, "");
    }

    /// Delete dated logs older than `retention_days` (current.log is kept).
    fn cleanup(&self) {
        if self.retention_days == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = std::time::SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name == "current.log" || !name.ends_with(".log") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            let age_days = now
                .duration_since(modified)
                .map(|d| d.as_secs() as u64 / 86400)
                .unwrap_or(0);
            if age_days > self.retention_days as u64 {
                let _ = fs::remove_file(&path);
            }
        }
    }

    /// Open (or "rotate") the log for the current day.
    pub fn open(&self) {
        if !self.enabled || self.ensure_dir().is_err() {
            return;
        }
        let today = date_str(std::time::SystemTime::now());
        let cp = self.current_log_path();
        if let Ok(meta) = fs::metadata(&cp) {
            if let Ok(modified) = meta.modified() {
                let file_date = date_str(modified);
                if file_date != today {
                    self.merge_to_date(&file_date);
                }
            }
        }
        self.cleanup();
    }

    /// Append a single line with an ISO timestamp prefix.
    pub fn write(&self, data: &str) {
        if !self.enabled || self.ensure_dir().is_err() {
            return;
        }
        let now: chrono::DateTime<Local> = std::time::SystemTime::now().into();
        let ts = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let line = format!("[{ts}] {data}");
        let path = self.current_log_path();
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(line.as_bytes());
        }
    }

    pub fn close(&self) {
        // Nothing to flush: writes are synchronous.
    }
}