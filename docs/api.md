# Daemon HTTP API

Machine-to-machine control API, served on `DAEMON_PORT` (default **10300**). Every endpoint requires an **HMAC-SHA256 signature**. It is designed for dashboards, cron jobs, CI scripts or a mobile client to drive the daemon programmatically.

**Language:** English | [简体中文](api_CN.md) | [繁體中文](api_TW.md)

---

## 1. Base Information

- **Base URL:** `http://<host>:10300`
- **Method:** all endpoints are `POST`
- **Request body:** `application/json` (empty for query-only endpoints)
- **Response format (all endpoints):**

```json
{ "code": 200, "message": "OK", "data": { ... } }
```

The HTTP status code always equals `code`. `data` is omitted when there is nothing to return.

- **CORS:** responses carry `Access-Control-Allow-Origin: *`; `OPTIONS` preflight requests are short-circuited with `204`.

## 2. Authentication — HMAC Signature

### 2.1 Algorithm

```
signature = lowercase_hex( HMAC-SHA256( secret, timestamp_ms + raw_body ) )
```

- `secret` — the value of `DAEMON_SECRET_KEY` from `.env` (never ship this in client code that users can read).
- `timestamp_ms` — current unix time **in milliseconds**, sent in the `X-Timestamp` header.
- `raw_body` — the **exact raw request body bytes** as sent. For endpoints with an empty body, it is the empty string `""`.
- The signature is sent in the `X-Signature` header as a 64-character lowercase hex string.

The server rejects the request when:

- `X-Signature` or `X-Timestamp` is missing/malformed (signature must be exactly 64 hex chars), or
- the timestamp differs from server time by more than **5 minutes** (replay protection).

### 2.2 Headers

| Header | Example | Description |
|---|---|---|
| `Content-Type` | `application/json` | JSON body |
| `X-Timestamp` | `1700000000000` | Unix ms at signing time |
| `X-Signature` | `a3f2...` (64 hex) | HMAC-SHA256 as above |

### 2.3 Signing examples

**Node.js**

```js
const crypto = require('crypto');

function sign(body, secret) {
  const timestamp = Date.now().toString();
  const signature = crypto
    .createHmac('sha256', secret)
    .update(timestamp + body)   // timestamp string + raw body
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

## 3. Endpoints

### 3.1 `POST /service/list`

List all configured service names (from `services/*.json`). Empty body.

**Response `data`:**

```json
["example", "api-server", "worker"]
```

### 3.2 `POST /service/status`

Detailed runtime status for **every** configured service. Empty body.

**Response `data`** (object keyed by service name):

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

| Field | Type | Description |
|---|---|---|
| `running` | bool | process is currently up |
| `operation` | string \| null | `starting` / `stopping` / `restarting` / `force-stopping` while an operation is in flight, else `null` |
| `duration` | number \| null | uptime in seconds (`null` when not running) |
| `abnormalExitCount` | number | number of unexpected exits since the daemon started |

### 3.3 `POST /os/status`

Server resource snapshot (CPU is sampled over a 100 ms window). Empty body.

**Response `data`:**

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

Status of a single service. Empty body.

- `404` when the service is not configured.
- `data` has the same shape as a single entry in `3.2`.

### 3.5 `POST /service/control/{name}`

Start / stop / restart / force-stop a service. Body limit: 1 MB.

**Request body:**

```json
{ "type": "restart" }
```

`type` is one of:

| type | Effect |
|---|---|
| `start` | Start the service (no-op with an error if already running) |
| `stop` | Gracefully terminate the whole process tree |
| `restart` | Stop (if running) then start again |
| `force-stop` | Kill immediately, no graceful phase |

**Responses:**

- `200` — command accepted, e.g. `{"code":200,"message":"Service example stopped"}`
- `400` — invalid `type` or malformed JSON
- `404` — service not configured
- `401` — bad signature
- `500` — operation failed (e.g. `start` on an already-running service)

## 4. Error Codes

| HTTP | Meaning |
|---|---|
| 200 | success |
| 400 | invalid JSON body or invalid `type` |
| 401 | missing/malformed/expired signature |
| 404 | endpoint or service not found |
| 405 | method not allowed (only `POST` is accepted) |
| 500 | service operation failed |

## 5. End-to-End Examples (curl)

**1. List services** (empty body — sign the empty string):

```bash
SECRET="your-secret-key-here"
TS=$(date +%s%3N)
SIG=$(printf '%s' "$TS" | openssl dgst -sha256 -hmac "$SECRET" -hex | awk '{print $2}')

curl -s -X POST http://localhost:10300/service/list \
  -H "X-Timestamp: $TS" \
  -H "X-Signature: $SIG"
```

**2. Restart a service** (JSON body is part of the signed payload):

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

> **Important:** the signature input is `timestamp + body` with **no separator**. A 3-line body `{"type":"restart"}` signed as `"1700000000000{"type":"restart"}"`.
