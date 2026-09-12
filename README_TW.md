# Rust Daemon

![Rust](https://img.shields.io/badge/Rust-2021-orange)
![Tokio](https://img.shields.io/badge/Tokio-1.x-ff69b4)
![Axum](https://img.shields.io/badge/Axum-0.8-7A8BFF)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-2ea44f)
![Build](https://img.shields.io/badge/Release-LTO%20%2B%20strip-blue)
![License](https://img.shields.io/badge/License-MIT-green)

**語言切換：** [English](README.md) | [简体中文](README_CN.md) | 繁體中文

一個以 Rust 撰寫的**跨平台服務守護程序**，由 Node.js 專案 `NodeDaemon` 移植而來。它能讓你的常駐程序保持存活、在異常結束時自動重新啟動、寫入輪替日誌，並提供可腳本化的 HTTP API 與可選的 Web UI——全部打包在一個小巧的原生二進位檔裡。

## 特性

- **程序監管** —— 透過系統 shell 啟動服務，停止時殺掉整棵程序樹（Windows `taskkill /T`，Unix 程序群組）
- **自動重啟** —— 異常結束後自動重新啟動（延遲 1 秒、`maxRetries` 上限、守護程序主動停止時不會誤重啟）
- **輪替日誌** —— 每個服務依天輪替，附保留期清理
- **預設安全** —— HMAC-SHA256 簽章的控制 API、工作階段 + CSRF 保護的 Web UI、登入限流、路徑穿越防護
- **雙介面** —— 機器 API（`/service/control/*`）與瀏覽器儀表板（`/webui/`），介面支援 英文 / 简 / 繁
- **伺服器狀態** —— CPU / 記憶體 / 執行時間快照端點
- **小巧快速** —— LTO + strip 的 release 建置、裁剪依賴 feature、限制 worker 執行緒數

## 技術棧

`Rust` · `Tokio` · `Axum` · `serde/serde_json` · `sysinfo` · `hmac/sha2` —— 零 C 依賴，不需要 Node.js 執行環境。

## 快速開始

### 環境需求

- [Rust 工具鏈](https://rustup.rs)（stable）—— 原生建置
- [Zig](https://ziglang.org/) + `cargo-zigbuild` —— **僅**交叉編譯時需要

### 建置與執行

```bash
cargo build --release
cp target/release/rust-daemon .
```

在可執行檔旁邊建立 `.env`（見[設定](#設定)），準備好服務定義，然後執行：

```bash
./rust-daemon
```

Windows 上在主控台執行 `rust-daemon.exe`，或設定為排程工作 / 系統服務。

## 交叉編譯

從 **Windows** 交叉編譯到 **Linux**（靜態 musl，目標機無需 glibc）：

```powershell
cargo install cargo-zigbuild
rustup target add x86_64-unknown-linux-musl
cargo zigbuild --release --target x86_64-unknown-linux-musl
# ARM Linux（樹莓派 / ARM 伺服器）
rustup target add aarch64-unknown-linux-musl
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

從 **Linux** 交叉編譯到 **Windows**：

```bash
rustup target add x86_64-pc-windows-gnu
cargo zigbuild --release --target x86_64-pc-windows-gnu
# 或安裝 mingw-w64 後：
cargo build --release --target x86_64-pc-windows-gnu
```

## 一鍵建置（Windows）

[build-all.ps1](build-all.ps1) 一次執行同時建置兩個平台，並用帶版本號的檔名打包攜帶式 zip：

```powershell
.\build-all.ps1          # Windows + Linux，然後打包 zip
.\build-all.ps1 -NoZip   # 只要裸二進位檔
```

版本號自動從 `Cargo.toml` 讀取（如 `1.2.0`），產物：

```
dist/
├── rust-daemon-v1.2.0-windows-x64.exe
├── rust-daemon-v1.2.0-linux-x64
├── rust-daemon-v1.2.0-windows-x64.zip   # 二進位檔 + .env.example + services/ + webui/
└── rust-daemon-v1.2.0-linux-x64.zip
```

### 發佈目錄結構

守護程序**相對於工作目錄**解析 `services/`、`webui/`、`logs/` 與 `.env`，因此發佈套件應如下組織：

```
rust-daemon(.exe)
├── .env
├── services/          # 每個服務一個 JSON 檔案
└── webui/             # 前端靜態資源（僅 WEBUI_ENABLED=true 時需要）
```

## 部署指南

### Linux — systemd（推薦）

將 Linux 產物部署到 `/opt/rust-daemon`，然後註冊 systemd 單元，讓守護程序開機自啟並保持存活：

```bash
sudo mkdir -p /opt/rust-daemon/services
# 解壓 Linux 套件，然後把服務 JSON 和 .env 拷進去
sudo cp services/*.json /opt/rust-daemon/services/
sudo chmod +x /opt/rust-daemon/rust-daemon
sudo cp .env /opt/rust-daemon/.env    # 或使用下面的 EnvironmentFile 並省略此行
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
# 查看守護程序自身日誌
sudo journalctl -u rust-daemon -f
```

注意：`WorkingDirectory` 很關鍵——`services/`、`webui/`、`logs/` 都相對它解析。上面的 `Restart=always` 保護的是守護程序本身；各服務的自動重啟在每個服務 JSON 裡設定。

### Windows

**方案 A — 主控台（快速測試）：** 直接在終端機執行 `rust-daemon.exe`。

**方案 B — 排程工作（開機自啟）：**
1. 開啟工作排程器 → 建立工作
2. 觸發程序：*電腦啟動時*；動作：執行 `rust-daemon.exe`，"開始於"填發佈目錄

**方案 C — NSSM 註冊為服務（推薦，守護程序當掉時自動拉起）：**

```powershell
nssm install rust-daemon "C:\path\to\rust-daemon.exe"
nssm set rust-daemon AppDirectory "C:\path\to"
nssm start rust-daemon
```

## 設定

### `.env`

| 變數 | 預設值 | 說明 |
|---|---|---|
| `DAEMON_PORT` | `10300` | 簽章控制 API 的連接埠 |
| `DAEMON_SECRET_KEY` | *（必填）* | 與 API 客戶端共用的 HMAC 金鑰 |
| `WEBUI_ENABLED` | `false` | 設為 `true` 啟動 Web UI |
| `WEBUI_PORT` | `10400` | Web UI 連接埠 |
| `WEBUI_USERNAME` | `admin` | Web UI 登入名稱 |
| `WEBUI_PASSWORD` | `admin` | Web UI 登入密碼 |
| `WEBUI_LANG` | `en` | 預設介面語言（`en` / `zh-CN` / `zh-TW`） |

### 服務定義 —— `services/<name>.json`

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

| 欄位 | 預設值 | 說明 |
|---|---|---|
| `cwd` | `.` | 指令的工作目錄 |
| `command` | *（必填）* | 完整 shell 指令 |
| `priority` | `0` | 自動啟動順序，`0`–`999`，數值越大越先啟動 |
| `maxRetries` | `3` | 允許連續異常結束自動重啟的次數 |
| `retryOnAbnormalExit` | `true` | 是否啟用自動重啟 |
| `autoStart` | `false` | 守護程序啟動時自動拉起 |
| `log.enabled` | `false` | 輸出寫入 `logs/<name>/` |
| `log.retentionDays` | `7` | 超過該天數的日期日誌會被刪除 |

## 文件

| 主題 | English | 简体中文 | 繁體中文 |
|---|---|---|---|
| 技術實作 | [technical.md](docs/technical.md) | [technical_CN.md](docs/technical_CN.md) | [technical_TW.md](docs/technical_TW.md) |
| 守護程序 HTTP API（HMAC） | [api.md](docs/api.md) | [api_CN.md](docs/api_CN.md) | [api_TW.md](docs/api_TW.md) |
| Web UI 操作指南 | [webui.md](docs/webui.md) | [webui_CN.md](docs/webui_CN.md) | [webui_TW.md](docs/webui_TW.md) |

## 專案結構

```
├── Cargo.toml           # 依賴裁剪到實際使用的 feature；release 設定（LTO/strip）
├── .env.example
├── services/            # 服務定義（含 example.json）
├── webui/               # 前端（index.html, js/, css/, lang/）
└── src/
    ├── main.rs          # 進入點：env、runtime、雙伺服器、自動啟動、結束處理
    ├── config.rs        # 服務設定的載入/儲存/驗證
    ├── service_manager.rs # 程序生命週期 + 自動重啟
    ├── logger.rs        # 依天輪替日誌
    ├── daemon_server.rs # HMAC 簽章的控制 API
    ├── webui_server.rs  # 工作階段/CSRF + Web UI API + 靜態檔案
    ├── os_status.rs     # CPU/記憶體/執行時間快照
    └── util.rs          # 共用工具
```

## License

MIT
