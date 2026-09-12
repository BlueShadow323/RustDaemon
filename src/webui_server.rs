//! Web UI server (port WEBUI_PORT): session-based login with rate limiting,
//! CSRF protection and static file serving for the webui/ directory.
//! Mirrors src/webui-server.js.

use crate::config::ConfigLoader;
use crate::os_status::get_server_status;
use crate::service_manager::ServiceManager;
use crate::util::{constant_time_eq, json_res, mime_for_ext, now_ms, parse_cookies};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use rand::RngCore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const SESSION_DURATION_MS: i64 = 86_400_000; // 24 h
const CLEANUP_INTERVAL_MS: u64 = 3_600_000; // 1 h
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOGIN_BLOCK_DURATION_MS: i64 = 60_000; // 1 min
const LOGIN_BODY_LIMIT: usize = 4096;

/// An authenticated Web UI session (only the expiry matters after login).
struct Session {
    expires_at: i64,
}

#[derive(Clone, Copy)]
struct LoginRecord {
    count: u32,
    first_attempt: i64,
    blocked_until: i64,
}

/// In-memory cache entry for a served static file, invalidated by mtime.
struct CachedFile {
    mtime: SystemTime,
    data: Vec<u8>,
}

#[derive(Clone)]
pub struct WebUIState {
    service_manager: Arc<ServiceManager>,
    config_loader: Arc<ConfigLoader>,
    username: String,
    password: String,
    webui_dir: PathBuf,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    csrf_tokens: Arc<Mutex<HashMap<String, String>>>,
    login_attempts: Arc<Mutex<HashMap<String, LoginRecord>>>,
    cache: Arc<Mutex<HashMap<PathBuf, CachedFile>>>,
}

/// Generate a 64-char hex token (32 random bytes), like crypto.randomBytes.
fn random_hex32() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

impl WebUIState {
    pub fn new(
        service_manager: Arc<ServiceManager>,
        config_loader: Arc<ConfigLoader>,
        username: String,
        password: String,
        webui_dir: PathBuf,
    ) -> Self {
        WebUIState {
            service_manager,
            config_loader,
            username,
            password,
            webui_dir,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            csrf_tokens: Arc::new(Mutex::new(HashMap::new())),
            login_attempts: Arc::new(Mutex::new(HashMap::new())),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn create_session(&self, _username: &str) -> (String, String) {
        let session_id = random_hex32();
        let csrf_token = random_hex32();
        let now = now_ms();
        self.sessions.lock().unwrap().insert(
            session_id.clone(),
            Session {
                expires_at: now + SESSION_DURATION_MS,
            },
        );
        self.csrf_tokens
            .lock()
            .unwrap()
            .insert(session_id.clone(), csrf_token.clone());
        (session_id, csrf_token)
    }

    fn get_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get(session_id) else {
            return false;
        };
        if now_ms() > session.expires_at {
            sessions.remove(session_id);
            self.csrf_tokens.lock().unwrap().remove(session_id);
            return false;
        }
        true
    }

    /// Extract the `session-id` cookie and check it is still valid.
    fn require_auth(&self, headers: &HeaderMap) -> Option<String> {
        let cookies = parse_cookies(headers.get(header::COOKIE).and_then(|v| v.to_str().ok()));
        let session_id = cookies.get("session-id")?.clone();
        if !self.get_session(&session_id) {
            return None;
        }
        Some(session_id)
    }

    /// The `csrf-token` cookie and the `X-CSRF-Token` header must both match
    /// the token issued with the session.
    fn verify_csrf(&self, headers: &HeaderMap, session_id: &str) -> bool {
        let cookies = parse_cookies(headers.get(header::COOKIE).and_then(|v| v.to_str().ok()));
        let cookie_token = cookies.get("csrf-token");
        let header_token = headers.get("x-csrf-token").and_then(|v| v.to_str().ok());
        let (Some(cookie_token), Some(header_token)) = (cookie_token, header_token) else {
            return false;
        };
        let stored = match self.csrf_tokens.lock().unwrap().get(session_id) {
            Some(t) => t.clone(),
            None => return false,
        };
        constant_time_eq(stored.as_bytes(), header_token.as_bytes())
            && constant_time_eq(stored.as_bytes(), cookie_token.as_bytes())
    }

    fn check_login_rate_limit(&self, ip: &str) -> bool {
        let now = now_ms();
        let mut attempts = self.login_attempts.lock().unwrap();
        let record = attempts
            .entry(ip.to_string())
            .or_insert(LoginRecord { count: 0, first_attempt: now, blocked_until: 0 });
        if now < record.blocked_until {
            return false;
        }
        if now - record.first_attempt > LOGIN_BLOCK_DURATION_MS {
            record.count = 1;
            record.first_attempt = now;
            return true;
        }
        record.count += 1;
        if record.count > MAX_LOGIN_ATTEMPTS {
            record.blocked_until = now + LOGIN_BLOCK_DURATION_MS;
            return false;
        }
        true
    }

    /// Periodically drop expired sessions and stale login-attempt records.
    pub fn start_cleanup(self: &Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(CLEANUP_INTERVAL_MS)).await;
                let now = now_ms();
                this.sessions.lock().unwrap().retain(|_, s| s.expires_at > now);
                this.login_attempts
                    .lock()
                    .unwrap()
                    .retain(|_, r| now - r.first_attempt <= LOGIN_BLOCK_DURATION_MS * 2);
            }
        });
    }
}

