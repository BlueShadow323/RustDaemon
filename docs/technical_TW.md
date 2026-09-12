# 技術實作說明

一個以 Rust 撰寫的跨平台服務守護程序，由 Node.js 專案 `NodeDaemon` 移植而來。它負責管理常駐程序、異常結束時自動重新啟動、依天輪替日誌，並對外提供以 HMAC 簽章保護的 HTTP API 以及可選的需登入 Web UI。

**語言切換：** [English](technical.md) | [简体中文](technical_CN.md) | 繁體中文

---

## 1. 架構總覽

守護程序在**同一個 Tokio runtime** 內執行**兩個獨立的 HTTP 伺服器**：

| 伺服器 | 連接埠（預設） | 用途 |
|---|---|---|
| 守護程序 API（`daemon_server.rs`） | `DAEMON_PORT` = 10300 | 機器對機器的控制 API，由 HMAC-SHA256 簽章保護 |
| Web UI（`webui_server.rs`） | `WEBUI_PORT` = 10400 | 面向人的管理介面，具備登入、工作階段與 CSRF 保護 |

兩個伺服器共用同一個 `Arc<ServiceManager>` 與 `Arc<ConfigLoader>`，因此無論從哪個介面發出控制指令，操作的都是同一份程序登錄表。

```
                 ┌────────────────────────── Tokio runtime ──────────────────────────┐
  POST /service/... │  daemon_server.rs     webui_server.rs                          │
  ─────────────────┼─────────────────────────────────────────────────────────────────┤
                 │         └──────────────┬──────────────┘                          │
                 │                        ▼                                          │
                 │              ServiceManager（共用）                                │
                 │   spawn / kill / restart / status / auto-restart                 │
                 │                        │                                          │
                 │               ConfigLoader（services/*.json）                      │
                 │                        │                                          │
                 │               ServiceLogger（logs/<name>/）                        │
                 └───────────────────────────────────────────────────────────────────┘
```

## 2. 模組說明

| 模組 | 職責 |
|---|---|
| `main.rs` | 進入點：載入 `.env`、建立 runtime、綁定兩個伺服器、自動啟動服務、處理結束 |
| `config.rs` | 解析/驗證 `services/*.json` 服務定義，套用預設值並做數值鉗制 |
| `service_manager.rs` | 程序生命週期：啟動、停止、強制停止、重新啟動、狀態查詢、異常結束自動重啟 |
| `logger.rs` | 每個服務獨立的依天輪替檔案日誌，附保留期清理 |
| `daemon_server.rs` | HMAC 保護的 API 端點 + CORS 中介層 |
| `webui_server.rs` | 登入/工作階段/CSRF 管理、Web UI API 端點、靜態檔案服務 |
| `os_status.rs` | 透過 `sysinfo` 採集 CPU/記憶體/執行時間 |
| `util.rs` | 共用工具：JSON 回應、常數時間比較、MIME 對照、cookie 解析 |

## 3. 程序管理

### 3.1 程序啟動

服務指令**透過系統 shell 執行**，與原版 `spawn(cmd, { shell: true })` 一致：

- **Windows**：`cmd /C <command>`，並加上 `CREATE_NO_WINDOW` 旗標，避免彈出主控台視窗。
- **Unix**：`sh -c <command>`，並把子程序放入**獨立的程序群組**（`setpgid`），這樣可以用一條 `killpg` 殺掉整棵程序樹。

stdout/stderr 被管線化，兩個背景任務（`spawn_output_pump`）持續把管線輸出寫入服務日誌。

### 3.2 終止程序樹

`kill_process_tree(pid, force)`：

- **Windows**：先 `taskkill /pid <pid> /T`（優雅），再 `/T /F`（強制）。`/T` 遞迴終止以服務 shell 為根節點的整棵程序樹。
- **Unix**：先發送優雅的 `SIGTERM`，再發送 `SIGKILL`。因為服務執行在獨立程序群組中，訊號能到達所有子孫程序。

### 3.3 狀態追蹤

每個服務都會追蹤：

- **running** —— `processes` 表中是否有對應的 pid
- **operation** —— 操作進行中時為 `"starting"`、`"stopping"`、`"restarting"`、`"force-stopping"`
- **duration** —— 已執行秒數
- **abnormalExitCount** —— 意外結束次數

### 3.4 異常結束自動重啟

每個服務有一個背景**監看任務**在 `child.wait()` 上等待。程序結束時：

