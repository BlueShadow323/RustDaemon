# Rust Daemon

![Rust](https://img.shields.io/badge/Rust-2021-orange)
![Tokio](https://img.shields.io/badge/Tokio-1.x-ff69b4)
![Axum](https://img.shields.io/badge/Axum-0.8-7A8BFF)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-2ea44f)
![Build](https://img.shields.io/badge/Release-LTO%20%2B%20strip-blue)
![License](https://img.shields.io/badge/License-MIT-green)

**语言切换：** [English](README.md) | 简体中文 | [繁體中文](README_TW.md)

一个用 Rust 编写的**跨平台服务守护进程**，由 Node.js 项目 `NodeDaemon` 移植而来。它能保持你的常驻进程存活、在异常退出时自动重启、写入轮转日志，并提供可脚本化的 HTTP API 和可选的 Web UI——全部打包在一个小巧的原生二进制里。

## 特性

- **进程监管** —— 通过系统 shell 启动服务，停止时杀掉整棵进程树（Windows `taskkill /T`，Unix 进程组）
- **自动重启** —— 异常退出后自动重启（延迟 1 秒、`maxRetries` 上限、守护进程主动停止时不会误重启）
- **轮转日志** —— 每个服务按天轮转，带保留期清理
- **默认安全** —— HMAC-SHA256 签名的控制 API、会话 + CSRF 保护的 Web UI、登录限流、路径穿越防护
- **双接口** —— 机器 API（`/service/control/*`）和浏览器仪表盘（`/webui/`），界面支持 英文 / 简 / 繁
- **服务器状态** —— CPU / 内存 / 运行时长快照端点
- **小巧快速** —— LTO + strip 的 release 构建、裁剪依赖 feature、限制 worker 线程数

## 技术栈

`Rust` · `Tokio` · `Axum` · `serde/serde_json` · `sysinfo` · `hmac/sha2` —— 零 C 依赖，不需要 Node.js 运行时。

## 快速开始

### 环境要求

- [Rust 工具链](https://rustup.rs)（stable）—— 原生构建
- [Zig](https://ziglang.org/) + `cargo-zigbuild` —— **仅**交叉编译时需要

### 构建与运行

```bash
cargo build --release
cp target/release/rust-daemon .
```

在可执行文件旁边创建 `.env`（见[配置](#配置)），准备好服务定义，然后运行：

```bash
./rust-daemon
```

Windows 上在控制台运行 `rust-daemon.exe`，或配置为计划任务 / 系统服务。

## 交叉编译

从 **Windows** 交叉编译到 **Linux**（静态 musl，目标机无需 glibc）：

```powershell
cargo install cargo-zigbuild
rustup target add x86_64-unknown-linux-musl
cargo zigbuild --release --target x86_64-unknown-linux-musl
# ARM Linux（树莓派 / ARM 服务器）
rustup target add aarch64-unknown-linux-musl
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

从 **Linux** 交叉编译到 **Windows**：

```bash
rustup target add x86_64-pc-windows-gnu
cargo zigbuild --release --target x86_64-pc-windows-gnu
# 或安装 mingw-w64 后：
cargo build --release --target x86_64-pc-windows-gnu
```

## 一键构建（Windows）

[build-all.ps1](build-all.ps1) 一次运行同时构建两个平台，并用带版本号的文件名打包便携 zip：

```powershell
.\build-all.ps1          # Windows + Linux，然后打包 zip
.\build-all.ps1 -NoZip   # 只要裸二进制
```

版本号自动从 `Cargo.toml` 读取（如 `1.2.0`），产物：

```
dist/
├── rust-daemon-v1.2.0-windows-x64.exe
├── rust-daemon-v1.2.0-linux-x64
├── rust-daemon-v1.2.0-windows-x64.zip   # 二进制 + .env.example + services/ + webui/
└── rust-daemon-v1.2.0-linux-x64.zip
```

### 发布目录结构

守护进程**相对于工作目录**解析 `services/`、`webui/`、`logs/` 和 `.env`，因此发布包应如下组织：

```
rust-daemon(.exe)
├── .env
├── services/          # 每个服务一个 JSON 文件
└── webui/             # 前端静态资源（仅 WEBUI_ENABLED=true 时需要）
```

## 部署指南

### Linux — systemd（推荐）

将 Linux 产物部署到 `/opt/rust-daemon`，然后注册 systemd 单元，让守护进程开机自启并保持存活：

```bash
sudo mkdir -p /opt/rust-daemon/services
# 解压 Linux 包，然后把服务 JSON 和 .env 拷进去
sudo cp services/*.json /opt/rust-daemon/services/
sudo chmod +x /opt/rust-daemon/rust-daemon
sudo cp .env /opt/rust-daemon/.env    # 或使用下面的 EnvironmentFile 并省略此行
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
# 查看守护进程自身日志
sudo journalctl -u rust-daemon -f
```

注意：`WorkingDirectory` 很关键——`services/`、`webui/`、`logs/` 都相对它解析。上面的 `Restart=always` 保护的是守护进程本身；各服务的自动重启在每个服务 JSON 里配置。

### Windows

**方案 A — 控制台（快速测试）：** 直接在终端运行 `rust-daemon.exe`。

**方案 B — 计划任务（开机自启）：**
1. 打开任务计划程序 → 创建任务
2. 触发器：*计算机启动时*；操作：运行 `rust-daemon.exe`，"起始于"填发布目录

**方案 C — NSSM 注册为服务（推荐，守护进程崩溃时自动拉起）：**

```powershell
nssm install rust-daemon "C:\path\to\rust-daemon.exe"
nssm set rust-daemon AppDirectory "C:\path\to"
nssm start rust-daemon
```

## 配置

### `.env`

| 变量 | 默认值 | 说明 |
|---|---|---|
| `DAEMON_PORT` | `10300` | 签名控制 API 的端口 |
| `DAEMON_SECRET_KEY` | *（必填）* | 与 API 客户端共享的 HMAC 密钥 |
| `WEBUI_ENABLED` | `false` | 设为 `true` 启动 Web UI |
| `WEBUI_PORT` | `10400` | Web UI 端口 |
| `WEBUI_USERNAME` | `admin` | Web UI 登录名 |
| `WEBUI_PASSWORD` | `admin` | Web UI 登录密码 |
| `WEBUI_LANG` | `en` | 默认界面语言（`en` / `zh-CN` / `zh-TW`） |

### 服务定义 —— `services/<name>.json`

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

| 字段 | 默认值 | 说明 |
|---|---|---|
| `cwd` | `.` | 命令的工作目录 |
| `command` | *（必填）* | 完整 shell 命令 |
| `priority` | `0` | 自动启动顺序，`0`–`999`，数值越大越先启动 |
| `maxRetries` | `3` | 允许连续异常退出自动重启的次数 |
| `retryOnAbnormalExit` | `true` | 是否启用自动重启 |
| `autoStart` | `false` | 守护进程启动时自动拉起 |
| `log.enabled` | `false` | 输出写入 `logs/<name>/` |
| `log.retentionDays` | `7` | 超过该天数的日期日志会被删除 |

## 文档

| 主题 | English | 简体中文 | 繁體中文 |
|---|---|---|---|
| 技术实现 | [technical.md](docs/technical.md) | [technical_CN.md](docs/technical_CN.md) | [technical_TW.md](docs/technical_TW.md) |
| 守护进程 HTTP API（HMAC） | [api.md](docs/api.md) | [api_CN.md](docs/api_CN.md) | [api_TW.md](docs/api_TW.md) |
| Web UI 操作指南 | [webui.md](docs/webui.md) | [webui_CN.md](docs/webui_CN.md) | [webui_TW.md](docs/webui_TW.md) |

## 项目结构

```
├── Cargo.toml           # 依赖裁剪到实际用到的 feature；release 配置（LTO/strip）
├── .env.example
├── services/            # 服务定义（含 example.json）
├── webui/               # 前端（index.html, js/, css/, lang/）
└── src/
    ├── main.rs          # 入口：env、runtime、双服务器、自动启动、退出处理
    ├── config.rs        # 服务配置的加载/保存/校验
    ├── service_manager.rs # 进程生命周期 + 自动重启
    ├── logger.rs        # 按天轮转日志
    ├── daemon_server.rs # HMAC 签名的控制 API
    ├── webui_server.rs  # 会话/CSRF + Web UI API + 静态文件
    ├── os_status.rs     # CPU/内存/运行时长快照
    └── util.rs          # 共享工具
```

## License

MIT
