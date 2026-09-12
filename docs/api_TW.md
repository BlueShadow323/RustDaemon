# 守護程序 HTTP API

機器對機器的控制 API，監聽 `DAEMON_PORT`（預設 **10300**）。所有端點都要求 **HMAC-SHA256 簽章**。它適合給儀表板、排程任務、CI 腳本或行動端客戶端以程式化方式驅動守護程序。

**語言切換：** [English](api.md) | [简体中文](api_CN.md) | 繁體中文

---

## 1. 基礎資訊

- **基礎 URL：** `http://<host>:10300`
- **請求方法：** 所有端點都是 `POST`
- **請求主體：** `application/json`（僅查詢端點為空主體）
- **統一回應格式（所有端點）：**

```json
{ "code": 200, "message": "OK", "data": { ... } }
```

HTTP 狀態碼始終等於 `code`。沒有回傳值時省略 `data` 欄位。

- **CORS：** 回應攜帶 `Access-Control-Allow-Origin: *`；`OPTIONS` 預檢請求直接以 `204` 短路回傳。

## 2. 認證 —— HMAC 簽章

### 2.1 演算法

```
signature = 小寫hex( HMAC-SHA256( secret, timestamp_ms + raw_body ) )
```

- `secret` —— `.env` 中 `DAEMON_SECRET_KEY` 的值（不要把金鑰寫進使用者可讀的客戶端程式碼裡）。
- `timestamp_ms` —— 簽章當下的 unix 毫秒時間戳，放在 `X-Timestamp` 請求標頭。
- `raw_body` —— **傳送的原始請求主體位元組**（一字不差）。空主體的端點簽的是空字串 `""`。
- 簽章放在 `X-Signature` 請求標頭，是 64 位小寫十六進位字串。

以下情況會被伺服器拒絕：

- `X-Signature` 或 `X-Timestamp` 缺失/格式錯誤（簽章必須是恰好 64 位 hex），或
- 時間戳與伺服器時間相差超過 **5 分鐘**（防重放）。

### 2.2 請求標頭

| 請求標頭 | 範例 | 說明 |
|---|---|---|
| `Content-Type` | `application/json` | JSON 請求主體 |
| `X-Timestamp` | `1700000000000` | 簽章當下的 unix 毫秒 |
| `X-Signature` | `a3f2...`（64 位 hex） | 上述 HMAC-SHA256 結果 |

### 2.3 簽章範例

**Node.js**

```js
const crypto = require('crypto');

function sign(body, secret) {
  const timestamp = Date.now().toString();
  const signature = crypto
    .createHmac('sha256', secret)
    .update(timestamp + body)   // 時間戳字串 + 原始請求主體
    .digest('hex');
  return { timestamp, signature };
}
```

**Python**

```python
import hashlib, hmac, time

def sign(body: str, secret: str):
    ts = str(int(time.time() * 1000))
    sig = hmac.new(secret.encode(), (ts + body).encode(), hashlib.sha256).hexdigest()
    return ts, sig
```

**Go**

```go
import ("crypto/hmac"; "crypto/sha256"; "encoding/hex"; "strconv"; "time")

func sign(body, secret string) (string, string) {
    ts := strconv.FormatInt(time.Now().UnixMilli(), 10)
    mac := hmac.New(sha256.New, []byte(secret))
    mac.Write([]byte(ts + body))
    return ts, hex.EncodeToString(mac.Sum(nil))
}
```

## 3. 端點詳解

### 3.1 `POST /service/list`

列出所有已設定的服務名稱（來自 `services/*.json`）。空請求主體。

**回應 `data`：**

```json
["example", "api-server", "worker"]
```

### 3.2 `POST /service/status`

**每個**已設定服務的詳細執行狀態。空請求主體。

**回應 `data`**（以服務名稱為鍵的物件）：

```json
{
  "example": {
    "running": true,
    "operation": null,
    "duration": 3599,
    "abnormalExitCount": 0
  }
}
```

| 欄位 | 型別 | 說明 |
|---|---|---|
| `running` | bool | 程序目前是否在執行 |
| `operation` | string \| null | 操作進行中為 `starting` / `stopping` / `restarting` / `force-stopping`，否則 `null` |
| `duration` | number \| null | 已執行秒數（未執行時為 `null`） |
| `abnormalExitCount` | number | 自守護程序啟動以來意外結束次數 |

### 3.3 `POST /os/status`

伺服器資源快照（CPU 在 100 毫秒視窗內取樣）。空請求主體。

**回應 `data`：**

```json
{
  "cpu": 12.3,
  "memory": {
    "total": 16777216000,
    "used": 6291456000,
    "free": 10485760000,
    "usage": 37.5
  },
  "uptime": 36000,
  "platform": "linux",
  "arch": "x86_64",
  "hostname": "web-01"
}
```

### 3.4 `POST /service/status/{name}`

單一服務的狀態。空請求主體。

- 服務未設定時回傳 `404`。
- `data` 結構與 3.2 中單一條目相同。

### 3.5 `POST /service/control/{name}`

啟動 / 停止 / 重新啟動 / 強制停止某個服務。請求主體上限 1 MB。

**請求主體：**

```json
{ "type": "restart" }
```

`type` 取值：

| type | 效果 |
|---|---|
| `start` | 啟動服務（已在執行時報錯） |
| `stop` | 優雅終止整棵程序樹 |
| `restart` | 先停止（若在執行）再啟動 |
| `force-stop` | 立即殺死，無優雅階段 |

**回應：**

- `200` —— 指令已接受，例如 `{"code":200,"message":"Service example stopped"}`
- `400` —— `type` 非法或 JSON 格式錯誤
- `404` —— 服務未設定
- `401` —— 簽章無效
- `500` —— 操作失敗（例如對執行中的服務執行 `start`）

## 4. 錯誤碼

| HTTP | 含義 |
|---|---|
| 200 | 成功 |
| 400 | JSON 請求主體非法或 `type` 非法 |
| 401 | 簽章缺失/格式錯誤/過期 |
| 404 | 端點或服務不存在 |
| 405 | 方法不允許（只接受 `POST`） |
| 500 | 服務操作失敗 |

## 5. 端對端範例（curl）

**1. 列出服務**（空請求主體——簽的是空字串）：

```bash
SECRET="your-secret-key-here"
TS=$(date +%s%3N)
SIG=$(printf '%s' "$TS" | openssl dgst -sha256 -hmac "$SECRET" -hex | awk '{print $2}')

curl -s -X POST http://localhost:10300/service/list \
  -H "X-Timestamp: $TS" \
  -H "X-Signature: $SIG"
```

**2. 重新啟動服務**（JSON 請求主體是簽章載荷的一部分）：

```bash
SECRET="your-secret-key-here"
BODY='{"type":"restart"}'
TS=$(date +%s%3N)
SIG=$(printf '%s%s' "$TS" "$BODY" | openssl dgst -sha256 -hmac "$SECRET" -hex | awk '{print $2}')

curl -s -X POST http://localhost:10300/service/control/example \
  -H "Content-Type: application/json" \
  -H "X-Timestamp: $TS" \
  -H "X-Signature: $SIG" \
  -d "$BODY"
```

> **重要：** 簽章輸入是 `timestamp + body`，**中間沒有分隔符號**。例如 3 行的請求主體 `{"type":"restart"}`，簽章輸入就是 `"1700000000000{"type":"restart"}"`。
