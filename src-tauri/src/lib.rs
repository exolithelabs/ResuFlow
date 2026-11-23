pub mod cli;
pub mod markdown;
pub mod mcp;
pub mod memory;
pub mod releases;
pub mod render;
pub mod server;
pub mod skills_store;
pub mod user_config;
pub mod workspace;

use cli::{help_text, parse_args, CommandMode};
use server::{ open_browser, start_server, StartServerConfig };
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use user_config::{home_dir, load_user_config, remember_vault};
use uuid::Uuid;
use workspace::{find_workspace, init_workspace, Workspace, WorkspaceError};

pub fn run() {
    if let Err(err) = try_run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn try_run() -> Result<(), WorkspaceError> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&raw_args)?;

    if args.help || args.command == CommandMode::Help {
        println!("{}", help_text());
        return Ok(());
    }

    if args.command == CommandMode::Init {
        let target = args
            .dir
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let workspace = init_workspace(&target)?;
        let _ = remember_vault(&workspace.root, true);
        println!("Workspace ready: {}", workspace.root.display());
        println!("This is now the default vault. Next: resuflow");
        return Ok(());
    }

    if args.command == CommandMode::Serve {
        let workspace = resolve_workspace(args.dir.as_deref(), false)?;
        let started = start_server(StartServerConfig {
            workspace_root: workspace.root.clone(),
            port: args.port,
            api_token: None,
            launch_token: None,
        })?;
        println!("ResuFlow");
        println!("Workspace: {}", workspace.root.display());
        println!("Web UI:    {}", started.url);
        println!("MCP:       {}", started.mcp_url);
        if args.open_browser {
            open_browser(&started.launch_url);
        } else {
            println!("Authentication: open with --browser, then copy the MCP configuration from Read docs.");
        }
        loop {
            thread::park();
        }
    }

    // Desktop mode (default)
    let workspace = resolve_workspace(args.dir.as_deref(), true)?;
    let preferred_port = std::env::var("RESUFLOW_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(args.port);
    let port = available_port(preferred_port)
        .map_err(|e| WorkspaceError::new(format!("Could not allocate a localhost port: {e}"), 500))?;
    let launch_token = Uuid::new_v4().to_string();
    let api_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

    let started = start_server(StartServerConfig {
        workspace_root: workspace.root,
        port,
        api_token: Some(api_token.clone()),
        launch_token: Some(launch_token),
    })?;

    let window_url = format!("http://127.0.0.1:{}/?desktop=1&token={}", started.port, api_token);

    tauri::Builder::default()
        .setup(move |app| {
            #[allow(unused_mut)]
            let mut builder =
                WebviewWindowBuilder::new(app.handle(), "main", WebviewUrl::External(window_url.parse()?))
                    .title("ResuFlow")
                    .inner_size(1320.0, 900.0)
                    .min_inner_size(800.0, 600.0)
                    .resizable(true)
                    .center();

            #[cfg(target_os = "windows")]
            {
                builder = builder.additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI");
            }

            builder.build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .map_err(|e| WorkspaceError::new(format!("Failed to start ResuFlow desktop window: {e}"), 500))?
        .run(|_app, _event| {});

    Ok(())
}

pub fn resolve_workspace(dir: Option<&str>, create_default: bool) -> Result<Workspace, WorkspaceError> {
    if let Some(d) = dir.or_else(|| None) {
        let workspace = find_workspace(&PathBuf::from(d))?;
        let _ = remember_vault(&workspace.root, true);
        return Ok(workspace);
    }
    if let Ok(env_ws) = std::env::var("RESUFLOW_WORKSPACE") {
        if !env_ws.trim().is_empty() {
            let workspace = find_workspace(&PathBuf::from(env_ws))?;
            let _ = remember_vault(&workspace.root, true);
            return Ok(workspace);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(ws) = find_workspace(&cwd) {
            return Ok(ws);
        }
    }
    let config = load_user_config();
    if !config.default_vault.is_empty() {
        if let Ok(ws) = find_workspace(&PathBuf::from(&config.default_vault)) {
            return Ok(ws);
        }
    }
    if create_default {
        let default_dir = home_dir().join("Documents").join("ResuFlow");
        let workspace = init_workspace(&default_dir)?;
        let _ = remember_vault(&workspace.root, true);
        return Ok(workspace);
    }
    Err(WorkspaceError::bad_request(
        "No vault found. Run `resuflow init <dir>` or set a default vault in the app (Settings → Vault).",
    ))
}

pub fn available_port(preferred: u16) -> std::io::Result<u16> {
    if let Ok(listener) = TcpListener::bind(("127.0.0.1", preferred)) {
        return Ok(listener.local_addr()?.port());
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use super::available_port;
    use crate::cli::{parse_args, CommandMode};
    use crate::releases::{check_latest_release_with, compare_versions};
    use crate::render::render_resume;
    use crate::server::{normalize_local_app_url, start_server, StartServerConfig, StartedServer};
    use crate::skills_store::load_skill;
    use crate::workspace::{
        find_workspace, init_workspace, markdown_to_profile, markdown_to_resume, save_profile,
    };
    use serde_json::{json, Value};
    use std::cmp::Ordering;
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestFixture {
        server: StartedServer,
        root: PathBuf,
    }

    impl TestFixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("resuflow-test-{}", Uuid::new_v4()));
            init_workspace(&root).unwrap();
            let server = start_server(StartServerConfig {
                workspace_root: root.clone(),
                port: 0,
                api_token: None,
                launch_token: Some("desktop-health-token".to_string()),
            })
            .unwrap();
            Self { server, root }
        }
    }

    impl Drop for TestFixture {
        fn drop(&mut self) {
            self.server.close();
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn raw_http_request(
        port: u16,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (u16, HashMap<String, String>, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut req = format!("{method} {target} HTTP/1.1\r\n");
        let has_host = headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("host"));
        if !has_host {
            req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
        }
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        if !body.is_empty() {
            req.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        req.push_str("Connection: close\r\n\r\n");
        req.push_str(body);
        stream.write_all(req.as_bytes()).unwrap();

        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        let (head, resp_body) = resp.split_once("\r\n\r\n").unwrap_or((&resp, ""));
        let mut lines = head.split("\r\n");
        let status_line = lines.next().unwrap_or("");
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let mut resp_headers = HashMap::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                resp_headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        (status, resp_headers, resp_body.to_string())
    }

    #[test]
    fn selects_another_port_when_the_preferred_port_is_occupied() {
        let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();
        assert_ne!(available_port(occupied_port).unwrap(), occupied_port);
    }

    #[test]
    fn parses_cli_commands_and_flags() {
        let help = parse_args(&["--help".to_string()]).unwrap();
        assert!(help.help);

        let browser = parse_args(&["--browser".to_string(), "/tmp/vault".to_string()]).unwrap();
        assert_eq!(browser.command, CommandMode::Serve);
        assert!(browser.open_browser);
        assert_eq!(browser.dir.as_deref(), Some("/tmp/vault"));

        let init = parse_args(&["init".to_string(), "/tmp/new-vault".to_string()]).unwrap();
        assert_eq!(init.command, CommandMode::Init);
        assert_eq!(init.dir.as_deref(), Some("/tmp/new-vault"));

        assert!(parse_args(&["--port=99999".to_string()]).is_err());
        assert!(parse_args(&["--unknown-flag".to_string()]).is_err());
    }

    #[test]
    fn compares_stable_and_prerelease_semantic_versions() {
        assert_eq!(compare_versions("1.2.0", "1.1.9").unwrap(), Ordering::Greater);
        assert_eq!(compare_versions("v1.2.0", "1.2.0").unwrap(), Ordering::Equal);
        assert_eq!(
            compare_versions("1.2.0-beta.2", "1.2.0-beta.1").unwrap(),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.2.0", "1.2.0-beta.2").unwrap(),
            Ordering::Greater
        );
    }

    #[test]
    fn recognizes_newer_valid_github_release_and_rejects_external_urls() {
        let result = check_latest_release_with("1.0.0", false, |_| {
            Ok((
                200,
                json!({
                    "tag_name": "v1.1.0",
                    "name": "ResuFlow 1.1.0",
                    "html_url": "https://github.com/exolithelabs/resuflow/releases/tag/v1.1.0",
                    "published_at": "2026-09-12T00:00:00Z"
                })
                .to_string(),
            ))
        })
        .unwrap();
        assert_eq!(result["available"], json!(true));
        assert_eq!(result["latestVersion"], json!("1.1.0"));

        let rejected = check_latest_release_with("1.0.0", false, |_| {
            Ok((
                200,
                json!({
                    "tag_name": "v9.0.0",
                    "html_url": "https://example.com/fake-release"
                })
                .to_string(),
            ))
        });
        assert!(rejected.is_err());
    }

    #[test]
    fn localhost_api_requires_token_and_rejects_cross_origin_mutation() {
        let fx = TestFixture::new();
        let port = fx.server.port;
        let token = &fx.server.api_token;

        let (status, _, _) = raw_http_request(port, "GET", "/api/resumes", &[], "");
        assert_eq!(status, 401);

        let (status, _, _) = raw_http_request(
            port,
            "GET",
            "/api/resumes",
            &[("Host", "attacker.example"), ("X-ResuFlow-Token", token)],
            "",
        );
        assert_eq!(status, 403);

        let (status, _, body) = raw_http_request(port, "GET", "/api/health", &[], "");
        assert_eq!(status, 200);
        let health: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(health["launchToken"], json!("desktop-health-token"));
        assert_ne!(health["launchToken"], json!(token));

        let (status, _, _) = raw_http_request(
            port,
            "POST",
            "/api/resumes",
            &[
                ("Content-Type", "text/plain"),
                ("Origin", "https://attacker.example"),
                ("X-ResuFlow-Token", token),
            ],
            r#"{"title":"Blocked request"}"#,
        );
        assert_eq!(status, 403);

        let (status, _, _) = raw_http_request(
            port,
            "POST",
            "/mcp",
            &[
                ("Authorization", &format!("Bearer {token}")),
                ("Content-Type", "application/json"),
                ("Origin", "https://attacker.example"),
            ],
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_resume","arguments":{"title":"Blocked"}}}"#,
        );
        assert_eq!(status, 403);

        let (status, _, body) = raw_http_request(
            port,
            "GET",
            "/api/resumes",
            &[("X-ResuFlow-Token", token)],
            "",
        );
        assert_eq!(status, 200);
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["resumes"], json!([]));
    }

    #[test]
    fn browser_bootstrap_exchanges_token_for_strict_session_cookie() {
        let fx = TestFixture::new();
        let port = fx.server.port;
        let token = &fx.server.api_token;

        let (status, headers, _) =
            raw_http_request(port, "GET", &format!("/?token={token}"), &[], "");
        assert_eq!(status, 302);
        assert_eq!(headers.get("location").map(|s| s.as_str()), Some("/"));
        let set_cookie = headers.get("set-cookie").unwrap();
        assert!(set_cookie.contains("rf_session="));
        assert!(!set_cookie.contains(token));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));

        let cookie_pair = set_cookie.split(';').next().unwrap();

        let (status, _, page_html) =
            raw_http_request(port, "GET", "/", &[("Cookie", cookie_pair)], "");
        assert_eq!(status, 200);
        assert!(!page_html.contains(token));

        let (status, _, _) = raw_http_request(
            port,
            "POST",
            "/api/resumes",
            &[
                ("Cookie", cookie_pair),
                ("Content-Type", "application/json"),
            ],
            r#"{"title":"Blocked request"}"#,
        );
        assert_eq!(status, 403);

        let (status, _, _) = raw_http_request(
            port,
            "POST",
            "/api/resumes",
            &[
                ("Cookie", cookie_pair),
                ("Content-Type", "application/json"),
                ("X-ResuFlow-Request", "1"),
            ],
            r#"{"title":"Browser request"}"#,
        );
        assert_eq!(status, 201);

        let (status, _, _) = raw_http_request(
            port,
            "GET",
            "/api/resume",
            &[
                ("Cookie", cookie_pair),
                ("X-ResuFlow-Slug", "browser-request"),
            ],
            "",
        );
        assert_eq!(status, 200);
    }

    #[test]
    fn browser_launcher_accepts_only_local_http_app_urls() {
        assert_eq!(
            normalize_local_app_url("http://127.0.0.1:4173/?token=value").unwrap(),
            "http://127.0.0.1:4173/?token=value"
        );
        assert_eq!(
            normalize_local_app_url("http://localhost:4173/").unwrap(),
            "http://localhost:4173/"
        );
        assert!(normalize_local_app_url("https://example.com/").is_err());
        assert!(normalize_local_app_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn json_endpoints_reject_wrong_content_type_and_oversized_bodies() {
        let fx = TestFixture::new();
        let port = fx.server.port;
        let token = &fx.server.api_token;

        let (status, _, _) = raw_http_request(
            port,
            "POST",
            "/api/resumes",
            &[
                ("Content-Type", "text/plain"),
                ("X-ResuFlow-Token", token),
            ],
            r#"{"title":"No"}"#,
        );
        assert_eq!(status, 415);

        let oversized = format!(r#"{{"title":"{}"}}"#, "x".repeat(1024 * 1024));
        let (status, _, _) = raw_http_request(
            port,
            "POST",
            "/api/resumes",
            &[
                ("Content-Type", "application/json"),
                ("X-ResuFlow-Token", token),
            ],
            &oversized,
        );
        assert_eq!(status, 413);
    }

    #[test]
    fn encoded_traversal_cannot_escape_static_or_skill_directories() {
        let fx = TestFixture::new();
        let port = fx.server.port;
        let token = &fx.server.api_token;

        let (status, _, _) = raw_http_request(port, "GET", "/..%2Fpackage.json", &[], "");
        assert_eq!(status, 403);

        let (status, _, _) = raw_http_request(
            port,
            "GET",
            "/api/skill",
            &[
                ("X-ResuFlow-Token", token),
                ("X-ResuFlow-Slug", "..\\audit-target"),
            ],
            "",
        );
        assert_eq!(status, 400);

        assert!(load_skill(&fx.root, "..\\audit-target").is_err());
    }

    #[test]
    fn resume_rendering_removes_scripts_event_handlers_and_unsafe_urls() {
        let profile = markdown_to_profile("---\nname: Safe\n---\n");
        let resume = markdown_to_resume(
            "---\ntitle: Security test\n---\n\n<script>globalThis.pwned=true</script>\n\n<img src=x onerror=\"alert(1)\">\n\n[bad](javascript:alert(1))\n",
            "sec-test",
        );
        let html = render_resume(&profile, &resume);
        assert!(!html.to_ascii_lowercase().contains("<script"));
        assert!(!html.to_ascii_lowercase().contains("onerror"));
        assert!(!html.to_ascii_lowercase().contains("javascript:"));
        assert!(!html.to_ascii_lowercase().contains("<img"));
    }

    #[test]
    fn workspace_markers_migrate_safely_and_reject_future_schemas() {
        let root = std::env::temp_dir().join(format!("resuflow-mig-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("resuflow.json"), "{}").unwrap();
        let migrated = find_workspace(&root).unwrap();
        assert_eq!(migrated.marker["schemaVersion"], json!(1));

        fs::write(root.join("resuflow.json"), r#"{"schemaVersion":999}"#).unwrap();
        assert!(find_workspace(&root).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_endpoint_handles_tools_and_prompts() {
        let fx = TestFixture::new();
        let port = fx.server.port;
        let token = &fx.server.api_token;

        let (status, _, body) = raw_http_request(
            port,
            "POST",
            "/mcp",
            &[
                ("Authorization", &format!("Bearer {token}")),
                ("Content-Type", "application/json"),
            ],
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_resume","arguments":{"title":"Staff Engineer"}}}"#,
        );
        assert_eq!(status, 200);
        let resp: Value = serde_json::from_str(&body).unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("staff-engineer"));

        let _ = save_profile(&fx.root, &json!({ "name": "Test User", "body": "Bio" })).unwrap();
        let profile_text = fs::read_to_string(fx.root.join("profile.md")).unwrap();
        assert!(profile_text.contains("name: Test User"));
    }
}
