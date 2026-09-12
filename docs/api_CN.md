# 守护进程 HTTP API

机器对机器的控制 API，监听 `DAEMON_PORT`（默认 **10300**）。所有端点都要求 **HMAC-SHA256 签名**。它适合给看板、定时任务、CI 脚本或移动端客户端以编程方式驱动守护进程。

**语言切换：** [English](api.md) | 简体中文 | [繁體中文](api_TW.md)

---

## 1. 基础信息

- **基础 URL：** `http://<host>:10300`
- **请求方法：** 所有端点都是 `POST`
- **请求体：** `application/json`（仅查询端点为空体）
- **统一响应格式（所有端点）：**

```json
{ "code": 200, "message": "OK", "data": { ... } }
```

HTTP 状态码始终等于 `code`。没有返回值时省略 `data` 字段。

- **CORS：** 响应携带 `Access-Control-Allow-Origin: *`；`OPTIONS` 预检请求直接以 `204` 短路返回。

## 2. 认证 —— HMAC 签名

### 2.1 算法

```
signature = 小写hex( HMAC-SHA256( secret, timestamp_ms + raw_body ) )
```

- `secret` —— `.env` 中 `DAEMON_SECRET_KEY` 的值（不要把密钥写进用户可见的客户端代码里）。
- `timestamp_ms` —— 签名时刻的 unix 毫秒时间戳，放在 `X-Timestamp` 请求头。
- `raw_body` —— **发送的原始请求体字节**（一字不差）。空体的端点签的是空字符串 `""`。
- 签名放在 `X-Signature` 请求头，是 64 位小写十六进制字符串。

以下情况会被服务器拒绝：

- `X-Signature` 或 `X-Timestamp` 缺失/格式错误（签名必须是恰好 64 位 hex），或
- 时间戳与服务器时间相差超过 **5 分钟**（防重放）。

### 2.2 请求头

| 请求头 | 示例 | 说明 |
|---|---|---|
| `Content-Type` | `application/json` | JSON 请求体 |
| `X-Timestamp` | `1700000000000` | 签名时刻的 unix 毫秒 |
| `X-Signature` | `a3f2...`（64 位 hex） | 上述 HMAC-SHA256 结果 |

### 2.3 签名示例

**Node.js**

```js
const crypto = require('crypto');

function sign(body, secret) {
  const timestamp = Date.now().toString();
  const signature = crypto
    .createHmac('sha256', secret)
    .update(timestamp + body)   // 时间戳字符串 + 原始请求体
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

## 3. 端点详解

### 3.1 `POST /service/list`

列出所有已配置的服务名（来自 `services/*.json`）。空请求体。

**响应 `data`：**

```json
["example", "api-server", "worker"]
```

### 3.2 `POST /service/status`

**每个**已配置服务的详细运行状态。空请求体。

**响应 `data`**（以服务名为键的对象）：

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

| 字段 | 类型 | 说明 |
|---|---|---|
| `running` | bool | 进程当前是否在运行 |
| `operation` | string \| null | 操作进行中为 `starting` / `stopping` / `restarting` / `force-stopping`，否则 `null` |
| `duration` | number \| null | 已运行秒数（未运行时为 `null`） |
| `abnormalExitCount` | number | 自守护进程启动以来意外退出次数 |

### 3.3 `POST /os/status`

服务器资源快照（CPU 在 100 毫秒窗口内采样）。空请求体。

**响应 `data`：**

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

单个服务的状态。空请求体。

- 服务未配置时返回 `404`。
- `data` 结构与 3.2 中单个条目相同。

### 3.5 `POST /service/control/{name}`

启动 / 停止 / 重启 / 强制停止某个服务。请求体上限 1 MB。

**请求体：**

```json
{ "type": "restart" }
```

`type` 取值：

| type | 效果 |
|---|---|
| `start` | 启动服务（已在运行时报错） |
| `stop` | 优雅终止整棵进程树 |
| `restart` | 先停止（若在运行）再启动 |
| `force-stop` | 立即杀死，无优雅阶段 |

**响应：**

- `200` —— 命令已接受，例如 `{"code":200,"message":"Service example stopped"}`
- `400` —— `type` 非法或 JSON 格式错误
- `404` —— 服务未配置
- `401` —— 签名无效
- `500` —— 操作失败（例如对运行中的服务执行 `start`）

## 4. 错误码

| HTTP | 含义 |
|---|---|
| 200 | 成功 |
| 400 | JSON 请求体非法或 `type` 非法 |
| 401 | 签名缺失/格式错误/过期 |
| 404 | 端点或服务不存在 |
| 405 | 方法不允许（只接受 `POST`） |
| 500 | 服务操作失败 |

## 5. 端到端示例（curl）

**1. 列出服务**（空请求体——签的是空字符串）：

```bash
SECRET="your-secret-key-here"
TS=$(date +%s%3N)
SIG=$(printf '%s' "$TS" | openssl dgst -sha256 -hmac "$SECRET" -hex | awk '{print $2}')

curl -s -X POST http://localhost:10300/service/list \
  -H "X-Timestamp: $TS" \
  -H "X-Signature: $SIG"
```

**2. 重启服务**（JSON 请求体是签名载荷的一部分）：

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

> **重要：** 签名输入是 `timestamp + body`，**中间没有分隔符**。例如 3 行的请求体 `{"type":"restart"}`，签名输入就是 `"1700000000000{"type":"restart"}"`。
