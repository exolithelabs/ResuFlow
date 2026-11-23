use crate::mcp::{handle_mcp_payload, mcp_snippet};
use crate::memory::{load_memory, save_memory};
use crate::releases::{check_latest_release, CURRENT_VERSION};
use crate::render::render_resume;
use crate::skills_store::{
    create_skill, delete_skill, list_skills, load_skill, save_skill, set_skill_enabled,
};
use crate::user_config::{load_user_config, remember_vault};
use crate::workspace::{
    assert_slug, create_resume, delete_resume, find_workspace, init_workspace, list_resumes,
    load_profile, load_resume, save_profile, save_resume, WorkspaceError, SCHEMA_VERSION,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const MAX_JSON_BODY_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

const APP_CSP: &str = "default-src 'self'; base-uri 'none'; connect-src 'self'; font-src 'self'; form-action 'self'; frame-ancestors 'self'; frame-src 'self'; img-src 'self' data:; object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'";
const PREVIEW_CSP: &str = "default-src 'none'; base-uri 'none'; font-src 'self'; form-action 'none'; frame-ancestors 'self'; img-src 'self' data:; object-src 'none'; script-src 'none'; style-src 'unsafe-inline'";

const EMBEDDED_INDEX_HTML: &str = include_str!("../../dist/index.html");
const EMBEDDED_APP_JS: &str = include_str!("../../dist/app.js");
const EMBEDDED_STYLES_CSS: &str = include_str!("../../dist/index.css");

#[derive(Clone)]
struct ServerState {
    workspace_root: Arc<Mutex<PathBuf>>,
    port: u16,
    api_token: String,
    browser_session_id: String,
    launch_token: String,
}

pub struct StartedServer {
    pub port: u16,
    pub url: String,
    pub launch_url: String,
    pub mcp_url: String,
    pub api_token: String,
    pub launch_token: String,
    shutdown: Arc<AtomicBool>,
}

impl StartedServer {
    pub fn close(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

pub struct StartServerConfig {
    pub workspace_root: PathBuf,
    pub port: u16,
    pub api_token: Option<String>,
    pub launch_token: Option<String>,
}

pub fn start_server(config: StartServerConfig) -> Result<StartedServer, WorkspaceError> {
    let configured_token = config
        .api_token
        .or_else(|| std::env::var("RESUFLOW_API_TOKEN").ok())
        .unwrap_or_default();
    if !configured_token.is_empty() && !is_valid_token(&configured_token) {
        return Err(WorkspaceError::bad_request(
            "RESUFLOW_API_TOKEN must be a 32-128 character URL-safe token.",
        ));
    }

    let api_token = if configured_token.is_empty() {
        format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
    } else {
        configured_token
    };
    let browser_session_id = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let launch_token = config
        .launch_token
        .or_else(|| std::env::var("RESUFLOW_LAUNCH_TOKEN").ok())
        .unwrap_or_default();

    let listener = TcpListener::bind(("127.0.0.1", config.port)).map_err(|e| {
        WorkspaceError::new(
            format!("Failed to bind 127.0.0.1:{} ({e})", config.port),
            500,
        )
    })?;
    let actual_port = listener
        .local_addr()
        .map_err(|e| WorkspaceError::new(e.to_string(), 500))?
        .port();

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    let state = ServerState {
        workspace_root: Arc::new(Mutex::new(config.workspace_root)),
        port: actual_port,
        api_token: api_token.clone(),
        browser_session_id,
        launch_token: launch_token.clone(),
    };

    thread::spawn(move || {
        for stream_res in listener.incoming() {
            if shutdown_clone.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream_res else { continue };
            let st = state.clone();
            thread::spawn(move || {
                handle_connection(stream, &st);
            });
        }
    });

    Ok(StartedServer {
        port: actual_port,
        url: format!("http://127.0.0.1:{actual_port}/"),
        launch_url: format!("http://127.0.0.1:{actual_port}/?token={api_token}"),
        mcp_url: format!("http://127.0.0.1:{actual_port}/mcp"),
        api_token,
        launch_token,
        shutdown,
    })
}

fn is_valid_token(token: &str) -> bool {
    let len = token.len();
    (32..=128).contains(&len)
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

struct HttpRequest {
    method: String,
    raw_target: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn handle_connection(mut stream: TcpStream, state: &ServerState) {
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));

    let resp = match read_http_request(&mut stream) {
        Ok(Some(req)) => match route_request(&req, state) {
            Ok(r) => r,
            Err(err) => error_response(&err),
        },
        Ok(None) => return,
        Err(err) => error_response(&err),
    };

    let _ = write_http_response(&mut stream, &resp);
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
    let mut drain = [0u8; 8192];
    let mut drained = 0usize;
    while drained < 2 * 1024 * 1024 {
        match stream.read(&mut drain) {
            Ok(0) | Err(_) => break,
            Ok(n) => drained += n,
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>, WorkspaceError> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_double_crlf(&buf) {
            break pos;
        }
        if buf.len() > 64 * 1024 {
            return Err(WorkspaceError::bad_request("Request headers too large."));
        }
        let n = stream
            .read(&mut chunk)
            .map_err(|_| WorkspaceError::bad_request("Failed to read request."))?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            return Err(WorkspaceError::bad_request("Incomplete HTTP request."));
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let header_bytes = &buf[..header_end];
    let header_str = std::str::from_utf8(header_bytes)
        .map_err(|_| WorkspaceError::bad_request("Invalid UTF-8 in headers."))?;
    let mut lines = header_str.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| WorkspaceError::bad_request("Empty request line."))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| WorkspaceError::bad_request("Missing HTTP method."))?
        .to_uppercase();
    let raw_target = parts
        .next()
        .ok_or_else(|| WorkspaceError::bad_request("Missing request target."))?
        .to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_JSON_BODY_BYTES {
        return Err(WorkspaceError::new("Request body is too large.", 413));
    }

    let body_start = header_end + 4;
    let mut body = Vec::with_capacity(content_length);
    if buf.len() > body_start {
        body.extend_from_slice(&buf[body_start..]);
    }
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let to_read = remaining.min(chunk.len());
        let n = stream
            .read(&mut chunk[..to_read])
            .map_err(|_| WorkspaceError::bad_request("Failed to read request body."))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > MAX_JSON_BODY_BYTES {
            return Err(WorkspaceError::new("Request body is too large.", 413));
        }
    }

    Ok(Some(HttpRequest {
        method,
        raw_target,
        headers,
        body,
    }))
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn route_request(req: &HttpRequest, state: &ServerState) -> Result<HttpResponse, WorkspaceError> {
    let (raw_path, raw_query) = match req.raw_target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (req.raw_target.as_str(), ""),
    };
    let pathname = percent_decode(raw_path)?;
    let query = parse_query(raw_query);

    assert_trusted_host(req, state.port)?;
    let protected_path = pathname.starts_with("/api/")
        || pathname == "/mcp"
        || pathname.starts_with("/preview/");
    if protected_path {
        assert_trusted_origin(req, state.port)?;
    }

    if pathname == "/api/health" && req.method == "GET" {
        return json_response(
            200,
            &json!({
                "ok": true,
                "app": "resuflow",
                "launchToken": state.launch_token,
            }),
        );
    }

    if protected_path && req.method != "OPTIONS" {
        let browser_path = pathname.starts_with("/api/") || pathname.starts_with("/preview/");
        authorize_request(
            req,
            &state.api_token,
            browser_path,
            &state.browser_session_id,
            browser_path && req.method != "GET" && req.method != "HEAD",
        )?;
    }

    if pathname == "/api/releases/latest" && req.method == "GET" {
        let use_cache = query.get("refresh").map(|v| v.as_str()) != Some("1");
        match check_latest_release(use_cache) {
            Ok(val) => return json_response(200, &val),
            Err(_) => {
                return json_response(
                    200,
                    &json!({
                        "checked": false,
                        "available": false,
                        "currentVersion": CURRENT_VERSION,
                    }),
                )
            }
        }
    }

    if pathname == "/api/open-browser" && req.method == "POST" {
        let body = read_json(req)?;
        let hash = body
            .get("hash")
            .and_then(|v| v.as_str())
            .filter(|h| is_safe_hash(h))
            .unwrap_or("");
        let url = format!(
            "http://127.0.0.1:{}/?token={}{}",
            state.port, state.api_token, hash
        );
        open_browser(&url);
        return json_response(200, &json!({ "ok": true }));
    }

    let current_workspace = state
        .workspace_root
        .lock()
        .map_err(|_| WorkspaceError::new("Internal state lock poisoned.", 500))?
        .clone();

    if pathname == "/api/workspace" && req.method == "GET" {
        let config = load_user_config();
        let root_str = current_workspace.to_string_lossy().to_string();
        let name = current_workspace
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("ResuFlow");
        let default_vault = if config.default_vault.is_empty() {
            root_str.clone()
        } else {
            config.default_vault
        };
        return json_response(
            200,
            &json!({
                "root": root_str,
                "name": name,
                "schemaVersion": SCHEMA_VERSION,
                "defaultVault": default_vault,
                "recentVaults": config.recent_vaults,
            }),
        );
    }

    if pathname == "/api/workspace" && req.method == "PUT" {
        let body = read_json(req)?;
        let target = body
            .get("root")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if target.is_empty() {
            return Err(WorkspaceError::bad_request("A vault path is required."));
        }
        let init_flag = body.get("init").and_then(|v| v.as_bool()).unwrap_or(false);
        let set_default = body
            .get("setDefault")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let target_path = PathBuf::from(target);
        let workspace = if init_flag {
            init_workspace(&target_path)?
        } else {
            find_workspace(&target_path)?
        };
        if let Ok(mut guard) = state.workspace_root.lock() {
            *guard = workspace.root.clone();
        }
        let config = remember_vault(&workspace.root, set_default)?;
        let root_str = workspace.root.to_string_lossy().to_string();
        let name = workspace
            .root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("ResuFlow");
        return json_response(
            200,
            &json!({
                "root": root_str,
                "name": name,
                "schemaVersion": SCHEMA_VERSION,
                "defaultVault": config.default_vault,
                "recentVaults": config.recent_vaults,
            }),
        );
    }

    if pathname == "/api/profile" && req.method == "GET" {
        let profile = load_profile(&current_workspace)?;
        return json_response(200, &serde_json::to_value(profile).unwrap_or_default());
    }

    if pathname == "/api/profile" && req.method == "PUT" {
        let body = read_json(req)?;
        let profile = save_profile(&current_workspace, &body)?;
        return json_response(200, &serde_json::to_value(profile).unwrap_or_default());
    }

    if pathname == "/api/mcp" && req.method == "GET" {
        let mcp_url = format!("http://127.0.0.1:{}/mcp", state.port);
        return json_response(200, &mcp_snippet(&mcp_url, &state.api_token));
    }

    if pathname == "/mcp" && req.method == "OPTIONS" {
        let mut headers = security_headers();
        if let Some(origin) = req.headers.get("origin") {
            headers.push(("Access-Control-Allow-Origin".to_string(), origin.clone()));
            headers.push(("Vary".to_string(), "Origin".to_string()));
        }
        headers.push((
            "Access-Control-Allow-Headers".to_string(),
            "authorization, content-type, mcp-session-id, mcp-protocol-version, x-resuflow-token"
                .to_string(),
        ));
        headers.push((
            "Access-Control-Allow-Methods".to_string(),
            "GET, POST, DELETE, OPTIONS".to_string(),
        ));
        return Ok(HttpResponse {
            status: 204,
            headers,
            body: Vec::new(),
        });
    }

    if pathname == "/mcp" {
        if req.method != "POST" {
            return Err(WorkspaceError::new("Method not allowed", 405));
        }
        let body = read_json(req)?;
        match handle_mcp_payload(&body, &current_workspace)? {
            Some(resp) => return json_response(200, &resp),
            None => {
                return Ok(HttpResponse {
                    status: 202,
                    headers: security_headers(),
                    body: Vec::new(),
                })
            }
        }
    }

    if pathname == "/api/memory" && req.method == "GET" {
        let memory = load_memory(&current_workspace)?;
        return json_response(200, &serde_json::to_value(memory).unwrap_or_default());
    }

    if pathname == "/api/memory" && req.method == "PUT" {
        let body = read_json(req)?;
        let memory = save_memory(&current_workspace, &body)?;
        return json_response(200, &serde_json::to_value(memory).unwrap_or_default());
    }

    if pathname == "/api/skills" && req.method == "GET" {
        let skills = list_skills(&current_workspace, true)?;
        return json_response(200, &json!({ "skills": skills }));
    }

    if pathname == "/api/skills" && req.method == "POST" {
        let body = read_json(req)?;
        let created = create_skill(&current_workspace, &body)?;
        return json_response(201, &serde_json::to_value(created).unwrap_or_default());
    }

    if pathname == "/api/skill/enabled" && req.method == "PUT" {
        let slug = request_slug(req, "skill")?;
        let body = read_json(req)?;
        let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
        let updated = set_skill_enabled(&current_workspace, &slug, enabled)?;
        return json_response(200, &serde_json::to_value(updated).unwrap_or_default());
    }

    if pathname == "/api/skill" {
        let slug = request_slug(req, "skill")?;
        if req.method == "GET" {
            let skill = load_skill(&current_workspace, &slug)?;
            return json_response(200, &serde_json::to_value(skill).unwrap_or_default());
        }
        if req.method == "PUT" {
            let body = read_json(req)?;
            let skill = save_skill(&current_workspace, &slug, &body)?;
            return json_response(200, &serde_json::to_value(skill).unwrap_or_default());
        }
        if req.method == "DELETE" {
            delete_skill(&current_workspace, &slug)?;
            return Ok(HttpResponse {
                status: 204,
                headers: security_headers(),
                body: Vec::new(),
            });
        }
    }

    if pathname == "/api/resumes" && req.method == "GET" {
        let resumes = list_resumes(&current_workspace)?;
        return json_response(200, &json!({ "resumes": resumes }));
    }

    if pathname == "/api/resumes" && req.method == "POST" {
        let body = read_json(req)?;
        let created = create_resume(&current_workspace, &body)?;
        return json_response(201, &serde_json::to_value(created).unwrap_or_default());
    }

    if pathname == "/api/resume" {
        let slug = request_slug(req, "resume")?;
        if req.method == "GET" {
            let resume = load_resume(&current_workspace, &slug)?;
            return json_response(200, &serde_json::to_value(resume).unwrap_or_default());
        }
        if req.method == "PUT" {
            let body = read_json(req)?;
            let resume = save_resume(&current_workspace, &slug, &body)?;
            return json_response(200, &serde_json::to_value(resume).unwrap_or_default());
        }
        if req.method == "DELETE" {
            delete_resume(&current_workspace, &slug)?;
            return Ok(HttpResponse {
                status: 204,
                headers: security_headers(),
                body: Vec::new(),
            });
        }
    }

    if let Some(rest) = pathname.strip_prefix("/preview/") {
        if req.method == "GET" {
            let slug = rest.trim_end_matches('/');
            if !slug.contains('/') {
                let profile = load_profile(&current_workspace)?;
                let resume = load_resume(&current_workspace, slug)?;
                let html = render_resume(&profile, &resume);
                return html_response(200, &html, PREVIEW_CSP, None);
            }
        }
    }

    if req.method == "GET" {
        return send_static(req, &pathname, &query, state);
    }

    Err(WorkspaceError::not_found("Not found"))
}

fn send_static(
    req: &HttpRequest,
    pathname: &str,
    query: &HashMap<String, String>,
    state: &ServerState,
) -> Result<HttpResponse, WorkspaceError> {
    if pathname.contains('\\') || pathname.split('/').any(|seg| seg == "..") {
        return Err(WorkspaceError::forbidden("Forbidden"));
    }

    if pathname == "/" || pathname == "/index.html" {
        return send_index(req, pathname, query, state);
    }

    if pathname == "/app.js" {
        let mut headers = security_headers();
        headers.push(("Content-Security-Policy".to_string(), APP_CSP.to_string()));
        headers.push((
            "Content-Type".to_string(),
            "text/javascript; charset=utf-8".to_string(),
        ));
        return Ok(HttpResponse {
            status: 200,
            headers,
            body: EMBEDDED_APP_JS.as_bytes().to_vec(),
        });
    }

    if pathname == "/index.css" || pathname == "/styles.css" {
        let mut headers = security_headers();
        headers.push(("Content-Security-Policy".to_string(), APP_CSP.to_string()));
        headers.push((
            "Content-Type".to_string(),
            "text/css; charset=utf-8".to_string(),
        ));
        return Ok(HttpResponse {
            status: 200,
            headers,
            body: EMBEDDED_STYLES_CSS.as_bytes().to_vec(),
        });
    }

    Err(WorkspaceError::not_found("Not found"))
}

fn send_index(
    req: &HttpRequest,
    pathname: &str,
    query: &HashMap<String, String>,
    state: &ServerState,
) -> Result<HttpResponse, WorkspaceError> {
    if let Some(supplied_token) = query.get("token") {
        if !tokens_match(supplied_token, &state.api_token) {
            return Err(WorkspaceError::forbidden("Invalid launch token."));
        }
        let mut remaining: Vec<String> = query
            .iter()
            .filter(|(k, _)| k.as_str() != "token")
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        remaining.sort();
        let location = if remaining.is_empty() {
            pathname.to_string()
        } else {
            format!("{pathname}?{}", remaining.join("&"))
        };
        let mut headers = security_headers();
        headers.push(("Location".to_string(), location));
        headers.push((
            "Set-Cookie".to_string(),
            session_cookie(&state.browser_session_id),
        ));
        return Ok(HttpResponse {
            status: 302,
            headers,
            body: Vec::new(),
        });
    }

    let cookie_val = read_cookie(req, "rf_session");
    if !tokens_match(&cookie_val, &state.browser_session_id) {
        let mut headers = security_headers();
        headers.push((
            "Content-Type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ));
        return Ok(HttpResponse {
            status: 401,
            headers,
            body: b"ResuFlow must be opened from the desktop app or CLI launch URL.".to_vec(),
        });
    }

    html_response(
        200,
        EMBEDDED_INDEX_HTML,
        APP_CSP,
        Some(session_cookie(&state.browser_session_id)),
    )
}

fn assert_trusted_host(req: &HttpRequest, port: u16) -> Result<(), WorkspaceError> {
    let host = req
        .headers
        .get("host")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return Err(WorkspaceError::forbidden("Untrusted host."));
    }
    Ok(())
}

fn assert_trusted_origin(req: &HttpRequest, port: u16) -> Result<(), WorkspaceError> {
    if let Some(origin) = req.headers.get("origin") {
        let expected_ip = format!("http://127.0.0.1:{port}");
        let expected_local = format!("http://localhost:{port}");
        if origin != &expected_ip && origin != &expected_local {
            return Err(WorkspaceError::forbidden(
                "Cross-origin requests are not allowed.",
            ));
        }
    }
    if req.headers.get("sec-fetch-site").map(|s| s.as_str()) == Some("cross-site") {
        return Err(WorkspaceError::forbidden(
            "Cross-site requests are not allowed.",
        ));
    }
    Ok(())
}

fn request_slug(req: &HttpRequest, kind: &str) -> Result<String, WorkspaceError> {
    let slug = req
        .headers
        .get("x-resuflow-slug")
        .cloned()
        .unwrap_or_default();
    assert_slug(&slug, kind)?;
    Ok(slug)
}

fn authorize_request(
    req: &HttpRequest,
    api_token: &str,
    allow_cookie: bool,
    browser_session_id: &str,
    require_browser_header: bool,
) -> Result<(), WorkspaceError> {
    let auth = req.headers.get("authorization").map(|s| s.as_str()).unwrap_or("");
    let bearer = auth.strip_prefix("Bearer ").unwrap_or("");
    let header_token = req
        .headers
        .get("x-resuflow-token")
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(bearer);

    if tokens_match(header_token, api_token) {
        return Ok(());
    }

    if allow_cookie && tokens_match(&read_cookie(req, "rf_session"), browser_session_id) {
        if require_browser_header
            && req.headers.get("x-resuflow-request").map(|s| s.as_str()) != Some("1")
        {
            return Err(WorkspaceError::forbidden("Browser request header required."));
        }
        return Ok(());
    }

    Err(WorkspaceError::unauthorized("Authentication required."))
}

pub fn tokens_match(candidate: &str, expected: &str) -> bool {
    if candidate.is_empty() || expected.is_empty() {
        return false;
    }
    let left = candidate.as_bytes();
    let right = expected.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&a, &b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn read_cookie(req: &HttpRequest, name: &str) -> String {
    let Some(cookie_hdr) = req.headers.get("cookie") else {
        return String::new();
    };
    for part in cookie_hdr.split(';') {
        if let Some((k, v)) = part.split_once('=') {
            if k.trim() == name {
                return v.trim().to_string();
            }
        }
    }
    String::new()
}

fn session_cookie(browser_session_id: &str) -> String {
    format!("rf_session={browser_session_id}; HttpOnly; SameSite=Strict; Path=/")
}

fn read_json(req: &HttpRequest) -> Result<Value, WorkspaceError> {
    let ct = req
        .headers
        .get("content-type")
        .map(|s| s.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .unwrap_or_default();
    if ct != "application/json" {
        return Err(WorkspaceError::new(
            "Content-Type must be application/json.",
            415,
        ));
    }
    if req.body.len() > MAX_JSON_BODY_BYTES {
        return Err(WorkspaceError::new("Request body is too large.", 413));
    }
    let raw = std::str::from_utf8(&req.body)
        .map_err(|_| WorkspaceError::bad_request("Invalid UTF-8 body"))?;
    if raw.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(raw).map_err(|_| WorkspaceError::bad_request("Invalid JSON body"))
}

fn security_headers() -> Vec<(String, String)> {
    vec![
        ("Cache-Control".to_string(), "no-store".to_string()),
        (
            "Cross-Origin-Opener-Policy".to_string(),
            "same-origin".to_string(),
        ),
        (
            "Cross-Origin-Resource-Policy".to_string(),
            "same-origin".to_string(),
        ),
        (
            "Permissions-Policy".to_string(),
            "camera=(), microphone=(), geolocation=(), payment=(), usb=()".to_string(),
        ),
        ("Referrer-Policy".to_string(), "no-referrer".to_string()),
        ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
        ("X-Frame-Options".to_string(), "SAMEORIGIN".to_string()),
    ]
}

fn json_response(status: u16, payload: &Value) -> Result<HttpResponse, WorkspaceError> {
    let body = serde_json::to_vec(payload)
        .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
    let mut headers = security_headers();
    headers.push((
        "Content-Type".to_string(),
        "application/json; charset=utf-8".to_string(),
    ));
    headers.push(("Content-Length".to_string(), body.len().to_string()));
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

fn html_response(
    status: u16,
    html: &str,
    csp: &str,
    set_cookie: Option<String>,
) -> Result<HttpResponse, WorkspaceError> {
    let body = html.as_bytes().to_vec();
    let mut headers = security_headers();
    headers.push(("Content-Security-Policy".to_string(), csp.to_string()));
    headers.push((
        "Content-Type".to_string(),
        "text/html; charset=utf-8".to_string(),
    ));
    headers.push(("Content-Length".to_string(), body.len().to_string()));
    if let Some(cookie) = set_cookie {
        headers.push(("Set-Cookie".to_string(), cookie));
    }
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

fn error_response(err: &WorkspaceError) -> HttpResponse {
    let status = if err.status >= 100 { err.status } else { 500 };
    let message = if status >= 500 {
        "Internal server error"
    } else {
        &err.message
    };
    let body = serde_json::to_vec(&json!({ "error": message })).unwrap_or_default();
    let mut headers = security_headers();
    headers.push((
        "Content-Type".to_string(),
        "application/json; charset=utf-8".to_string(),
    ));
    headers.push(("Content-Length".to_string(), body.len().to_string()));
    HttpResponse {
        status,
        headers,
        body,
    }
}

fn write_http_response(stream: &mut TcpStream, resp: &HttpResponse) -> std::io::Result<()> {
    let reason = match resp.status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        _ => "Internal Server Error",
    };
    write!(stream, "HTTP/1.1 {} {}\r\n", resp.status, reason)?;
    for (k, v) in &resp.headers {
        write!(stream, "{}: {}\r\n", k, v)?;
    }
    write!(stream, "Connection: close\r\n\r\n")?;
    if !resp.body.is_empty() {
        stream.write_all(&resp.body)?;
    }
    stream.flush()
}

fn percent_decode(input: &str) -> Result<String, WorkspaceError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(WorkspaceError::bad_request("Invalid URL encoding."));
            }
            let hi = hex_val(bytes[i + 1])
                .ok_or_else(|| WorkspaceError::bad_request("Invalid URL encoding."))?;
            let lo = hex_val(bytes[i + 2])
                .ok_or_else(|| WorkspaceError::bad_request("Invalid URL encoding."))?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| WorkspaceError::bad_request("Invalid URL encoding."))
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        if let (Ok(dk), Ok(dv)) = (percent_decode(k), percent_decode(v)) {
            map.insert(dk, dv);
        }
    }
    map
}

fn is_safe_hash(hash: &str) -> bool {
    let Some(rest) = hash.strip_prefix("#/") else {
        return false;
    };
    rest.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_~./-".contains(&b))
}

pub fn normalize_local_app_url(value: &str) -> Result<String, WorkspaceError> {
    let trimmed = value.trim();
    let Some(rest) = trimmed.strip_prefix("http://") else {
        return Err(WorkspaceError::bad_request(
            "Only a local ResuFlow URL can be opened.",
        ));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    if host != "127.0.0.1" && host != "localhost" {
        return Err(WorkspaceError::bad_request(
            "Only a local ResuFlow URL can be opened.",
        ));
    }
    if trimmed.contains('/') {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("{trimmed}/"))
    }
}

pub fn open_browser(url: &str) {
    let Ok(local_url) = normalize_local_app_url(url) else {
        eprintln!("Could not open a browser: Only a local ResuFlow URL can be opened.");
        return;
    };

    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd")
        .args(["/C", "start", "", &local_url])
        .spawn();

    #[cfg(not(target_os = "windows"))]
    let _ = Command::new("xdg-open").arg(&local_url).spawn();
}
