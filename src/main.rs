//! rust-daemon entry point. Reads `.env`, starts the daemon API server and
//! the optional Web UI server, auto-starts configured services and shuts
//! everything down cleanly on SIGINT/SIGTERM.
//! Mirrors index.js.

mod config;
mod daemon_server;
mod logger;
mod os_status;
mod service_manager;
mod util;
mod webui_server;

use crate::config::{ConfigLoader, ServiceConfig};
use crate::service_manager::ServiceManager;
use crate::webui_server::WebUIState;
use std::net::SocketAddr;
use std::sync::Arc;

fn main() {
    // Load .env from the working directory if present.
    dotenvy::dotenv().ok();

    // Use a multi-thread runtime but cap the worker count (this daemon is
    // I/O-bound, so 2-4 workers are enough) and shrink the per-thread stack
    // from the default 2 MiB to keep memory usage low.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(std::thread::available_parallelism().map(|n| n.get().min(4)).unwrap_or(2))
        .thread_stack_size(1024 * 1024)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[daemon] Failed to start async runtime: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = runtime.block_on(run()) {
        eprintln!("[daemon] Fatal error: {e}");
        std::process::exit(1);
    }
}

fn env_str(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

fn env_u16(key: &str, default: u16) -> u16 {
    env_str(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_bool(key: &str) -> bool {
    env_str(key).map(|v| v == "true").unwrap_or(false)
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let daemon_port = env_u16("DAEMON_PORT", 10300);
    let secret = match env_str("DAEMON_SECRET_KEY") {
        Some(s) => s,
        None => {
            eprintln!("DAEMON_SECRET_KEY environment variable is required");
            std::process::exit(1);
        }
    };
    let webui_enabled = env_bool("WEBUI_ENABLED");
    let webui_port = env_u16("WEBUI_PORT", 10400);
    let webui_username = env_str("WEBUI_USERNAME").unwrap_or_else(|| "admin".to_string());
    let webui_password = env_str("WEBUI_PASSWORD").unwrap_or_else(|| "admin".to_string());

    // services/, webui/ and logs/ are resolved relative to the working
    // directory, like the original `path.resolve('logs')`.
    let base_dir = std::env::current_dir()?;
    let config_loader = Arc::new(ConfigLoader::new(base_dir.join("services")));
    let service_manager = Arc::new(ServiceManager::new(
        config_loader.clone(),
        base_dir.join("logs"),
    ));

    // Daemon API server.
    let daemon_router = daemon_server::create_router(
        service_manager.clone(),
        config_loader.clone(),
        secret,
    );
    let daemon_listener = tokio::net::TcpListener::bind(("0.0.0.0", daemon_port)).await?;
    println!("[daemon] Listening on port {daemon_port}");
    let daemon_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(daemon_listener, daemon_router).await {
            eprintln!("[daemon] HTTP server error: {e}");
        }
    });

    // Optional Web UI server.
    let webui_handle = if webui_enabled {
        let state = Arc::new(WebUIState::new(
            service_manager.clone(),
            config_loader.clone(),
            webui_username,
            webui_password,
            base_dir.join("webui"),
        ));
        state.start_cleanup();
        let webui_router = webui_server::create_router((*state).clone());
        let webui_listener = tokio::net::TcpListener::bind(("0.0.0.0", webui_port)).await?;
        println!("[webui] Web UI available at http://localhost:{webui_port}/webui/");
        let handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(
                webui_listener,
                webui_router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            {
                eprintln!("[webui] HTTP server error: {e}");
            }
        });
        Some(handle)
    } else {
        None
    };

    // Start services marked autoStart, highest priority first.
    let mut autostart: Vec<(String, ServiceConfig)> = config_loader
        .load_all_service_configs()
        .into_iter()
        .filter(|(_, c)| c.auto_start)
        .collect();
    autostart.sort_by(|a, b| b.1.priority.cmp(&a.1.priority));
    for (name, config) in autostart {
        let result = service_manager.start(&name, &config, false);
        if result.success {
            println!("[daemon] Auto-started service: {name}");
        }
    }

    shutdown_signal().await;

    println!("\n[daemon] Shutting down...");
    service_manager.stop_all();
    daemon_handle.abort();
    if let Some(handle) = webui_handle {
        handle.abort();
    }
    Ok(())
}

/// Wait for Ctrl+C or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
