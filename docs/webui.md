# Web UI Operation Guide

A browser-based management interface for the daemon. It is served by the `webui_server` on `WEBUI_PORT` (default **10400**) and lets you start/stop/restart services, add/edit/delete service definitions and watch server resource usage — no HTTP API client or signing code required.

**Language:** English | [简体中文](webui_CN.md) | [繁體中文](webui_TW.md)

---

## 1. Enabling the Web UI

The Web UI is **off by default**. Set the following in `.env`:

```env
WEBUI_ENABLED=true
WEBUI_PORT=10400
WEBUI_USERNAME=admin
WEBUI_PASSWORD=your-strong-password-here
WEBUI_LANG=en
```

Restart the daemon after editing `.env`.

## 2. Accessing & Logging In

1. Open `http://<host>:10400/webui/` in a browser (visiting `/` redirects here automatically).
2. Log in with the account from `.env` (defaults: `admin` / `admin`).
3. After login, two cookies are set: `session-id` (24 h, `HttpOnly`) and `csrf-token`.

> **Rate limiting:** 5 failed login attempts from the same IP block the IP for 1 minute.
> **Language:** use the **EN / 简 / 繁** buttons in the top-right corner to switch the UI language — the setting is remembered in `localStorage`.

## 3. Dashboard Features

### 3.1 Service cards

The dashboard shows one card per service in `services/*.json`:

| Element | Meaning |
|---|---|
| Status badge | `running` (green) / `stopped` (gray), plus an in-flight operation like `starting`, `stopping`, `restarting` |
| Duration | uptime, humanized (`1h 5m 30s`) |
| Retries | `abnormalExitCount` |
| Command | the configured launch command |

Card actions:

- **Start** / **Stop** / **Restart** / **Force kill** — one click; the result is shown in a toast message.
- **Edit** — opens the service form pre-filled; the service must be stopped first.
- **Delete** — confirmation dialog; the service must be stopped first.

### 3.2 Server status

The top panel shows CPU %, memory usage (total / used / free), system uptime, platform and hostname, refreshed each time the dashboard loads.

### 3.3 Adding a service

Click **Add Service** and fill the form:

| Field | Description |
|---|---|
| Service ID | unique name used for the config file name, `services/<id>.json`; only letters, numbers, `-` and `_` |
| Command | full shell command, e.g. `npm run dev` |
| Working directory | where the command runs; `.` for the daemon's working directory |
| Priority | auto-start order, `0`–`999`, higher starts first |
| Max retries | consecutive abnormal-exit auto-restart limit (default 3) |
| Auto start | launch automatically when the daemon starts |
| Retry on abnormal exit | enable auto-restart on abnormal exit |
| Log enabled | write service output to `logs/<name>/` |
| Retention days | delete dated logs older than this (default 7) |

## 4. Web UI API (used by the frontend)

All API paths are under `/webui/api/*`. They require the **session cookie** plus the `X-CSRF-Token` header matching the `csrf-token` cookie. Responses use the same `{ code, message, data? }` envelope as the daemon API.

### 4.1 `POST /webui/api/login`

```json
{ "username": "admin", "password": "..." }
```

On success (`200`) the server sets `session-id` and `csrf-token` cookies and returns `{ "username": "admin" }` in `data`.

- `400` — missing username/password
- `401` — wrong credentials
- `429` — rate limited

### 4.2 `POST /webui/api/logout`

Invalidates the session and clears the cookies.

### 4.3 `GET /webui/api/services`

Status + configuration of every service (no body).

```json
{
  "example": {
    "running": true,
    "operation": null,
    "duration": 120,
    "abnormalExitCount": 0,
    "command": "npm run dev",
    "cwd": "/your-project",
    "autoStart": false,
    "priority": 999,
    "maxRetries": 3,
    "retryOnAbnormalExit": true,
    "log": { "enabled": true, "retentionDays": 7 }
  }
}
```

### 4.4 `POST /webui/api/service/new`

Create a service. Body: `{ "id": "my-service", "command": "...", "cwd": "...", "priority": 0, "maxRetries": 3, "retryOnAbnormalExit": true, "autoStart": false, "log": { "enabled": true, "retentionDays": 7 } }`

- `400` — invalid id (only letters, numbers, `-`, `_`)
- `409` — id already exists

### 4.5 `POST /webui/api/service/{name}/edit`

Update a service. Same fields as `new`, but the id always comes from the URL. The service must be stopped and have no in-flight operation.

### 4.6 `POST /webui/api/service/{name}/delete`

Delete the service config file. The service must be stopped and idle.

### 4.7 `POST /webui/api/service/{name}/{action}`

`action` ∈ `start` | `stop` | `restart` | `force-stop`. Result message is returned in `message`.

### 4.8 `GET /webui/api/server-status`

Same payload as the daemon API `POST /os/status` (CPU / memory / uptime / platform / hostname).

## 5. Error Handling in the UI

- Any API call returning `401` bounces the UI back to the login page.
- In-flight action buttons are disabled until the request finishes.
- Conflict errors (service still running / operation in progress) are surfaced as messages (e.g. delete/edit blocked while running).
- Network failures show a "network error" toast.

## 6. Security Notes

- Session expires after 24 h; expired sessions are cleaned up hourly.
- Every state-changing request must pass the CSRF double-check (cookie **and** header).
- Responses carry `nosniff` / `DENY` framing / no-store cache headers.
- Static files are served only from the `webui/` directory; path traversal is rejected.
