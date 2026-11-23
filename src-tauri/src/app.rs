//! Desktop shell: Tauri commands over the same `Workspace` the MCP server uses,
//! a `preview://` scheme that serves rendered resumes, and native printing.

use crate::config;
use crate::lint::Report;
use crate::skills::{self, Skill};
use crate::workspace::{Note, Parts, ResumeSummary, Target, Workspace};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::http::{header, Response, StatusCode};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

struct AppState {
    ws: Mutex<Workspace>,
}

type Res<T> = Result<T, String>;

fn with<T>(s: &State<AppState>, f: impl FnOnce(&Workspace) -> crate::workspace::Result<T>) -> Res<T> {
    let ws = s.ws.lock().map_err(|_| "workspace lock poisoned".to_string())?;
    f(&ws).map_err(|e| e.0)
}

fn target(t: &str) -> Res<Target> {
    match t {
        "profile" => Ok(Target::Note(Note::Profile)),
        "memory" => Ok(Target::Note(Note::Memory)),
        _ => t.strip_prefix("resume:").map(|s| Target::Resume(s.into())).ok_or_else(|| format!("unknown file '{t}'")),
    }
}

/// MCP client configuration pointing at this exact binary.
pub fn mcp_config() -> Value {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "resuflow".into());
    json!({ "mcpServers": { "resuflow": { "command": exe, "args": ["mcp"] } } })
}

#[derive(Serialize)]
struct Info {
    root: PathBuf,
    mcp_config: Value,
    mcp_command: String,
    version: &'static str,
    pdf_export: bool,
}

#[tauri::command]
fn info(s: State<AppState>) -> Res<Info> {
    let root = with(&s, |w| Ok(w.root.clone()))?;
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "resuflow".into());
    Ok(Info { root, mcp_config: mcp_config(), mcp_command: format!("{exe} mcp"), version: env!("CARGO_PKG_VERSION"), pdf_export: crate::pdf::supported() })
}

#[tauri::command]
fn revision(s: State<AppState>) -> Res<String> {
    with(&s, |w| Ok(w.revision()))
}

#[tauri::command]
fn list_resumes(s: State<AppState>) -> Res<Vec<ResumeSummary>> {
    with(&s, |w| w.list_resumes())
}

#[tauri::command]
fn create_resume(s: State<AppState>, title: String) -> Res<String> {
    with(&s, |w| w.create_resume(&title, None, None, None, vec![]).map(|r| r.summary.slug))
}

#[tauri::command]
fn read_parts(s: State<AppState>, file: String) -> Res<Parts> {
    let t = target(&file)?;
    with(&s, |w| w.read_parts(&t))
}

#[tauri::command]
fn write_body(s: State<AppState>, file: String, body: String, expected_hash: Option<String>) -> Res<Parts> {
    let t = target(&file)?;
    with(&s, |w| w.write_body(&t, &body, expected_hash.as_deref()))
}

#[tauri::command]
fn set_fields(
    s: State<AppState>,
    file: String,
    fields: serde_json::Map<String, Value>,
    expected_hash: Option<String>,
) -> Res<Parts> {
    let t = target(&file)?;
    with(&s, |w| w.set_fields(&t, &fields, expected_hash.as_deref()))
}

#[tauri::command]
fn list_skills() -> Vec<Skill> {
    skills::list()
}

#[tauri::command]
fn install_skills(agent: String) -> Res<String> {
    skills::install(&agent).map(|p| p.display().to_string())
}

#[tauri::command]
fn set_meta(
    s: State<AppState>,
    slug: String,
    title: Option<String>,
    status: Option<String>,
    tags: Option<Vec<String>>,
) -> Res<Parts> {
    with(&s, |w| {
        w.set_meta(&slug, title.as_deref(), status.as_deref(), tags)?;
        w.read_parts(&Target::Resume(slug.clone()))
    })
}

#[tauri::command]
fn delete_resume(s: State<AppState>, slug: String) -> Res<String> {
    with(&s, |w| w.delete_resume(&slug))
}

#[tauri::command]
fn check_resume(s: State<AppState>, slug: String) -> Res<Report> {
    with(&s, |w| w.check(&slug))
}

static PRINT_SEQ: AtomicU32 = AtomicU32::new(0);

