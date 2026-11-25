//! Desktop shell: Tauri commands over the same `Workspace` the MCP server uses, a `preview://`
//! scheme that serves rendered resumes, printing and PDF export. No behaviour lives here.

use crate::config;
use crate::lint::Report;
use crate::skills::{self, Skill};
use crate::workspace::{InstructionFile, NewResume, Parts, ResumeSummary, Target, TextFile, TrashEntry, Workspace};
use serde::Serialize;
use serde_json::{json, Map, Value};
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

/// "profile" or "resume:<slug>"
fn target(file: &str) -> Res<Target> {
    match file {
        "profile" => Ok(Target::Profile),
        _ => file.strip_prefix("resume:").map(|s| Target::Resume(s.into())).ok_or_else(|| format!("unknown file '{file}'")),
    }
}

fn exe() -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "resuflow".into())
}

/// MCP client configuration pointing at this exact binary.
pub fn mcp_config() -> Value {
    json!({ "mcpServers": { "resuflow": { "command": exe(), "args": ["mcp"] } } })
}

#[derive(Serialize)]
struct Info {
    root: PathBuf,
    exe: String,
    mcp_config: Value,
    version: &'static str,
    pdf_export: bool,
    preview_base: String,
}

#[tauri::command]
fn info(s: State<AppState>) -> Res<Info> {
    Ok(Info {
        root: with(&s, |w| Ok(w.root.clone()))?,
        exe: exe(),
        mcp_config: mcp_config(),
        version: env!("CARGO_PKG_VERSION"),
        pdf_export: crate::pdf::supported(),
        preview_base: preview_url(""),
    })
}

#[tauri::command]
fn revision(s: State<AppState>) -> Res<String> {
    with(&s, |w| Ok(w.revision()))
}

#[tauri::command]
fn list_resumes(s: State<AppState>) -> Res<Vec<ResumeSummary>> {
    with(&s, |w| w.list_resumes())
}

/// Returns the new resume's slug.
#[tauri::command]
fn create_resume(s: State<AppState>, title: String, from: Option<String>) -> Res<String> {
    with(&s, |w| w.create_resume(NewResume { title: &title, from: from.as_deref(), ..Default::default() }).map(|r| r.summary.slug))
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
fn set_fields(s: State<AppState>, file: String, fields: Map<String, Value>, expected_hash: Option<String>) -> Res<Parts> {
    let t = target(&file)?;
    with(&s, |w| w.set_fields(&t, &fields, expected_hash.as_deref()))
}

#[tauri::command]
fn set_meta(
    s: State<AppState>,
    slug: String,
    title: Option<String>,
    status: Option<String>,
    tags: Option<Vec<String>>,
    expected_hash: Option<String>,
) -> Res<Parts> {
    with(&s, |w| {
        w.set_meta(&slug, title.as_deref(), status.as_deref(), tags, expected_hash.as_deref())?;
        w.read_parts(&Target::Resume(slug.clone()))
    })
}

#[tauri::command]
fn delete_resume(s: State<AppState>, slug: String) -> Res<TrashEntry> {
    with(&s, |w| w.delete_resume(&slug))
}

#[tauri::command]
fn list_trash(s: State<AppState>) -> Res<Vec<TrashEntry>> {
    with(&s, |w| w.list_trash())
}

/// Returns the restored resume's slug.
#[tauri::command]
fn restore_resume(s: State<AppState>, id: String) -> Res<String> {
    with(&s, |w| w.restore_resume(&id).map(|r| r.summary.slug))
}

#[tauri::command]
fn read_posting(s: State<AppState>, slug: String) -> Res<TextFile> {
    with(&s, |w| w.read_posting(&slug))
}

#[tauri::command]
fn write_posting(s: State<AppState>, slug: String, content: String, expected_hash: Option<String>) -> Res<TextFile> {
    with(&s, |w| w.write_posting(&slug, &content, expected_hash.as_deref()))
}

#[tauri::command]
fn read_instructions(s: State<AppState>) -> Res<InstructionFile> {
    with(&s, |w| w.read_instructions())
}

#[tauri::command]
fn write_instructions(s: State<AppState>, items: Vec<String>, expected_hash: Option<String>) -> Res<InstructionFile> {
    with(&s, |w| w.write_instructions(&items, expected_hash.as_deref()))
}

#[tauri::command]
fn check_resume(s: State<AppState>, slug: String) -> Res<Report> {
    with(&s, |w| w.check(&slug))
}

#[tauri::command]
fn list_skills() -> Vec<Skill> {
    skills::list()
}

#[tauri::command]
fn install_skills(agent: String) -> Res<String> {
    skills::install(&agent).map(|p| p.display().to_string())
}

static PRINT_SEQ: AtomicU32 = AtomicU32::new(0);

/// Open the rendered resume in its own window and hand it to the system print dialog,
/// where "Save as PDF" is available on every platform.
#[tauri::command]
async fn print_resume(app: tauri::AppHandle, s: State<'_, AppState>, slug: String) -> Res<()> {
    let title = with(&s, |w| w.read_resume(&slug))?.summary.title;
    let url = tauri::Url::parse(&preview_url(&slug)).map_err(|e| e.to_string())?;
    WebviewWindowBuilder::new(&app, format!("print-{}", PRINT_SEQ.fetch_add(1, Ordering::Relaxed)), WebviewUrl::CustomProtocol(url))
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

/// Ask where to save, then write the resume there as a PDF. Returns the path, or None if cancelled.
#[tauri::command]
async fn save_pdf(app: tauri::AppHandle, s: State<'_, AppState>, slug: String) -> Res<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (title, name) = with(&s, |w| {
        let r = crate::doc::Doc::parse(&w.read_resume(&slug)?.content);
        Ok((r.str("title").unwrap_or_else(|| slug.clone()), r.str("name").or_else(|| w.profile().str("name"))))
    })?;
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Export resume as PDF")
        .set_file_name(pdf_file_name(name.as_deref(), &title))
        .add_filter("PDF", &["pdf"]);
    if let Some(dir) = dirs::download_dir() {
        dialog = dialog.set_directory(dir);
    }
    let Some(picked) = dialog.blocking_save_file() else { return Ok(None) };
    let mut path = picked.into_path().map_err(|e| e.to_string())?;
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        path.set_extension("pdf");
    }
    let (tx, rx) = std::sync::mpsc::channel();
    crate::pdf::export(&app, &slug, path, Box::new(move |r| drop(tx.send(r))))?;
    let result = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(60)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|_| "PDF export timed out".to_string())?;
    result.map(|p| Some(p.display().to_string()))
}

