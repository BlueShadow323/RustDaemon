# Web UI 操作說明

守護程序的瀏覽器管理介面。由 `webui_server` 監聽在 `WEBUI_PORT`（預設 **10400**），可以啟動/停止/重新啟動服務、新增/編輯/刪除服務設定、查看伺服器資源占用——完全不需要撰寫 HTTP API 客戶端或簽章程式碼。

**語言切換：** [English](webui.md) | [简体中文](webui_CN.md) | 繁體中文

---

## 1. 啟用 Web UI

Web UI **預設關閉**。在 `.env` 中設定：

```env
WEBUI_ENABLED=true
WEBUI_PORT=10400
WEBUI_USERNAME=admin
WEBUI_PASSWORD=your-strong-password-here
WEBUI_LANG=en
```

修改 `.env` 後重新啟動守護程序。

## 2. 存取與登入

1. 瀏覽器開啟 `http://<host>:10400/webui/`（存取 `/` 會自動跳轉到這裡）。
2. 使用 `.env` 中設定的帳號登入（預設 `admin` / `admin`）。
3. 登入成功後會設定兩個 cookie：`session-id`（24 小時有效，`HttpOnly`）和 `csrf-token`。

> **登入限流：** 同一 IP 連續輸錯 5 次，會被封鎖 1 分鐘。
> **多語言：** 右上角 **EN / 简 / 繁** 按鈕可切換介面語言，選擇會儲存在 `localStorage` 中。

## 3. 儀表板功能

### 3.1 服務卡片

儀表板為 `services/*.json` 中每個服務顯示一張卡片：

| 元素 | 含義 |
|---|---|
| 狀態徽章 | `running`（綠）/ `stopped`（灰），進行中的操作如 `starting`、`stopping`、`restarting` |
| 執行時間 | 人類可讀的 uptime（如 `1h 5m 30s`） |
| 重試次數 | `abnormalExitCount` |
| 指令 | 設定的啟動指令 |

卡片操作按鈕：

- **啟動 / 停止 / 重新啟動 / 強制終止** —— 一鍵操作，結果以 toast 提示。
- **編輯** —— 開啟預填的服務表單；服務必須先停止。
- **刪除** —— 確認對話框；服務必須先停止。

### 3.2 伺服器狀態

頂部面板顯示 CPU 百分比、記憶體占用（總量/已用/閒置）、系統執行時間、平台與主機名稱，每次載入儀表板時重新整理。

### 3.3 新增服務

點擊 **Add Service** 填寫表單：

| 欄位 | 說明 |
|---|---|
| 服務 ID | 唯一名稱，作為設定檔檔名 `services/<id>.json`；只允許字母、數字、`-` 和 `_` |
| 指令 | 完整 shell 指令，如 `npm run dev` |
| 工作目錄 | 指令執行目錄；`.` 表示守護程序的工作目錄 |
| 優先權 | 自動啟動順序，`0`–`999`，數值越大越先啟動 |
| 最大重試次數 | 連續異常結束自動重啟的上限（預設 3） |
| 自動啟動 | 守護程序啟動時自動拉起 |
| 異常結束重試 | 是否在異常結束後自動重新啟動 |
| 啟用日誌 | 服務輸出寫入 `logs/<name>/` |
| 日誌保留天數 | 超過該天數的日期日誌會被刪除（預設 7） |

## 4. Web UI API（前端使用）

所有 API 路徑都在 `/webui/api/*` 下。它們要求**工作階段 cookie**，並且 `X-CSRF-Token` 請求標頭必須與 `csrf-token` cookie 相符。回應格式與守護程序 API 一致：`{ code, message, data? }`。

### 4.1 `POST /webui/api/login`

```json
{ "username": "admin", "password": "..." }
```

成功（`200`）時伺服器設定 `session-id` 和 `csrf-token` 兩個 cookie，`data` 回傳 `{ "username": "admin" }`。

- `400` —— 缺少使用者名稱/密碼
- `401` —— 憑證錯誤
- `429` —— 觸發限流

### 4.2 `POST /webui/api/logout`

使工作階段失效並清除 cookie。

### 4.3 `GET /webui/api/services`

所有服務的狀態 + 設定（無請求主體）。

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

建立服務。請求主體：`{ "id": "my-service", "command": "...", "cwd": "...", "priority": 0, "maxRetries": 3, "retryOnAbnormalExit": true, "autoStart": false, "log": { "enabled": true, "retentionDays": 7 } }`

- `400` —— id 非法（只允許字母、數字、`-`、`_`）
- `409` —— id 已存在

### 4.5 `POST /webui/api/service/{name}/edit`

更新服務。欄位與 `new` 相同，但 id 始終取自 URL。服務必須已停止且沒有進行中的操作。

### 4.6 `POST /webui/api/service/{name}/delete`

刪除服務設定檔。服務必須已停止且閒置。

### 4.7 `POST /webui/api/service/{name}/{action}`

`action` 取值為 `start` | `stop` | `restart` | `force-stop`。結果訊息在 `message` 欄位回傳。

### 4.8 `GET /webui/api/server-status`

與守護程序 API 的 `POST /os/status` 載荷相同（CPU / 記憶體 / 執行時間 / 平台 / 主機名稱）。

## 5. 介面中的錯誤處理

- 任何 API 回傳 `401` 都會把介面彈回登入頁。
- 請求進行中時，操作按鈕會被停用。
- 衝突錯誤（服務仍在執行 / 操作進行中）會以訊息形式提示（例如執行中不允許刪除/編輯）。
- 網路失敗會顯示 "network error" 提示。

## 6. 安全說明

- 工作階段 24 小時後過期，過期工作階段每小時清理一次。
- 所有狀態變更請求都必須通過 CSRF 雙重驗證（cookie **和** header）。
- 回應攜帶 `nosniff` / `DENY` 防嵌框 / 禁止快取等安全標頭。
- 靜態檔案只能從 `webui/` 目錄提供，拒絕路徑穿越。
