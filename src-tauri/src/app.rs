//! Desktop shell: Tauri commands over the same `Workspace` the MCP server uses, a `preview://`
//! scheme that serves rendered resumes, printing, PDF export and in-app updates. No resume
//! behaviour lives here.
//!
//! Updates are desktop-only: they replace the installed app (and with it the `resuflow` binary
//! agents run as their MCP server), so the MCP server doesn't offer them. The update
//! is signed and verified against the key in `tauri.conf.json`; the data folder is upgraded, with
//! a backup, the next time the new version opens it (see `Workspace::upgrade`).

use crate::config;
use crate::lint::Report;
use crate::skills::{self, Skill};
use crate::workspace::{self, InstructionFile, NewResume, Parts, ResumeSummary, Target, TextFile, Upgrade, Workspace};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::http::{header, Response, StatusCode};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_updater::{Update, UpdaterExt};

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

/// The path agents should run. Inside an AppImage the running binary sits in a temporary mount
/// that disappears when the app quits, so point at the AppImage itself.
fn exe() -> String {
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return PathBuf::from(appimage).display().to_string();
    }
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
    /// The data folder's format, and the upgrade opening it just did, if any.
    format: u64,
    upgraded: Option<Upgrade>,
    /// Whether this copy can update itself (see [`self_update`]).
    updates: bool,
    license: &'static str,
}

#[tauri::command]
fn info(s: State<AppState>) -> Res<Info> {
    Ok(Info {
        root: with(&s, |w| Ok(w.root.clone()))?,
        exe: exe(),
        mcp_config: mcp_config(),
        version: env!("CARGO_PKG_VERSION"),
        pdf_export: crate::pdf::supported(),
        format: with(&s, |w| Ok(w.format()))?,
        upgraded: with(&s, |w| Ok(w.upgraded.clone()))?,
        updates: self_update(),
        license: crate::about::LICENSE_ID,
    })
}

#[tauri::command]
fn licenses() -> crate::about::Licenses {
    crate::about::licenses()
}

// ---- updates -------------------------------------------------------------------

/// Whether this copy can replace itself: a release build running as an AppImage, from an NSIS
/// install or from a macOS app bundle. A distro package or a source build is updated the way it
/// was installed.
fn self_update() -> bool {
    !cfg!(debug_assertions) && (cfg!(any(windows, target_os = "macos")) || std::env::var_os("APPIMAGE").is_some())
}

/// The update found by the last check, kept so the user installs exactly what they were shown.
#[derive(Default)]
struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    notes: Option<String>,
    date: Option<String>,
}

#[derive(Serialize, Clone)]
struct Progress {
    done: u64,
    total: Option<u64>,
}

async fn find_update(app: &AppHandle) -> Res<Option<Update>> {
    if !self_update() {
        return Err("this copy of ResuFlow was not installed from a release, so it can't update itself".into());
    }
    app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| format!("could not check for updates: {e}"))
}

/// Download, verify the signature, install. On Windows the installer takes over and the app exits;
/// elsewhere the caller restarts into the new version.
async fn install(update: &Update, mut progress: impl FnMut(Progress)) -> Res<()> {
    let mut done = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                done += chunk as u64;
                progress(Progress { done, total });
            },
            || {},
        )
        .await
        .map_err(|e| format!("could not install the update: {e}"))
}

#[tauri::command]
async fn check_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> Res<Option<UpdateInfo>> {
    let update = find_update(&app).await?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        date: u.date.map(|d| d.date().to_string()),
    });
    *pending.0.lock().map_err(|_| "update lock poisoned")? = update;
    Ok(info)
}

