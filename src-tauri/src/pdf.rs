//! Save a resume straight to a PDF file, with real selectable text and no print dialog.
//!
//! The resume loads into a hidden webview (the same `preview://` page the print window shows),
//! and the platform's own print engine writes it to the chosen file. This is implemented
//! for Linux (WebKitGTK); other platforms use the system print dialog's "Save as PDF".

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};

static SEQ: AtomicU32 = AtomicU32::new(0);

pub type Done = Box<dyn FnOnce(Result<PathBuf, String>) + Send + 'static>;

pub fn supported() -> bool {
    cfg!(target_os = "linux")
}

pub fn export(app: &AppHandle, slug: &str, out: PathBuf, done: Done) -> Result<(), String> {
    let url = tauri::Url::parse(&crate::app::preview_url(slug)).map_err(|e| e.to_string())?;
    let done = Mutex::new(Some((out, done)));
    WebviewWindowBuilder::new(app, format!("pdf-{}", SEQ.fetch_add(1, Ordering::Relaxed)), WebviewUrl::CustomProtocol(url))
        .visible(false)
        .inner_size(900.0, 1200.0)
        .on_page_load(move |w, p| {
            if !matches!(p.event(), tauri::webview::PageLoadEvent::Finished) {
                return;
            }
            let Some((out, done)) = done.lock().ok().and_then(|mut g| g.take()) else { return };
            print_to_file(w, out, done);
        })
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn print_to_file(w: tauri::WebviewWindow, out: PathBuf, done: Done) {
    use webkit2gtk::{PrintOperation, PrintOperationExt};

    let win = w.clone();
    let result = w.with_webview(move |wv| {
        let finish = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
        let uri = match tauri::Url::from_file_path(&out) {
            Ok(u) => u.to_string(),
            Err(()) => {
                if let Some(d) = finish.borrow_mut().take() {
                    d(Err(format!("not an absolute path: {}", out.display())));
                }
                let _ = win.destroy();
                return;
            }
        };

        let settings = gtk::PrintSettings::new();
        settings.set_printer("Print to File");
        settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
        settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(&uri));

        // A4 with the same margins as the stylesheet's @page rule (WebKit ignores @page here).
        let setup = gtk::PageSetup::new();
        setup.set_paper_size(&gtk::PaperSize::new(Some(&gtk::PAPER_NAME_A4)));
        setup.set_top_margin(14.0, gtk::Unit::Mm);
        setup.set_bottom_margin(14.0, gtk::Unit::Mm);
        setup.set_left_margin(16.0, gtk::Unit::Mm);
        setup.set_right_margin(16.0, gtk::Unit::Mm);

        let op = PrintOperation::new(&wv.inner());
        op.set_print_settings(&settings);
        op.set_page_setup(&setup);

        let (f1, w1, out1) = (finish.clone(), win.clone(), out.clone());
        op.connect_finished(move |_| {
            if let Some(d) = f1.borrow_mut().take() {
                d(Ok(out1.clone()));
            }
            let _ = w1.destroy();
        });
        let (f2, w2) = (finish, win);
        op.connect_failed(move |_, e| {
            if let Some(d) = f2.borrow_mut().take() {
                d(Err(e.to_string()));
            }
            let _ = w2.destroy();
        });
        op.print();
    });
    if let Err(e) = result {
        eprintln!("resuflow: pdf export failed: {e}");
    }
}

#[cfg(not(target_os = "linux"))]
fn print_to_file(w: tauri::WebviewWindow, _out: PathBuf, done: Done) {
    let _ = w.destroy();
    done(Err("direct PDF export is only available on Linux; use Print and choose Save as PDF".into()));
}
