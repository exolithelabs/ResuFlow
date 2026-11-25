//! Where ResuFlow keeps its data: the platform's app-data folder, chosen automatically.
//!
//! - Linux:   ~/.local/share/resuflow
//! - macOS:   ~/Library/Application Support/resuflow
//! - Windows: %APPDATA%\resuflow
//!
//! `--dir` or `RESUFLOW_DIR` override it (for development and tests only).

use crate::workspace::{Result, Workspace};
use std::fs;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("resuflow")
}

pub fn resolve(explicit: Option<PathBuf>) -> PathBuf {
    explicit
        .or_else(|| std::env::var_os("RESUFLOW_DIR").filter(|v| !v.is_empty()).map(PathBuf::from))
        .unwrap_or_else(data_dir)
}

/// Open the data folder, creating it on first run. On that first run, copies (never moves)
/// resumes, profile and instructions from the previous default location, `~/Documents/ResuFlow`.
pub fn workspace(explicit: Option<PathBuf>) -> Result<Workspace> {
    let dir = resolve(explicit.clone());
    if explicit.is_none() && !dir.exists() {
        if let Some(legacy) = dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents"))).map(|d| d.join("ResuFlow")) {
            if Workspace::is_workspace(&legacy) {
                if let Err(e) = import(&legacy, &dir) {
                    eprintln!("resuflow: could not import {}: {e}", legacy.display());
                }
            }
        }
    }
    Workspace::init(dir)
}

fn import(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to.join("resumes"))?;
    for f in ["profile.md", "instructions.md", "memory.md"] {
        if from.join(f).is_file() {
            fs::copy(from.join(f), to.join(f))?;
        }
    }
    for e in fs::read_dir(from.join("resumes"))?.flatten() {
        let src = e.path().join("resume.md");
        if src.is_file() {
            let dest = to.join("resumes").join(e.file_name());
            fs::create_dir_all(&dest)?;
            fs::copy(src, dest.join("resume.md"))?;
        }
    }
    Ok(())
}
