# Technical Implementation

A cross-platform service daemon written in Rust, ported from the Node.js project `NodeDaemon`. It manages long-running processes, restarts them on abnormal exit, writes rotating logs, exposes an HMAC-signed HTTP API and an optional authenticated Web UI.

**Language:** English | [简体中文](technical_CN.md) | [繁體中文](technical_TW.md)

---

## 1. Architecture Overview

The daemon runs **two independent HTTP servers** inside one Tokio runtime:

| Server | Port (default) | Purpose |
|---|---|---|
| Daemon API (`daemon_server.rs`) | `DAEMON_PORT` = 10300 | Machine-to-machine control API, protected by HMAC-SHA256 signatures |
| Web UI (`webui_server.rs`) | `WEBUI_PORT` = 10400 | Human-facing management UI with login, session and CSRF protection |

Both servers share a single `Arc<ServiceManager>` and `Arc<ConfigLoader>`, so a control command issued through either interface acts on the same process registry.

```
                 ┌────────────────────────── Tokio runtime ──────────────────────────┐
  POST /service/... │  daemon_server.rs     webui_server.rs                          │
  ─────────────────┼─────────────────────────────────────────────────────────────────┤
                 │         └──────────────┬──────────────┘                          │
                 │                        ▼                                          │
                 │              ServiceManager (shared)                              │
                 │   spawn / kill / restart / status / auto-restart                 │
                 │                        │                                          │
                 │               ConfigLoader (services/*.json)                      │
                 │                        │                                          │
                 │               ServiceLogger (logs/<name>/)                        │
                 └───────────────────────────────────────────────────────────────────┘
```

## 2. Module Map

| Module | Responsibility |
|---|---|
| `main.rs` | Entry point: loads `.env`, builds the runtime, binds both servers, auto-starts services, handles shutdown |
| `config.rs` | Parses/validates `services/*.json` definitions, applies defaults and clamps |
| `service_manager.rs` | Process lifecycle: start, stop, force-stop, restart, status, abnormal-exit auto-restart |
| `logger.rs` | Per-service daily rotating file logging with retention cleanup |
| `daemon_server.rs` | HMAC-protected API endpoints + CORS middleware |
| `webui_server.rs` | Login/session/CSRF management, Web UI API endpoints, static file serving |
| `os_status.rs` | CPU/memory/uptime snapshot via `sysinfo` |
| `util.rs` | Shared helpers: JSON responses, constant-time compare, MIME map, cookie parser |

## 3. Process Management

### 3.1 Spawning

A service command is executed **through the system shell**, matching the original `spawn(cmd, { shell: true })`:

- **Windows**: `cmd /C <command>` with the `CREATE_NO_WINDOW` flag so no console window pops up.
- **Unix**: `sh -c <command>` and the child is placed into its own **process group** (`setpgid`), which makes the whole tree killable with a single `killpg`.

stdout/stderr are piped; two background tasks (`spawn_output_pump`) continuously drain the pipes into the service log.

### 3.2 Killing the process tree

`kill_process_tree(pid, force)`:

- **Windows**: `taskkill /pid <pid> /T` (graceful) then `/T /F` (forced). `/T` recursively terminates the whole tree rooted at the service shell.
- **Unix**: a graceful `SIGTERM` first, then `SIGKILL`. Because the service runs in its own process group, the signal reaches every descendant.

### 3.3 Status tracking

Every service tracks:

- **running** – pid present in the `processes` map
- **operation** – `"starting"`, `"stopping"`, `"restarting"`, `"force-stopping"` while an operation is in flight
- **duration** – uptime in seconds
- **abnormalExitCount** – number of unexpected exits

### 3.4 Abnormal-exit auto-restart

A background **watcher task** per service awaits `child.wait()`. When the process exits:

