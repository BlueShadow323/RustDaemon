# Web UI 操作说明

守护进程的浏览器管理界面。由 `webui_server` 监听在 `WEBUI_PORT`（默认 **10400**），可以启动/停止/重启服务、新增/编辑/删除服务配置、查看服务器资源占用——完全不需要编写 HTTP API 客户端或签名代码。

**语言切换：** [English](webui.md) | 简体中文 | [繁體中文](webui_TW.md)

---

## 1. 启用 Web UI

Web UI **默认关闭**。在 `.env` 中设置：

```env
WEBUI_ENABLED=true
WEBUI_PORT=10400
WEBUI_USERNAME=admin
WEBUI_PASSWORD=your-strong-password-here
WEBUI_LANG=en
```

修改 `.env` 后重启守护进程。

## 2. 访问与登录

1. 浏览器打开 `http://<host>:10400/webui/`（访问 `/` 会自动跳转到这里）。
2. 用 `.env` 里配置的账号登录（默认 `admin` / `admin`）。
3. 登录成功后设置两个 cookie：`session-id`（24 小时有效，`HttpOnly`）和 `csrf-token`。

> **登录限流：** 同一 IP 连续输错 5 次，会被封禁 1 分钟。
> **多语言：** 右上角 **EN / 简 / 繁** 按钮可切换界面语言，选择会保存在 `localStorage` 中。

## 3. 仪表盘功能

### 3.1 服务卡片

仪表盘为 `services/*.json` 中每个服务显示一张卡片：

| 元素 | 含义 |
|---|---|
| 状态徽章 | `running`（绿）/ `stopped`（灰），进行中的操作如 `starting`、`stopping`、`restarting` |
| 运行时长 | 人类可读的 uptime（如 `1h 5m 30s`） |
| 重试次数 | `abnormalExitCount` |
| 命令 | 配置的启动命令 |

卡片操作按钮：

- **启动 / 停止 / 重启 / 强制终止** —— 一键操作，结果以 toast 提示。
- **编辑** —— 打开预填的服务表单；服务必须先停止。
- **删除** —— 确认弹窗；服务必须先停止。

### 3.2 服务器状态

顶部面板显示 CPU 百分比、内存占用（总量/已用/空闲）、系统运行时长、平台和主机名，每次加载仪表盘时刷新。

### 3.3 新增服务

点击 **Add Service** 填写表单：

| 字段 | 说明 |
|---|---|
| 服务 ID | 唯一名称，作为配置文件文件名 `services/<id>.json`；只允许字母、数字、`-` 和 `_` |
| 命令 | 完整 shell 命令，如 `npm run dev` |
| 工作目录 | 命令运行目录；`.` 表示守护进程的工作目录 |
| 优先级 | 自动启动顺序，`0`–`999`，数值越大越先启动 |
| 最大重试次数 | 连续异常退出自动重启的上限（默认 3） |
| 自动启动 | 守护进程启动时自动拉起 |
| 异常退出重试 | 是否在异常退出后自动重启 |
| 启用日志 | 服务输出写入 `logs/<name>/` |
| 日志保留天数 | 超过该天数的日期日志会被删除（默认 7） |

## 4. Web UI API（前端使用）

所有 API 路径都在 `/webui/api/*` 下。它们要求**会话 cookie**，并且 `X-CSRF-Token` 请求头必须与 `csrf-token` cookie 匹配。响应格式与守护进程 API 一致：`{ code, message, data? }`。

### 4.1 `POST /webui/api/login`

```json
{ "username": "admin", "password": "..." }
```

成功（`200`）时服务器设置 `session-id` 和 `csrf-token` 两个 cookie，`data` 返回 `{ "username": "admin" }`。

- `400` —— 缺少用户名/密码
- `401` —— 凭据错误
- `429` —— 触发限流

### 4.2 `POST /webui/api/logout`

使会话失效并清除 cookie。

### 4.3 `GET /webui/api/services`

所有服务的状态 + 配置（无请求体）。

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

创建服务。请求体：`{ "id": "my-service", "command": "...", "cwd": "...", "priority": 0, "maxRetries": 3, "retryOnAbnormalExit": true, "autoStart": false, "log": { "enabled": true, "retentionDays": 7 } }`

- `400` —— id 非法（只允许字母、数字、`-`、`_`）
- `409` —— id 已存在

### 4.5 `POST /webui/api/service/{name}/edit`

更新服务。字段与 `new` 相同，但 id 始终取自 URL。服务必须已停止且没有进行中的操作。

### 4.6 `POST /webui/api/service/{name}/delete`

删除服务配置文件。服务必须已停止且空闲。

### 4.7 `POST /webui/api/service/{name}/{action}`

`action` 取值为 `start` | `stop` | `restart` | `force-stop`。结果消息在 `message` 字段返回。

### 4.8 `GET /webui/api/server-status`

与守护进程 API 的 `POST /os/status` 载荷相同（CPU / 内存 / 运行时长 / 平台 / 主机名）。

## 5. 界面中的错误处理

- 任何 API 返回 `401` 都会把界面弹回登录页。
- 请求进行中时，操作按钮会被禁用。
- 冲突错误（服务仍在运行 / 操作进行中）会以消息形式提示（例如运行中不允许删除/编辑）。
- 网络失败会显示 "network error" 提示。

## 6. 安全说明

- 会话 24 小时后过期，过期会话每小时清理一次。
- 所有状态变更请求都必须通过 CSRF 双重校验（cookie **和** header）。
- 响应携带 `nosniff` / `DENY` 防嵌框 / 禁止缓存等安全头。
- 静态文件只能从 `webui/` 目录提供，拒绝路径穿越。