/// Security headers applied to every Web UI response.
async fn security_headers_middleware(request: Request, next: Next) -> Response {
    let mut res = next.run(request).await;
    let headers = res.headers_mut();
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert("X-XSS-Protection", HeaderValue::from_static("1; mode=block"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store, no-cache, must-revalidate"));
    res
}

pub fn create_router(state: WebUIState) -> Router {
    Router::new()
        .route("/webui/api/login", post(login).layer(DefaultBodyLimit::max(LOGIN_BODY_LIMIT)))
        .route("/webui/api/logout", post(logout))
        .route("/webui/api/services", get(services_list))
        .route("/webui/api/service/new", post(service_new))
        .route("/webui/api/service/{name}/edit", post(service_edit))
        .route("/webui/api/service/{name}/delete", post(service_delete))
        .route("/webui/api/service/{name}/{action}", post(service_action))
        .route("/webui/api/server-status", get(server_status))
        .fallback(webui_fallback)
        .layer(middleware::from_fn(security_headers_middleware))
        .with_state(state)
}

/// Fallback handler mirroring the original routing: redirect "/", serve the
/// static files under /webui/, JSON 404 for anything else.
async fn webui_fallback(State(state): State<WebUIState>, request: Request) -> Response {
    let pathname = request.uri().path().to_string();
    if pathname == "/" {
        // 302 to the Web UI
        let mut res = Response::new(axum::body::Body::empty());
        *res.status_mut() = StatusCode::FOUND;
        res.headers_mut()
            .insert(header::LOCATION, HeaderValue::from_static("/webui/"));
        return res;
    }
    if pathname == "/webui" || pathname == "/webui/" || pathname == "/webui/index.html" {
        return serve_file(&state, "index.html");
    }
    if let Some(rest) = pathname.strip_prefix("/webui/") {
        // Unknown API paths must not be served as static files.
        if rest == "api" || rest.starts_with("api/") {
            return json_res(404, "Not found", None);
        }
        return serve_file(&state, rest);
    }
    json_res(404, "Not found", None)
}

fn client_ip(headers: &HeaderMap, addr: SocketAddr) -> String {
    if let Some(fwd) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = fwd.split(',').next() {
            return first.trim().to_string();
        }
    }
    addr.ip().to_string()
}

/// POST /webui/api/login
async fn login(
    State(state): State<WebUIState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_res(400, "Invalid JSON", None),
    };
    let username = parsed.get("username").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let password = parsed.get("password").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if username.is_empty() || password.is_empty() {
        return json_res(400, "Username and password required", None);
    }
    if username.len() > 64 || password.len() > 128 {
        return json_res(400, "Invalid credentials", None);
    }
    let ip = client_ip(&headers, addr);
    if !state.check_login_rate_limit(&ip) {
        return json_res(429, "Too many login attempts, please wait", None);
    }
    if username != state.username || password != state.password {
        return json_res(401, "Invalid username or password", None);
    }
    let (session_id, csrf_token) = state.create_session(&username);
    let max_age = SESSION_DURATION_MS / 1000;
    let cookie1 = format!("session-id={session_id}; Max-Age={max_age}; Path=/webui; HttpOnly; SameSite=Lax");
    let cookie2 = format!("csrf-token={csrf_token}; Max-Age={max_age}; Path=/webui; SameSite=Lax");
    let mut res = json_res(200, "OK", Some(json!({ "username": username })));
    res.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&cookie1).unwrap());
    res.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&cookie2).unwrap());
    res
}