1. 如果 `stopping` 集合包含該服務（停止/重啟由守護程序發起），監看任務直接返回，不重啟。
2. 否則，除非結束碼為 0 或終止訊號是 `SIGTERM`/`SIGKILL`（Unix），否則視為異常結束。
3. 異常結束後，**重試計時器**在 1 秒後觸發，重新從磁碟載入最新設定並再次 `start`——最多連續失敗 `maxRetries` 次（預設 3）。

`stopping` 旗標是關鍵防線：它防止守護程序自己剛殺掉的服務被監看任務重新拉起，否則會陷入無限重啟迴圈。

### 3.5 並行注意事項

- 所有共用狀態都以 `std::sync::Mutex` 保護，鎖的持有範圍刻意保持很短（沒有任何鎖跨 `.await` 持有）。
- `start`/`stop`/`restart`/`force-stop` 都是**同步方法**——只負責派發背景任務並立即回傳，因此可以在任何 async handler 中安全呼叫。
- 開發過程中修復過一個隱蔽陷阱：`if let Some(x) = mutex.lock()...` 會把暫時的 `MutexGuard` 生命週期**延長到整個 if 區塊**，導致對同一把鎖再次 `lock()` 時**死鎖**（std 的 Mutex 不可重入）。現在所有 pid 查詢都先把 pid 複製出來、釋放鎖，再進行後續加鎖操作。

## 4. 執行緒與記憶體模型

`main.rs` 建構的是 Tokio 多執行緒 runtime：

- worker 執行緒數上限為 `min(4, CPU 核心數)`——守護程序是 I/O 密集，4 個 worker 綽綽有餘
- 每執行緒堆疊從預設 2 MiB 降到 1 MiB

這樣在多核心機器上也能維持較低的記憶體常駐占用。

## 5. 安全設計

| 層 | 機制 |
|---|---|
| 守護程序 API | HMAC-SHA256 對 `X-Timestamp + body` 簽章（共用金鑰）；時間戳必須在 5 分鐘以內；簽章以常數時間比較 |
| Web UI 登入 | 工作階段 cookie（`session-id`，24 小時有效，`HttpOnly`）+ CSRF cookie |
| CSRF | `csrf-token` cookie 與 `X-CSRF-Token` 請求標頭必須同時符合登入時簽發的令牌 |
| 登入限流 | 同一 IP 連續失敗 5 次 → 封鎖 1 分鐘 |
| 回應標頭 | `X-Content-Type-Options: nosniff`、`X-Frame-Options: DENY`、`Referrer-Policy`、禁止快取 |
| 靜態檔案 | 拒絕路徑穿越（只允許 `Component::Normal` 路徑段） |

## 6. 日誌系統

每個服務寫入 `logs/<name>/current.log`。日期輪替時（`open()`），內容會合併進 `logs/<name>/<YYYYMMDD>.log`，`current.log` 被清空。超過 `retentionDays` 的日期日誌會被刪除，`current.log` 始終保留。所有寫入都是同步的，每行帶 ISO 時間戳前綴。

## 7. 系統狀態採集

`get_server_status()` 使用 `sysinfo`：

- CPU 使用率在**100 毫秒視窗**內取樣（兩次 `refresh_cpu_usage()`），對應原版的兩次 `os.cpus()` 讀取
- 記憶體總量/已用/閒置 + 使用率百分比
- 系統執行時間、平台（`std::env::consts`）、主機名稱

## 8. 發佈最佳化

`Cargo.toml` 內建一套針對體積與速度調校的 release 設定：

```toml
[profile.release]
opt-level = 3       # 最大化程式碼最佳化
lto = "fat"         # 跨 crate 連結期最佳化
codegen-units = 1   # 更好的最佳化，編譯較慢
panic = "abort"     # 不產生 unwind 表 -> 二進位檔更小
strip = true        # 去除符號表
```

依賴裁剪到只保留實際使用的 feature（例如 Tokio 不再是 `full`，而是精確的 `rt-multi-thread, macros, signal, process, io-util, time, net`；`sysinfo` 去掉以 rayon 為基礎的 `multithread`；`chrono` 只留 `clock`）。這能明顯縮小二進位檔體積並縮短編譯時間。

## 9. 資料流範例

透過守護程序 API 重新啟動服務 `example`：

1. 收到 `POST /service/control/example`，請求主體為 `{"type":"restart"}`。
2. `verify_signature` 用金鑰重新計算 `HMAC-SHA256(secret, timestamp + body)` 並與 `X-Signature` 比對。
3. `restart()` 把 `example` 加入 `stopping`，終止目前程序樹，然後呼叫 `start()`。
4. `start()` 啟動新的 shell 程序，登記 pid 與啟動時間，並派發新的監看任務。
5. 清除 `stopping` 旗標；之後若再次異常結束，監看任務仍會依規則自動重啟。
