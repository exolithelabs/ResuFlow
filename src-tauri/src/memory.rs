use crate::workspace::{atomic_write_file, date_stamp, WorkspaceError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryDoc {
    pub markdown: String,
}

pub fn load_memory(root: &Path) -> Result<MemoryDoc, WorkspaceError> {
    let file = memory_file(root);
    match fs::read_to_string(&file) {
        Ok(markdown) => Ok(MemoryDoc { markdown }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(MemoryDoc {
            markdown: starter_memory().to_string(),
        }),
        Err(err) => Err(WorkspaceError::new(err.to_string(), 500)),
    }
}

pub fn save_memory(root: &Path, input: &Value) -> Result<MemoryDoc, WorkspaceError> {
    let markdown = input
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| starter_memory())
        .to_string();
    atomic_write_file(&memory_file(root), &markdown)?;
    Ok(MemoryDoc { markdown })
}

pub fn append_memory(root: &Path, note: &str) -> Result<MemoryDoc, WorkspaceError> {
    let current = load_memory(root)?;
    let stamp = date_stamp();
    let next = format!(
        "{}\n\n## {stamp}\n\n{}\n",
        current.markdown.trim(),
        note.trim()
    );
    atomic_write_file(&memory_file(root), &next)?;
    Ok(MemoryDoc { markdown: next })
}

fn memory_file(root: &Path) -> PathBuf {
    root.join("memory.md")
}

fn starter_memory() -> &'static str {
    "# Memory

Durable facts the resume agent should remember. Prefer verified career facts, preferences, and constraints.

## Preferences

- Tone:
- Roles I am targeting:

## Facts

- 
"
}
