//! Small shared helpers used by the daemon API server and the Web UI server.
//! They mirror the inline utilities in the original Node.js implementation.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Build a JSON response shaped like the original: `{ code, message, data? }`.
/// The HTTP status code equals `code`.
pub fn json_res(code: u16, message: &str, data: Option<Value>) -> Response {
    let mut body = json!({ "code": code, "message": message });
    if let Some(data) = data {
        body["data"] = data;
    }
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(body)).into_response()
}

/// Constant-time byte comparison, used for HMAC signatures and CSRF tokens.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Current unix timestamp in milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Map a file extension (including the leading dot) to its MIME type,
/// mirroring the original getMimeType function.
pub fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        ".html" => "text/html; charset=utf-8",
        ".css" => "text/css; charset=utf-8",
        ".js" => "application/javascript; charset=utf-8",
        ".json" => "application/json",
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".svg" => "image/svg+xml",
        ".ico" => "image/x-icon",
        ".woff" => "font/woff",
        ".woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Minimal cookie parser: `"a=1; b=2"` -> `{ "a": "1", "b": "2" }`.
pub fn parse_cookies(header: Option<&str>) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    if let Some(header) = header {
        for part in header.split(';') {
            if let Some((key, value)) = part.trim().split_once('=') {
                cookies.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    cookies
}

/// True when `s` is a 64-character hexadecimal string (an HMAC signature or
/// a hex-encoded session/CSRF token).
pub fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
