# Rust Daemon

![Rust](https://img.shields.io/badge/Rust-2021-orange)
![Tokio](https://img.shields.io/badge/Tokio-1.x-ff69b4)
![Axum](https://img.shields.io/badge/Axum-0.8-7A8BFF)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-2ea44f)
![Build](https://img.shields.io/badge/Release-LTO%20%2B%20strip-blue)
![License](https://img.shields.io/badge/License-MIT-green)

**Language:** English | [简体中文](README_CN.md) | [繁體中文](README_TW.md)

A cross-platform **service daemon** written in Rust, ported from the Node.js project `NodeDaemon`. It keeps your long-running processes alive, restarts them on abnormal exit, writes rotating logs, and exposes both a scriptable HTTP API and an optional Web UI — with a single small native binary.

## Features

- **Process supervision** — spawn services through the system shell, kill the whole process tree on stop (Windows `taskkill /T`, Unix process groups)
- **Auto-restart** — restart on abnormal exit (1 s delay, `maxRetries` limit, no restart when the daemon itself initiated the stop)
- **Rotating logs** — daily rollover per service, retention cleanup
- **Secure by default** — HMAC-SHA256 signed control API, session + CSRF protected Web UI, login rate limiting, path-traversal protection
- **Dual interface** — machine API (`/service/control/*`) and a browser dashboard (`/webui/`) with EN / 简 / 繁 languages
- **Server status** — CPU / memory / uptime snapshot endpoint
- **Small & fast** — LTO + strip release build, trimmed dependency features, capped worker threads

## Tech Stack

`Rust` · `Tokio` · `Axum` · `serde/serde_json` · `sysinfo` · `hmac/sha2` — zero C dependencies, no Node.js runtime required.

## Quick Start

### Prerequisites

- [Rust toolchain](https://rustup.rs) (stable) — for native builds
- [Zig](https://ziglang.org/) + `cargo-zigbuild` — **only** for cross-compiling

### Build & Run

```bash
cargo build --release
cp target/release/rust-daemon .
```

Create `.env` next to the binary (see [Configuration](#configuration)), prepare your service definitions, then run:

```bash
./rust-daemon
```

On Windows, run `rust-daemon.exe` in a console or as a scheduled task / service.

## Cross-Compilation

From **Windows** to **Linux** (static musl, no glibc needed on the target):

```powershell
cargo install cargo-zigbuild
rustup target add x86_64-unknown-linux-musl
cargo zigbuild --release --target x86_64-unknown-linux-musl
# ARM Linux (Raspberry Pi / ARM servers)
rustup target add aarch64-unknown-linux-musl
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

From **Linux** to **Windows**:

```bash
rustup target add x86_64-pc-windows-gnu
cargo zigbuild --release --target x86_64-pc-windows-gnu
# or with mingw-w64 installed:
cargo build --release --target x86_64-pc-windows-gnu
```

## One-command Build (Windows)

[`build-all.ps1`](build-all.ps1) builds both platforms and packages portable zips with versioned filenames in a single run:

```powershell
.\build-all.ps1          # Windows + Linux, then package zips
.\build-all.ps1 -NoZip   # keep only the raw binaries
```

The version tag is read automatically from `Cargo.toml` (e.g. `1.2.0`), producing:

```
dist/
├── rust-daemon-v1.2.0-windows-x64.exe
├── rust-daemon-v1.2.0-linux-x64
├── rust-daemon-v1.2.0-windows-x64.zip   # binary + .env.example + services/ + webui/
└── rust-daemon-v1.2.0-linux-x64.zip
```

### Release layout

The daemon resolves `services/`, `webui/`, `logs/` and `.env` **relative to its working directory**, so a release bundle looks like:

```
rust-daemon(.exe)
├── .env
├── services/          # one JSON file per service
└── webui/             # frontend assets (required only when WEBUI_ENABLED=true)
```

## Deployment

### Linux — systemd (recommended)

Deploy the Linux artifact to `/opt/rust-daemon`, then register a systemd unit so the daemon starts on boot and stays alive:

```bash
sudo mkdir -p /opt/rust-daemon/services
# unzip the Linux package, then copy your service JSONs and .env in
sudo cp services/*.json /opt/rust-daemon/services/
sudo chmod +x /opt/rust-daemon/rust-daemon
sudo cp .env /opt/rust-daemon/.env    # or set EnvironmentFile below and omit this
```

```ini
# /etc/systemd/system/rust-daemon.service
[Unit]
Description=Rust Daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=www-data
WorkingDirectory=/opt/rust-daemon
ExecStart=/opt/rust-daemon/rust-daemon
EnvironmentFile=/opt/rust-daemon/.env
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now rust-daemon
sudo systemctl status rust-daemon
# follow the daemon's own log
sudo journalctl -u rust-daemon -f
```

Notes: `WorkingDirectory` matters — `services/`, `webui/` and `logs/` are resolved relative to it. The `Restart=always` above guards the daemon process itself; per-service auto-restart is configured in each service JSON.

### Windows

**Option A — console (quick test):** run `rust-daemon.exe` in a terminal.

**Option B — Task Scheduler (start on boot):**
1. Open Task Scheduler → Create Task
2. Trigger: *At startup*; Action: run `rust-daemon.exe` with *Start in* set to the release folder

**Option C — NSSM service (recommended, restarts the daemon on crash):**

```powershell
nssm install rust-daemon "C:\path\to\rust-daemon.exe"
nssm set rust-daemon AppDirectory "C:\path\to"
nssm start rust-daemon
```

## Configuration

### `.env`

| Variable | Default | Description |
|---|---|---|
| `DAEMON_PORT` | `10300` | Port of the signed control API |
| `DAEMON_SECRET_KEY` | *(required)* | HMAC secret shared by API clients |
| `WEBUI_ENABLED` | `false` | Set `true` to start the Web UI |
| `WEBUI_PORT` | `10400` | Web UI port |
| `WEBUI_USERNAME` | `admin` | Web UI login name |
| `WEBUI_PASSWORD` | `admin` | Web UI login password |
| `WEBUI_LANG` | `en` | Default UI language (`en` / `zh-CN` / `zh-TW`) |

### Service definitions — `services/<name>.json`

```json
{
  "cwd": "/your-project",
  "command": "npm run dev",
  "priority": 999,
  "maxRetries": 3,
  "retryOnAbnormalExit": true,
  "autoStart": false,
  "log": { "enabled": true, "retentionDays": 7 }
}
```

| Field | Default | Description |
|---|---|---|
| `cwd` | `.` | working directory of the command |
| `command` | *(required)* | full shell command |
| `priority` | `0` | auto-start order, `0`–`999`, higher starts first |
| `maxRetries` | `3` | consecutive abnormal-exit restarts allowed |
| `retryOnAbnormalExit` | `true` | enable auto-restart |
| `autoStart` | `false` | start automatically with the daemon |
| `log.enabled` | `false` | write output to `logs/<name>/` |
| `log.retentionDays` | `7` | delete dated logs older than this |

## Documentation

| Topic | English | 简体中文 | 繁體中文 |
|---|---|---|---|
| Technical implementation | [technical.md](docs/technical.md) | [technical_CN.md](docs/technical_CN.md) | [technical_TW.md](docs/technical_TW.md) |
| Daemon HTTP API (HMAC) | [api.md](docs/api.md) | [api_CN.md](docs/api_CN.md) | [api_TW.md](docs/api_TW.md) |
| Web UI operation guide | [webui.md](docs/webui.md) | [webui_CN.md](docs/webui_CN.md) | [webui_TW.md](docs/webui_TW.md) |

## Project Structure

```
├── Cargo.toml           # deps trimmed to used features; release profile (LTO/strip)
├── .env.example
├── services/            # service definitions (example.json included)
├── webui/               # frontend (index.html, js/, css/, lang/)
└── src/
    ├── main.rs          # entry: env, runtime, servers, auto-start, shutdown
    ├── config.rs        # service config load/save/validate
    ├── service_manager.rs # process lifecycle + auto-restart
    ├── logger.rs        # daily rotating logs
    ├── daemon_server.rs # HMAC-signed control API
    ├── webui_server.rs  # session/CSRF + Web UI API + static files
    ├── os_status.rs     # CPU/memory/uptime snapshot
    └── util.rs          # shared helpers
```

## License

MIT