/// POST /webui/api/logout
async fn logout(State(state): State<WebUIState>, headers: HeaderMap) -> Response {
    let Some(session_id) = state.require_auth(&headers) else {
        return json_res(401, "Not authenticated", None);
    };
    if !state.verify_csrf(&headers, &session_id) {
        return json_res(403, "Invalid CSRF token", None);
    }
    state.sessions.lock().unwrap().remove(&session_id);
    state.csrf_tokens.lock().unwrap().remove(&session_id);
    let mut res = json_res(200, "OK", None);
    res.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("session-id=; Max-Age=0; Path=/webui; HttpOnly; SameSite=Lax"),
    );
    res.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("csrf-token=; Max-Age=0; Path=/webui; SameSite=Lax"),
    );
    res
}

/// GET /webui/api/services — per-service status merged with its config.
async fn services_list(State(state): State<WebUIState>, headers: HeaderMap) -> Response {
    let Some(session_id) = state.require_auth(&headers) else {
        return json_res(401, "Not authenticated", None);
    };
    if !state.verify_csrf(&headers, &session_id) {
        return json_res(403, "Invalid CSRF token", None);
    }
    let mut statuses = serde_json::Map::new();
    for name in state.config_loader.list_services() {
        let daemon_status = state.service_manager.get_detailed_status(&name);
        let config = state.config_loader.load_service_config(&name);
        let entry = json!({
            "running": daemon_status.running,
            "operation": daemon_status.operation,
            "duration": daemon_status.duration,
            "abnormalExitCount": daemon_status.abnormal_exit_count,
            "command": config.as_ref().map(|c| c.command.as_str()).unwrap_or(""),
            "cwd": config.as_ref().map(|c| c.cwd.as_str()).unwrap_or(""),
            "autoStart": config.as_ref().map(|c| c.auto_start).unwrap_or(false),
            "priority": config.as_ref().map(|c| c.priority).unwrap_or(0),
            "maxRetries": config.as_ref().map(|c| c.max_retries).unwrap_or(3),
            "retryOnAbnormalExit": config.as_ref().map(|c| c.retry_on_abnormal_exit).unwrap_or(true),
            "log": {
                "enabled": config.as_ref().map(|c| c.log.enabled).unwrap_or(false),
                "retentionDays": config.as_ref().map(|c| c.log.retention_days).unwrap_or(7)
            }
        });
        statuses.insert(name, entry);
    }
    json_res(200, "OK", Some(Value::Object(statuses)))
}

/// POST /webui/api/service/{name}/{action}
async fn service_action(
    State(state): State<WebUIState>,
    Path((name, action)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let Some(session_id) = state.require_auth(&headers) else {
        return json_res(401, "Not authenticated", None);
    };
    if !state.verify_csrf(&headers, &session_id) {
        return json_res(403, "Invalid CSRF token", None);
    }
    let Some(config) = state.config_loader.load_service_config(&name) else {
        return json_res(404, &format!("Service {name} not found"), None);
    };
    let result = match action.as_str() {
        "start" => state.service_manager.start(&name, &config, false),
        "stop" => state.service_manager.stop(&name),
        "restart" => state.service_manager.restart(&name, &config),
        "force-stop" => state.service_manager.force_stop(&name),
        _ => return json_res(404, "Not found", None),
    };
    json_res(if result.success { 200 } else { 500 }, &result.message, None)
}

/// POST /webui/api/service/new
async fn service_new(
    State(state): State<WebUIState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(session_id) = state.require_auth(&headers) else {
        return json_res(401, "Not authenticated", None);
    };
    if !state.verify_csrf(&headers, &session_id) {
        return json_res(403, "Invalid CSRF token", None);
    }
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_res(400, "Invalid JSON body", None),
    };
    let id = parsed.get("id").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let valid_id = !id.is_empty()
        && id.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !valid_id {
        return json_res(400, "Invalid service ID: only letters, numbers, hyphens and underscores allowed", None);
    }
    if state.config_loader.load_service_config(&id).is_some() {
        return json_res(409, &format!("Service '{id}' already exists"), None);
    }
    if let Err(e) = state.config_loader.save_service_config(&id, &parsed) {
        return json_res(500, &format!("Failed to save service: {e}"), None);
    }
    json_res(200, &format!("Service '{id}' created"), None)
}