/// Install the update the last check found, then restart into it. The UI saves every open edit first.
#[tauri::command]
async fn install_update(app: AppHandle, pending: State<'_, PendingUpdate>, on_progress: Channel<Progress>) -> Res<()> {
    let update = pending.0.lock().map_err(|_| "update lock poisoned")?.take().ok_or("check for updates first")?;
    if let Err(e) = install(&update, |p| drop(on_progress.send(p))).await {
        *pending.0.lock().map_err(|_| "update lock poisoned")? = Some(update); // let the user retry
        return Err(e);
    }
    app.restart();
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
fn archive_resume(s: State<AppState>, slug: String) -> Res<ResumeSummary> {
    with(&s, |w| w.archive_resume(&slug).map(|r| r.summary))
}

#[tauri::command]
fn restore_resume(s: State<AppState>, slug: String) -> Res<ResumeSummary> {
    with(&s, |w| w.restore_resume(&slug).map(|r| r.summary))
}

/// Erases the resume for good. The UI asks first.
#[tauri::command]
fn delete_resume(s: State<AppState>, slug: String) -> Res<()> {
    with(&s, |w| w.delete_resume(&slug))
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

static PRINT_SEQ: AtomicU32 = AtomicU32::new(0);

/// The print window shows plain rendered HTML, so it gets none of the app's handlers: keep the
/// webview's browser menu (Reload, Inspect) and reload/find keys out of it. Copy still works.
const PRINT_WINDOW_GUARD: &str = r#"
addEventListener("contextmenu", (e) => e.preventDefault());
addEventListener("keydown", (e) => {
  const k = e.key.toLowerCase();
  if (["f3", "f5", "f7", "f12"].includes(k) || ((e.ctrlKey || e.metaKey) && ["r", "f", "g", "u", "o"].includes(k))) e.preventDefault();
});
"#;

/// Open the rendered resume in its own window and hand it to the system print dialog,
/// where "Save as PDF" is available on every platform.
#[tauri::command]
async fn print_resume(app: tauri::AppHandle, s: State<'_, AppState>, slug: String) -> Res<()> {
    let title = with(&s, |w| w.read_resume(&slug))?.summary.title;
    let url = tauri::Url::parse(&preview_url(&slug)).map_err(|e| e.to_string())?;
    WebviewWindowBuilder::new(&app, format!("print-{}", PRINT_SEQ.fetch_add(1, Ordering::Relaxed)), WebviewUrl::CustomProtocol(url))
        .title(format!("Print · {title}"))
        .inner_size(860.0, 1040.0)
        .initialization_script(PRINT_WINDOW_GUARD)
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
        let name = crate::render::split_header(&r.body).map(|h| crate::render::plain_text(h.name)).filter(|n| !n.is_empty());
        Ok((r.str("title").unwrap_or_else(|| slug.clone()), name))
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState { ws: Mutex::new(ws) })
        .manage(PendingUpdate::default())
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

pub fn run(dir: Option<PathBuf>) {
    // macOS dev builds embed Info.plist from this macro under the symbol `_EMBED_INFO_PLIST`.
    // The macro may run once per crate; a second call fails to compile, which `cargo test` hits.
    let context = tauri::generate_context!();
    match config::workspace(dir.clone()) {
        Ok(ws) => builder(ws)
            .invoke_handler(tauri::generate_handler![
                info, revision, list_resumes, create_resume, read_parts, write_body, set_fields, set_meta,
                archive_resume, restore_resume, delete_resume, read_posting, write_posting,
                read_instructions, write_instructions, check_resume, list_skills,
                print_resume, save_pdf, licenses, check_update, install_update
            ])
            .run(context)
            .expect("failed to start ResuFlow"),
        Err(e) => cannot_open(context, e.0, workspace::folder_format(&config::resolve(dir)) > workspace::FORMAT),
    }
}

/// The data folder could not be opened. A double-clicked app has no terminal, so say why in a
/// dialog. When a newer ResuFlow wrote the folder, the fix is updating, so offer that.
fn cannot_open(context: tauri::Context<tauri::Wry>, error: String, newer: bool) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    eprintln!("resuflow: {error}");
    let offer_update = newer && self_update();
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
            let handle = app.handle().clone();
            let mut dialog = app.dialog().message(error).title("ResuFlow can't open your data").kind(MessageDialogKind::Error);
            if offer_update {
                dialog = dialog.buttons(MessageDialogButtons::OkCancelCustom("Update ResuFlow".into(), "Quit".into()));
            }
            dialog.show(move |update| {
                if !(update && offer_update) {
                    return handle.exit(1);
                }
                tauri::async_runtime::spawn(async move {
                    let outcome = match find_update(&handle).await {
                        Ok(Some(u)) => install(&u, |_| {}).await,
                        Ok(None) => Err("no newer version of ResuFlow is available yet".into()),
                        Err(e) => Err(e),
                    };
                    match outcome {
                        Ok(()) => handle.restart(),
                        Err(e) => {
                            let exit = handle.clone();
                            handle.dialog().message(e).title("Update failed").kind(MessageDialogKind::Error).show(move |_| exit.exit(1));
                        }
                    }
                });
            });
            Ok(())
        })
        .run(context);
    if let Err(e) = result {
        eprintln!("resuflow: {e}");
    }
    std::process::exit(1);
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