fn pdf_file_name(name: Option<&str>, title: &str) -> String {
    let raw = match name {
        Some(n) if n != "Your Name" => format!("{n} - {title}"),
        _ => title.to_string(),
    };
    let clean: String = raw.chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '-' } else { c }).collect();
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
            delete_resume, list_trash, restore_resume, read_posting, write_posting,
            read_instructions, write_instructions, check_resume, list_skills, install_skills,
            print_resume, save_pdf
        ])
        .run(tauri::generate_context!())
        .expect("failed to start ResuFlow");
}

/// `resuflow pdf SLUG -o FILE`: export without showing a window, then exit.
pub fn export_pdf(dir: Option<PathBuf>, slug: String, out: PathBuf) -> Result<(), String> {
    if !crate::pdf::supported() {
        return Err("direct PDF export is only available on Linux; use `resuflow render` and print the HTML".into());
    }
    let ws = config::workspace(dir).map_err(|e| e.0)?;
    ws.read_resume(&slug).map_err(|e| e.0)?;
    let out = std::path::absolute(&out).map_err(|e| e.to_string())?;
    let ok = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ok2 = ok.clone();
    let app = builder(ws)
        .setup(move |app| {
            let handle = app.handle().clone();
            let exit = handle.clone();
            crate::pdf::export(&handle, &slug, out, Box::new(move |r| {
                match r {
                    Ok(p) => {
                        println!("Saved {}", p.display());
                        ok2.store(true, Ordering::Relaxed);
                    }
                    Err(e) => eprintln!("resuflow: {e}"),
                }
                exit.exit(0);
            }))?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .map_err(|e| e.to_string())?;
    app.run(|_, _| {});
    if ok.load(Ordering::Relaxed) { Ok(()) } else { Err(String::new()) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pdf_names_are_safe() {
        assert_eq!(super::pdf_file_name(Some("Ada L"), "Backend / Infra"), "Ada L - Backend - Infra.pdf");
        assert_eq!(super::pdf_file_name(Some("Your Name"), "X"), "X.pdf");
        assert_eq!(super::pdf_file_name(None, "X: y"), "X- y.pdf");
    }
}
