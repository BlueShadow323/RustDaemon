//! Service definitions are plain JSON files stored in the `services/`
//! directory. This module loads, lists, saves and deletes them, applying the
//! same defaults and clamping rules as the original src/config.js.

use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Loads, lists, saves and deletes service definition files.
pub struct ConfigLoader {
    services_dir: PathBuf,
}

/// A parsed service definition with defaults applied.
#[derive(Clone, Debug)]
pub struct ServiceConfig {
    pub cwd: String,
    pub command: String,
    pub priority: u32,
    pub max_retries: u32,
    pub retry_on_abnormal_exit: bool,
    pub auto_start: bool,
    pub log: LogConfig,
}

#[derive(Clone, Debug)]
pub struct LogConfig {
    pub enabled: bool,
    pub retention_days: u32,
}

impl ConfigLoader {
    pub fn new(services_dir: PathBuf) -> Self {
        ConfigLoader { services_dir }
    }

    pub fn services_dir(&self) -> &Path {
        &self.services_dir
    }

    /// Read and parse a single service config. Returns `None` when the file
    /// is missing or malformed.
    pub fn load_service_config(&self, name: &str) -> Option<ServiceConfig> {
        let path = self.services_dir.join(format!("{name}.json"));
        let raw = fs::read_to_string(path).ok()?;
        let v: Value = serde_json::from_str(&raw).ok()?;
        Some(ServiceConfig {
            cwd: v.get("cwd")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(".")
                .to_string(),
            command: v.get("command").and_then(Value::as_str).unwrap_or("").to_string(),
            // priority is clamped to [0, 999], default 0 on load
            priority: match v.get("priority").and_then(Value::as_f64) {
                Some(p) => p.max(0.0).min(999.0) as u32,
                None => 0,
            },
            max_retries: v.get("maxRetries").and_then(Value::as_u64).unwrap_or(3) as u32,
            retry_on_abnormal_exit: v
                .get("retryOnAbnormalExit")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            auto_start: v.get("autoStart").and_then(Value::as_bool).unwrap_or(false),
            log: LogConfig {
                enabled: v
                    .get("log")
                    .and_then(|l| l.get("enabled"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                retention_days: v
                    .get("log")
                    .and_then(|l| l.get("retentionDays"))
                    .and_then(Value::as_u64)
                    .unwrap_or(7) as u32,
            },
        })
    }

    /// List the names of all `*.json` files inside the services directory.
    pub fn list_services(&self) -> Vec<String> {
        let mut names = Vec::new();
        let Ok(entries) = fs::read_dir(&self.services_dir) else {
            return names;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        names
    }

    /// Load every service config into a `name -> config` map.
    pub fn load_all_service_configs(&self) -> HashMap<String, ServiceConfig> {
        let mut result = HashMap::new();
        for name in self.list_services() {
            if let Some(cfg) = self.load_service_config(&name) {
                result.insert(name, cfg);
            }
        }
        result
    }

    pub fn make_services_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.services_dir)
    }

    /// Persist a service config. The payload is rebuilt from the raw request
    /// body so defaults match the original saveServiceConfig behaviour
    /// (e.g. a missing `priority` is stored as 999).
    pub fn save_service_config(&self, name: &str, body: &Value) -> io::Result<()> {
        self.make_services_dir()?;
        let log = body.get("log");
        let payload = serde_json::json!({
            "cwd": body.get("cwd").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("."),
            "command": body.get("command").and_then(Value::as_str).unwrap_or(""),
            "priority": match body.get("priority").and_then(Value::as_f64) {
                Some(p) => p.max(0.0).min(999.0) as u64,
                None => 999,
            },
            "maxRetries": body.get("maxRetries").and_then(Value::as_u64).unwrap_or(3),
            "retryOnAbnormalExit": body.get("retryOnAbnormalExit").and_then(Value::as_bool).unwrap_or(true),
            "autoStart": body.get("autoStart").and_then(Value::as_bool).unwrap_or(false),
            "log": {
                "enabled": log.and_then(|l| l.get("enabled")).and_then(Value::as_bool).unwrap_or(false),
                "retentionDays": log.and_then(|l| l.get("retentionDays")).and_then(Value::as_u64).unwrap_or(7)
            }
        });
        let file = self.services_dir.join(format!("{name}.json"));
        fs::write(file, serde_json::to_string_pretty(&payload)?)?;
        Ok(())
    }

    /// Remove a service config file, returning whether it existed.
    pub fn delete_service_config(&self, name: &str) -> bool {
        let path = self.services_dir.join(format!("{name}.json"));
        match fs::remove_file(path) {
            Ok(()) => true,
            Err(_) => false,
        }
    }
}