1. If the `stopping` set contains the service (a stop/restart was initiated by the daemon), the watcher returns without restarting.
2. Otherwise the exit is "abnormal" unless `code == 0` or the kill signal was `SIGTERM`/`SIGKILL` (Unix).
3. On abnormal exit, a **retry timer** fires after 1 second, reloads a fresh config from disk, and calls `start` again — up to `maxRetries` consecutive failures (default 3).

The `stopping` flag is the key guard: it prevents the watcher from restarting a service that the daemon itself just killed, which would otherwise cause an infinite restart loop.

### 3.5 Concurrency notes

- All shared state is guarded by `std::sync::Mutex`; lock scopes are deliberately short (no lock is held across an `.await`).
- `start`/`stop`/`restart`/`force-stop` are **synchronous** methods — they only spawn background tasks and return. This keeps them safe to call from any async handler.
- One subtle pitfall was fixed during development: `if let Some(x) = mutex.lock()...` extends the temporary `MutexGuard` lifetime to the whole `if` block, which made a second `lock()` on the same mutex **deadlock** (std mutexes are not reentrant). All pid lookups now copy the pid out first, releasing the lock before further locking.

## 4. Threading & Memory Model

`main.rs` builds a Tokio multi-thread runtime with:

- worker threads capped at `min(4, CPU cores)` — the daemon is I/O-bound, so 4 workers are ample
- a per-thread stack of 1 MiB instead of the default 2 MiB

This keeps the resident memory footprint low on machines with many cores.

## 5. Security

| Layer | Mechanism |
|---|---|
| Daemon API | HMAC-SHA256 signature over `X-Timestamp + body` with the shared secret; timestamp must be within 5 minutes; signatures compared in constant time |
| Web UI login | Session cookie (`session-id`, 24 h lifetime, `HttpOnly`) + CSRF cookie |
| CSRF | The `csrf-token` cookie **and** the `X-CSRF-Token` header must both match the token issued at login |
| Login rate limit | 5 failed attempts from one IP → blocked for 1 minute |
| Response headers | `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy`, `no-store` caching |
| Static files | Path traversal is rejected (only `Component::Normal` segments allowed) |

## 6. Logging

Each service writes to `logs/<name>/current.log`. On date rollover (`open()`), the content is merged into `logs/<name>/<YYYYMMDD>.log` and `current.log` is truncated. Dated logs older than `retentionDays` are deleted; `current.log` is always kept. All writes are synchronous and line-prefixed with an ISO timestamp.

## 7. System Status

`get_server_status()` uses `sysinfo`:

- CPU usage sampled across a **100 ms window** (two `refresh_cpu_usage()` reads), mirroring the original double `os.cpus()` read
- Memory total/used/free + usage percentage
- System uptime, platform (`std::env::consts`), and hostname

## 8. Release Optimizations

`Cargo.toml` ships a release profile tuned for size and speed:

```toml
[profile.release]
opt-level = 3       # max code optimization
lto = "fat"         # cross-crate link-time optimization
codegen-units = 1   # better optimization, slower build
panic = "abort"     # no unwind tables -> smaller binary
strip = true        # strip symbols
```

Dependencies are trimmed to only the features actually used (e.g. Tokio is no longer `full`, but exactly `rt-multi-thread, macros, signal, process, io-util, time, net`; `sysinfo` drops the rayon-based `multithread` feature; `chrono` keeps only `clock`). This noticeably shrinks the binary and reduces compile time.

## 9. Data Flow Example

Restarting service `example` through the daemon API:

1. `POST /service/control/example` with `{"type":"restart"}` arrives.
2. `verify_signature` recomputes `HMAC-SHA256(secret, timestamp + body)` and compares it to `X-Signature`.
3. `restart()` adds `example` to `stopping`, kills the current process tree, then calls `start()`.
4. `start()` spawns a new shell process, registers pid + start time, and spawns a fresh watcher task.
5. The `stopping` flag is cleared; the watcher will still auto-restart the service on a later abnormal exit.