/// POST /webui/api/service/{name}/edit
async fn service_edit(
    State(state): State<WebUIState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(session_id) = state.require_auth(&headers) else {
        return json_res(401, "Not authenticated", None);
    };
    if !state.verify_csrf(&headers, &session_id) {
        return json_res(403, "Invalid CSRF token", None);
    }
    if state.config_loader.load_service_config(&name).is_none() {
        return json_res(404, &format!("Service '{name}' not found"), None);
    }
    let edit_status = state.service_manager.get_detailed_status(&name);
    if edit_status.running {
        return json_res(409, &format!("Service '{name}' is still running. Stop it first before editing."), None);
    }
    if edit_status.operation.is_some() {
        return json_res(409, &format!("Service '{name}' has an operation in progress."), None);
    }
    let mut parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_res(400, "Invalid JSON body", None),
    };
    // The service id is taken from the URL, never from the body.
    if let Some(obj) = parsed.as_object_mut() {
        obj.remove("id");
    }
    if let Err(e) = state.config_loader.save_service_config(&name, &parsed) {
        return json_res(500, &format!("Failed to save service: {e}"), None);
    }
    json_res(200, &format!("Service '{name}' updated"), None)
}

/// POST /webui/api/service/{name}/delete
async fn service_delete(
    State(state): State<WebUIState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session_id) = state.require_auth(&headers) else {
        return json_res(401, "Not authenticated", None);
    };
    if !state.verify_csrf(&headers, &session_id) {
        return json_res(403, "Invalid CSRF token", None);
    }
    if state.config_loader.load_service_config(&name).is_none() {
        return json_res(404, &format!("Service '{name}' not found"), None);
    }
    let status = state.service_manager.get_detailed_status(&name);
    if status.running {
        return json_res(409, &format!("Service '{name}' is still running. Stop it first before deleting."), None);
    }
    if status.operation.is_some() {
        return json_res(409, &format!("Service '{name}' has an operation in progress. Wait for it to complete."), None);
    }
    state.config_loader.delete_service_config(&name);
    json_res(200, &format!("Service '{name}' deleted"), None)
}

/// GET /webui/api/server-status — the original proxied this to the daemon
/// HTTP API; here the status is read directly in-process.
async fn server_status(State(state): State<WebUIState>, headers: HeaderMap) -> Response {
    let Some(session_id) = state.require_auth(&headers) else {
        return json_res(401, "Not authenticated", None);
    };
    if !state.verify_csrf(&headers, &session_id) {
        return json_res(403, "Invalid CSRF token", None);
    }
    let status = get_server_status().await;
    json_res(200, "OK", Some(status))
}

/// Serve a file from webui_dir, rejecting path traversal. Text files are
/// cached in memory and invalidated when the file's mtime changes.
fn serve_file(state: &WebUIState, rel: &str) -> Response {
    let rel_path = FsPath::new(rel);
    if !rel_path.components().all(|c| matches!(c, Component::Normal(_))) {
        return json_res(403, "Forbidden", None);
    }
    let full = state.webui_dir.join(rel_path);
    let ext = full.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mime = mime_for_ext(&ext);

    let meta = match std::fs::metadata(&full) {
        Ok(m) if m.is_file() => m,
        _ => return json_res(404, "Not found", None),
    };
    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    let mut cache = state.cache.lock().unwrap();
    let data = match cache.get(&full) {
        Some(entry) if entry.mtime == mtime => entry.data.clone(),
        _ => {
            let data = match std::fs::read(&full) {
                Ok(d) => d,
                Err(_) => return json_res(404, "Not found", None),
            };
            cache.insert(full.clone(), CachedFile { mtime, data: data.clone() });
            data
        }
    };
    drop(cache);

    let mut res = Response::new(axum::body::Body::from(data));
    *res.status_mut() = StatusCode::OK;
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_str(mime).unwrap());
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    res
}
