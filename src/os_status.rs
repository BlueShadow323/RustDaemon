//! Collects a snapshot of CPU usage, memory and uptime for the `/os/status`
//! endpoint. CPU usage is sampled across a 100 ms window, mirroring the
//! original Node.js implementation (two `os.cpus()` reads 100 ms apart).

use serde_json::{json, Value};
use std::time::Duration;
use sysinfo::System;

/// Build the `/os/status` JSON payload.
pub async fn get_server_status() -> Value {
    let mut sys = System::new_all();
    // Baseline read, then sample again after 100 ms to get the usage delta.
    sys.refresh_cpu_usage();
    tokio::time::sleep(Duration::from_millis(100)).await;
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let total = sys.total_memory();
    let used = sys.used_memory();
    let free = total.saturating_sub(used);

    let cpu_usage = (sys.global_cpu_info().cpu_usage() as f64 * 10.0).round() / 10.0;
    let usage_pct = if total > 0 {
        (used as f64 / total as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    };

    let host = hostname::get().unwrap_or_default().to_string_lossy().to_string();

    json!({
        "cpu": cpu_usage,
        "memory": {
            "total": total,
            "used": used,
            "free": free,
            "usage": usage_pct
        },
        "uptime": System::uptime(),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "hostname": host
    })
}