/// Open the rendered resume in its own window and hand it to the OS print dialog
/// (choose "Save as PDF" there). Real text, A4, no headless browser needed.
#[tauri::command]
async fn print_resume(app: tauri::AppHandle, s: State<'_, AppState>, slug: String) -> Res<()> {
    let title = with(&s, |w| w.read_resume(&slug))?.summary.title;
    let url = tauri::Url::parse(&preview_url(&slug)).map_err(|e| e.to_string())?;
    let label = format!("print-{}", PRINT_SEQ.fetch_add(1, Ordering::Relaxed));
    WebviewWindowBuilder::new(&app, label, WebviewUrl::CustomProtocol(url))
        .title(format!("Print · {title}"))
        .inner_size(860.0, 1040.0)
        .on_page_load(|w, p| {
            if matches!(p.event(), tauri::webview::PageLoadEvent::Finished) {
                let _ = w.print();
            }
        })
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Ask where to save, then write the resume as a PDF there. Returns the path, or None if cancelled.
#[tauri::command]
async fn save_pdf(app: tauri::AppHandle, s: State<'_, AppState>, slug: String) -> Res<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (title, name) = with(&s, |w| Ok((w.read_resume(&slug)?.summary.title, w.profile().str("name"))))?;
    let file_name = default_file_name(name.as_deref(), &title);
    let mut dialog = app.dialog().file().set_title("Save resume as PDF").set_file_name(&file_name).add_filter("PDF", &["pdf"]);
    if let Some(dir) = dirs::download_dir() {
        dialog = dialog.set_directory(dir);
    }
    let Some(picked) = dialog.blocking_save_file() else { return Ok(None) };
    let mut path = picked.into_path().map_err(|e| e.to_string())?;
    if path.extension().map_or(true, |e| !e.eq_ignore_ascii_case("pdf")) {
        path.set_extension("pdf");
    }
    let (tx, rx) = std::sync::mpsc::channel();
    crate::pdf::export(&app, &slug, path, Box::new(move |r| { let _ = tx.send(r); }))?;
    let result = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(60)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|_| "PDF export timed out".to_string())?;
    result.map(|p| Some(p.display().to_string()))
}

fn default_file_name(name: Option<&str>, title: &str) -> String {
    let raw = match name {
        Some(n) if !n.is_empty() && n != "Your Name" => format!("{n} - {title}"),
        _ => title.to_string(),
    };
    let clean: String = raw.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c }).collect();
    format!("{}.pdf", clean.trim())
}

pub(crate) fn preview_url(slug: &str) -> String {
    if cfg!(any(windows, target_os = "android")) {
        format!("http://preview.localhost/{slug}")
    } else {
        format!("preview://localhost/{slug}")
    }
}

fn builder(ws: Workspace) -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { ws: Mutex::new(ws) })
        .register_uri_scheme_protocol("preview", |ctx, req| {
            let slug = req.uri().path().trim_matches('/').to_string();
            let state = ctx.app_handle().state::<AppState>();
            let rendered = state.ws.lock().map_err(|_| ()).and_then(|w| w.render(&slug).map_err(|_| ()));
            let (status, body) = match rendered {
                Ok(html) => (StatusCode::OK, html),
                Err(()) => (StatusCode::NOT_FOUND, format!("<p>No resume named {}</p>", crate::render::esc(&slug))),
            };
            Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(body.into_bytes())
                .unwrap()
        })
}

fn open_or_exit(dir: Option<PathBuf>) -> Workspace {
    config::workspace(dir).unwrap_or_else(|e| {
        eprintln!("resuflow: {e}");
        std::process::exit(1);
    })
}

pub fn run(dir: Option<PathBuf>) {
    builder(open_or_exit(dir))
        .invoke_handler(tauri::generate_handler![
            info, revision, list_resumes, create_resume, read_parts, write_body, set_fields, set_meta,
            delete_resume, check_resume, list_skills, install_skills, print_resume, save_pdf
        ])
        .run(tauri::generate_context!())
        .expect("failed to start ResuFlow");
}

/// `resuflow pdf SLUG -o FILE`: export without showing a window, then exit.
pub fn export_pdf(dir: Option<PathBuf>, slug: String, out: PathBuf) -> Result<(), String> {
    if !crate::pdf::supported() {
        return Err("direct PDF export is only available on Linux; use `resuflow render` and print the HTML".into());
    }
    let ws = open_or_exit(dir);
    ws.read_resume(&slug).map_err(|e| e.0)?;
    let out = std::path::absolute(&out).map_err(|e| e.to_string())?;
    let code = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(1));
    let code2 = code.clone();
    let app = builder(ws)
        .setup(move |app| {
            let handle = app.handle().clone();
            let h2 = handle.clone();
            crate::pdf::export(&handle, &slug, out, Box::new(move |r| {
                match r {
                    Ok(p) => {
                        println!("Saved {}", p.display());
                        code2.store(0, Ordering::Relaxed);
                    }
                    Err(e) => eprintln!("resuflow: {e}"),
                }
                h2.exit(0);
            }))?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .map_err(|e| e.to_string())?;
    app.run(|_, _| {});
    match code.load(Ordering::Relaxed) {
        0 => Ok(()),
        _ => Err(String::new()),
    }
}
