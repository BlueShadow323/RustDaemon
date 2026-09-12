//! Daemon HTTP API (port DAEMON_PORT). All endpoints require an
//! HMAC-SHA256 signature over `X-Timestamp + body` using the shared secret.
//! Mirrors src/server.js.

use crate::config::ConfigLoader;
use crate::os_status::get_server_status;
use crate::service_manager::ServiceManager;
use crate::util::{constant_time_eq, is_hex64, json_res, now_ms};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

/// CORS middleware: short-circuit OPTIONS preflight requests with 204 and add
/// CORS headers to every response, like the original setCorsHeaders.
async fn cors_middleware(request: Request, next: Next) -> Response {
    let mut res = if request.method() == Method::OPTIONS {
        let mut res = Response::new(axum::body::Body::empty());
        *res.status_mut() = StatusCode::NO_CONTENT;
        res
    } else {
        next.run(request).await
    };
    let headers = res.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, "POST, OPTIONS".parse().unwrap());
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Content-Type, X-Signature, X-Timestamp".parse().unwrap(),
    );
    res
}

/// Verify the HMAC-SHA256 signature over `timestamp + body`. Rejects missing
/// or malformed headers and timestamps older than five minutes.
fn verify_signature(body: &str, headers: &HeaderMap, secret: &str) -> bool {
    let signature = match headers.get("x-signature").and_then(|v| v.to_str().ok()) {
        Some(s) if is_hex64(s) => s,
        _ => return false,
    };
    let timestamp = match headers.get("x-timestamp").and_then(|v| v.to_str().ok()) {
        Some(t) => t,
        _ => return false,
    };
    let ts: i64 = match timestamp.parse() {
        Ok(ts) => ts,
        Err(_) => return false,
    };
    if (now_ms() - ts).abs() > 300_000 {
        return false;
    }
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(timestamp.as_bytes());
    mac.update(body.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    constant_time_eq(expected.as_bytes(), signature.as_bytes())
}

#[derive(Clone)]
pub struct DaemonState {
    service_manager: Arc<ServiceManager>,
    config_loader: Arc<ConfigLoader>,
    secret: String,
}

pub fn create_router(
    service_manager: Arc<ServiceManager>,
    config_loader: Arc<ConfigLoader>,
    secret: String,
) -> Router {
    let state = DaemonState {
        service_manager,
        config_loader,
        secret,
    };
    Router::new()
        .route("/service/list", post(list_services))
        .route("/service/status", post(status_all))
        .route("/os/status", post(os_status))
        .route("/service/status/{name}", post(service_status))
        .route(
            "/service/control/{name}",
            post(service_control).layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .method_not_allowed_fallback(|| async { json_res(405, "Method not allowed", None) })
        .fallback(|| async { json_res(404, "Not found", None) })
        .layer(middleware::from_fn(cors_middleware))
        .with_state(state)
}

/// POST /service/list — signed over an empty body.
async fn list_services(State(state): State<DaemonState>, headers: HeaderMap) -> Response {
    if !verify_signature("", &headers, &state.secret) {
        return json_res(401, "Invalid signature", None);
    }
    let names = state.config_loader.list_services();
    json_res(200, "OK", Some(json!(names)))
}

/// POST /service/status — detailed status for every configured service.
async fn status_all(State(state): State<DaemonState>, headers: HeaderMap) -> Response {
    if !verify_signature("", &headers, &state.secret) {
        return json_res(401, "Invalid signature", None);
    }
    let mut all = serde_json::Map::new();
    for name in state.config_loader.list_services() {
        all.insert(
            name.clone(),
            serde_json::to_value(state.service_manager.get_detailed_status(&name)).unwrap(),
        );
    }
    json_res(200, "OK", Some(Value::Object(all)))
}

/// POST /os/status — server CPU/memory/uptime snapshot.
async fn os_status(State(state): State<DaemonState>, headers: HeaderMap) -> Response {
    if !verify_signature("", &headers, &state.secret) {
        return json_res(401, "Invalid signature", None);
    }
    let status = get_server_status().await;
    json_res(200, "OK", Some(status))
}

/// POST /service/status/{name}
async fn service_status(
    State(state): State<DaemonState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !verify_signature("", &headers, &state.secret) {
        return json_res(401, "Invalid signature", None);
    }
    if state.config_loader.load_service_config(&name).is_none() {
        return json_res(404, &format!("Service {name} not found"), None);
    }
    let status = state.service_manager.get_detailed_status(&name);
    json_res(200, "OK", Some(serde_json::to_value(status).unwrap()))
}

/// POST /service/control/{name} — start/stop/restart/force-stop. The raw body
/// is part of the signed payload, so it is read before parsing.
async fn service_control(
    State(state): State<DaemonState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let raw_body = String::from_utf8_lossy(&body).to_string();
    if !verify_signature(&raw_body, &headers, &state.secret) {
        return json_res(401, "Invalid signature", None);
    }
    let parsed: Value = match serde_json::from_str(&raw_body) {
        Ok(v) => v,
        Err(_) => return json_res(400, "Invalid JSON body", None),
    };
    let Some(config) = state.config_loader.load_service_config(&name) else {
        return json_res(404, &format!("Service {name} not found"), None);
    };
    let result = match parsed.get("type").and_then(Value::as_str) {
        Some("start") => state.service_manager.start(&name, &config, false),
        Some("stop") => state.service_manager.stop(&name),
        Some("restart") => state.service_manager.restart(&name, &config),
        Some("force-stop") => state.service_manager.force_stop(&name),
        _ => {
            return json_res(400, "Invalid type, must be start/stop/restart/force-stop", None);
        }
    };
    json_res(if result.success { 200 } else { 500 }, &result.message, None)
}
