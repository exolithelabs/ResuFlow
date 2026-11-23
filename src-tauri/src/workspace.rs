//! The workspace: a plain folder of Markdown files the user owns.
//!
//! ```text
//! <workspace>/
//!   resuflow.json          marker file
//!   profile.md             shared identity + contact (front matter) and career notes (body)
//!   memory.md              durable notes agents keep between sessions
//!   resumes/<slug>/resume.md
//!   .trash/                deleted resumes are moved here, never erased
//! ```
//!
//! Skills are not stored here: they ship inside the app (see `skills.rs`).
//!
//! Every mutating call accepts an optional `expected_hash`. If the file changed since
//! the caller read it, the write is refused with a conflict instead of clobbering an
//! edit made by the desktop app or another agent.

use crate::doc::{Doc, Value};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const STATUSES: [&str; 4] = ["draft", "ready", "sent", "archived"];

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

macro_rules! bail {
    ($($t:tt)*) => { return Err(Error(format!($($t)*))) };
}

#[derive(Serialize, Clone, Debug)]
pub struct ResumeSummary {
    pub slug: String,
    pub title: String,
    pub status: String,
    pub tags: Vec<String>,
    pub updated_ms: u64,
    pub words: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct ResumeFile {
    #[serde(flatten)]
    pub summary: ResumeSummary,
    pub content: String,
    pub hash: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct TextFile {
    pub content: String,
    pub hash: String,
}

/// A Markdown file split for editing: front matter fields and body edited separately.
#[derive(Serialize, Clone, Debug)]
pub struct Parts {
    pub fields: serde_json::Map<String, serde_json::Value>,
    pub body: String,
    /// Lines in the file before the body starts (to map check line numbers onto the body).
    pub body_offset: usize,
    pub hash: String,
}

/// A file that can be read and written as `Parts`.
#[derive(Clone, Debug)]
pub enum Target {
    Resume(String),
    Note(Note),
}
#[derive(Clone, Copy, Debug)]
pub enum Note {
    Profile,
    Memory,
}

impl Note {
    fn file(self) -> &'static str {
        match self {
            Note::Profile => "profile.md",
            Note::Memory => "memory.md",
        }
    }
}

pub struct Workspace {
    pub root: PathBuf,
}

const PROFILE_TEMPLATE: &str = "---
name: Your Name
headline: What you do, in one line
email: you@example.com
phone: \"+00 00000 00000\"
location: City, Country
links: [linkedin.com/in/you, github.com/you]
---

# Career notes

The front matter above becomes the header of every resume.

Everything below is private source material for writing resumes: roles, projects,
metrics, wins, tools. It is never printed. Be generous here; resumes pick from it.
";

const MEMORY_TEMPLATE: &str = "# Memory

Durable notes for AI agents working in this workspace: preferences, decisions,
target roles, things to avoid. Agents append here; edit freely.
";

const RESUME_TEMPLATE: &str = "## Summary

Two or three lines aimed at this role.

## Experience

### Role — Company
*Start – End · Location*

- Achievement with a measurable result.

## Skills

- Languages:
- Tools:

## Education

### Degree — Institution
*Year*
";

impl Workspace {
    /// Open an existing folder as a workspace. Missing pieces are created lazily.
    pub fn open(root: impl Into<PathBuf>) -> Result<Workspace> {
        let root = root.into();
        if !root.is_dir() {
            bail!("workspace folder does not exist: {} (run `resuflow init` first)", root.display());
        }
        let root = root.canonicalize()?;
        Ok(Workspace { root })
    }

    /// Create (or complete) a workspace. Never overwrites existing files.
    pub fn init(root: impl Into<PathBuf>) -> Result<Workspace> {
        let root = root.into();
        fs::create_dir_all(root.join("resumes"))?;
        write_if_missing(&root.join("resuflow.json"), "{\n  \"app\": \"resuflow\",\n  \"format\": 2\n}\n")?;
        write_if_missing(&root.join("profile.md"), PROFILE_TEMPLATE)?;
        write_if_missing(&root.join("memory.md"), MEMORY_TEMPLATE)?;
        Workspace::open(root)
    }

    pub fn is_workspace(dir: &Path) -> bool {
        dir.join("resuflow.json").is_file() || dir.join("resumes").is_dir()
    }

    // ---- resumes -------------------------------------------------------------

    fn resume_path(&self, slug: &str) -> Result<PathBuf> {
        check_slug(slug)?;
        Ok(self.root.join("resumes").join(slug).join("resume.md"))
    }

    pub fn list_resumes(&self) -> Result<Vec<ResumeSummary>> {
        let dir = self.root.join("resumes");
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&dir) else { return Ok(out) };
        for e in entries.flatten() {
            let slug = e.file_name().to_string_lossy().to_string();
            if check_slug(&slug).is_err() {
                continue;
            }
            let path = e.path().join("resume.md");
            if let Ok(text) = fs::read_to_string(&path) {
                out.push(summarize(&slug, &text, mtime_ms(&path)));
            }
        }
        out.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms));
        Ok(out)
    }

    pub fn read_resume(&self, slug: &str) -> Result<ResumeFile> {
        let path = self.resume_path(slug)?;
        let content = fs::read_to_string(&path).map_err(|_| Error(format!("no resume named '{slug}'")))?;
        Ok(ResumeFile {
            summary: summarize(slug, &content, mtime_ms(&path)),
            hash: hash(&content),
            content,
        })
    }

    /// Create a resume. `from` copies another resume's content; `content` is used verbatim.
    pub fn create_resume(
        &self,
        title: &str,
        slug: Option<&str>,
        from: Option<&str>,
        content: Option<&str>,
        tags: Vec<String>,
    ) -> Result<ResumeFile> {
        let title = title.trim();
        if title.is_empty() {
            bail!("title is required");
        }
        let slug = match slug {
            Some(s) => s.to_string(),
            None => self.unique_slug(&slugify(title)),
        };
        let path = self.resume_path(&slug)?;
        if path.exists() {
            bail!("a resume named '{slug}' already exists");
        }
        let mut doc = match (content, from) {
            (Some(c), _) => Doc::parse(c),
            (None, Some(src)) => Doc::parse(&self.read_resume(src)?.content),
            (None, None) => Doc { meta: vec![], body: RESUME_TEMPLATE.to_string() },
        };
        doc.set("title", Value::Str(title.to_string()));
        if from.is_some() {
            doc.set("status", Value::Str(String::new())); // a copy starts over as a draft
        }
        if !tags.is_empty() {
            doc.set("tags", Value::List(tags));
        }
        fs::create_dir_all(path.parent().unwrap())?;
        atomic_write(&path, &doc.to_string())?;
        self.read_resume(&slug)
    }

    pub fn write_resume(&self, slug: &str, content: &str, expected_hash: Option<&str>) -> Result<ResumeFile> {
        let path = self.resume_path(slug)?;
        if !path.exists() {
            bail!("no resume named '{slug}' (use create_resume)");
        }
        guard(&path, expected_hash)?;
        atomic_write(&path, content)?;
        self.read_resume(slug)
    }

    /// Replace exactly one occurrence of `old` with `new` in the resume source.
    pub fn edit_resume(&self, slug: &str, old: &str, new: &str, expected_hash: Option<&str>) -> Result<ResumeFile> {
        let current = self.read_resume(slug)?;
        if let Some(h) = expected_hash {
            if h != current.hash {
                bail!("conflict: resume '{slug}' changed since it was read (re-read and retry)");
            }
        }
        if old.is_empty() {
            bail!("old_text must not be empty");
        }
        match current.content.matches(old).count() {
            0 => bail!("old_text not found in resume '{slug}'"),
            1 => {}
            n => bail!("old_text appears {n} times; include more surrounding text to make it unique"),
        }
        let updated = current.content.replacen(old, new, 1);
        self.write_resume(slug, &updated, Some(&current.hash))
    }

    pub fn set_meta(
        &self,
        slug: &str,
        title: Option<&str>,
        status: Option<&str>,
        tags: Option<Vec<String>>,
    ) -> Result<ResumeFile> {
        let current = self.read_resume(slug)?;
        let mut doc = Doc::parse(&current.content);
        if let Some(t) = title {
            if t.trim().is_empty() {
                bail!("title must not be empty");
            }
            doc.set("title", Value::Str(t.trim().into()));
        }
        if let Some(s) = status {
            if !STATUSES.contains(&s) {
                bail!("status must be one of: {}", STATUSES.join(", "));
            }
            doc.set("status", Value::Str(s.into()));
        }
        if let Some(t) = tags {
            let t = t.into_iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
            doc.set("tags", Value::List(t));
        }
        self.write_resume(slug, &doc.to_string(), Some(&current.hash))
    }

    /// Move a resume to `.trash/`. Nothing is ever erased.
    pub fn delete_resume(&self, slug: &str) -> Result<String> {
        let path = self.resume_path(slug)?;
        let dir = path.parent().unwrap();
        if !dir.exists() {
            bail!("no resume named '{slug}'");
        }
        let trash = self.root.join(".trash");
        fs::create_dir_all(&trash)?;
        let dest = trash.join(format!("{slug}-{}", now_ms()));
        fs::rename(dir, &dest)?;
        Ok(dest.strip_prefix(&self.root).unwrap_or(&dest).display().to_string())
    }

    fn unique_slug(&self, base: &str) -> String {
        let base = if base.is_empty() { "resume" } else { base };
        let mut slug = base.to_string();
        let mut n = 2;
        while self.root.join("resumes").join(&slug).exists() {
            slug = format!("{base}-{n}");
            n += 1;
        }
        slug
    }

    // ---- profile & memory ----------------------------------------------------

    pub fn read_note(&self, note: Note) -> Result<TextFile> {
        let content = fs::read_to_string(self.root.join(note.file())).unwrap_or_default();
        Ok(TextFile { hash: hash(&content), content })
    }

    pub fn write_note(&self, note: Note, content: &str, expected_hash: Option<&str>) -> Result<TextFile> {
        let path = self.root.join(note.file());
        guard(&path, expected_hash)?;
        atomic_write(&path, content)?;
        self.read_note(note)
    }

    pub fn append_memory(&self, text: &str) -> Result<TextFile> {
        let mut content = self.read_note(Note::Memory)?.content;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push('\n');
        content.push_str(text.trim());
        content.push('\n');
        atomic_write(&self.root.join(Note::Memory.file()), &content)?;
        self.read_note(Note::Memory)
    }

    pub fn profile(&self) -> Doc {
        Doc::parse(&self.read_note(Note::Profile).map(|f| f.content).unwrap_or_default())
    }

    // ---- parts (front matter + body edited separately) -----------------------

    fn target_path(&self, t: &Target) -> Result<PathBuf> {
        match t {
            Target::Resume(slug) => self.resume_path(slug),
            Target::Note(n) => Ok(self.root.join(n.file())),
        }
    }

    pub fn read_parts(&self, t: &Target) -> Result<Parts> {
        let path = self.target_path(t)?;
        let content = match t {
            Target::Resume(slug) => fs::read_to_string(&path).map_err(|_| Error(format!("no resume named '{slug}'")))?,
            Target::Note(_) => fs::read_to_string(&path).unwrap_or_default(),
        };
        let doc = Doc::parse(&content);
        let mut fields = serde_json::Map::new();
        for (k, v) in &doc.meta {
            let v = match v {
                Value::Str(s) => serde_json::Value::String(s.clone()),
                Value::List(l) => serde_json::json!(l),
            };
            fields.insert(k.clone(), v);
        }
        Ok(Parts {
            body_offset: content.lines().count().saturating_sub(doc.body.lines().count()),
            hash: hash(&content),
            fields,
            body: doc.body,
        })
    }

    /// Replace the body, keeping the front matter as it is on disk.
    pub fn write_body(&self, t: &Target, body: &str, expected_hash: Option<&str>) -> Result<Parts> {
        let path = self.target_path(t)?;
        if matches!(t, Target::Resume(_)) && !path.exists() {
            bail!("resume does not exist");
        }
        guard(&path, expected_hash)?;
        let mut doc = Doc::parse(&fs::read_to_string(&path).unwrap_or_default());
        doc.body = body.to_string();
        atomic_write(&path, &doc.to_string())?;
        self.read_parts(t)
    }

    /// Set (or, with an empty value, remove) front matter fields, keeping the body.
    pub fn set_fields(
        &self,
        t: &Target,
        fields: &serde_json::Map<String, serde_json::Value>,
        expected_hash: Option<&str>,
    ) -> Result<Parts> {
        let path = self.target_path(t)?;
        guard(&path, expected_hash)?;
        let mut doc = Doc::parse(&fs::read_to_string(&path).unwrap_or_default());
        for (k, v) in fields {
            if !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') || k.is_empty() {
                bail!("invalid field name '{k}'");
            }
            let value = match v {
                serde_json::Value::Array(a) => {
                    Value::List(a.iter().filter_map(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
                }
                serde_json::Value::String(s) => Value::Str(s.trim().to_string()),
                serde_json::Value::Null => Value::Str(String::new()),
                other => Value::Str(other.to_string()),
            };
            doc.set(k, value);
        }
        atomic_write(&path, &doc.to_string())?;
        self.read_parts(t)
    }

    // ---- change detection ----------------------------------------------------

    /// A cheap fingerprint of every file the app shows. Changes when anything is edited
    /// on disk, by an agent or by hand, so the UI can refresh.
    pub fn revision(&self) -> String {
        let mut h = Fnv::new();
        let mut visit = |p: &Path| {
            if let Ok(m) = fs::metadata(p) {
                h.feed(p.to_string_lossy().as_bytes());
                h.feed(&m.len().to_le_bytes());
                h.feed(&mtime_ms(p).to_le_bytes());
            }
        };
        visit(&self.root.join("profile.md"));
        visit(&self.root.join("memory.md"));
        if let Ok(entries) = fs::read_dir(self.root.join("resumes")) {
            let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                visit(&p.join("resume.md"));
            }
        }
        format!("{:016x}", h.0)
    }
}

// ---- helpers -----------------------------------------------------------------

fn summarize(slug: &str, text: &str, updated_ms: u64) -> ResumeSummary {
    let d = Doc::parse(text);
    let title = d
        .str("title")
        .or_else(|| d.body.lines().find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string())))
        .unwrap_or_else(|| slug.to_string());
    ResumeSummary {
        slug: slug.to_string(),
        title,
        status: d.str("status").unwrap_or_else(|| "draft".into()),
        tags: d.list("tags"),
        updated_ms,
        words: d.body.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count(),
    }
}

pub fn check_slug(s: &str) -> Result<()> {
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-');
    if ok {
        Ok(())
    } else {
        bail!("invalid name '{s}': use lowercase letters, digits and dashes (max 64)")
    }
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

fn guard(path: &Path, expected_hash: Option<&str>) -> Result<()> {
    if let Some(expected) = expected_hash {
        let current = fs::read_to_string(path).unwrap_or_default();
        if hash(&current) != expected {
            bail!(
                "conflict: {} changed since it was read (re-read and retry)",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn write_if_missing(path: &Path, content: &str) -> Result<()> {
    if !path.exists() {
        fs::write(path, content)?;
    }
    Ok(())
}

fn mtime_ms(p: &Path) -> u64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

struct Fnv(u64);
impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf29ce484222325)
    }
    fn feed(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}

pub fn hash(s: &str) -> String {
    let mut h = Fnv::new();
    h.feed(s.as_bytes());
    format!("{:016x}", h.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ws() -> Workspace {
        let dir = std::env::temp_dir().join(format!("resuflow-test-{}-{}", std::process::id(), now_ms()));
        Workspace::init(&dir).unwrap()
    }

    #[test]
    fn create_edit_conflict_delete() {
        let ws = temp_ws();
        let r = ws.create_resume("Staff Engineer, Platform", None, None, None, vec!["rust".into()]).unwrap();
        assert_eq!(r.summary.slug, "staff-engineer-platform");
        assert_eq!(r.summary.tags, vec!["rust"]);
        assert!(!r.content.contains("status:"));
        assert_eq!(r.summary.status, "draft");

        let r2 = ws.edit_resume(&r.summary.slug, "## Summary", "## Profile", Some(&r.hash)).unwrap();
        assert!(r2.content.contains("## Profile"));
        // stale hash is refused
        assert!(ws.edit_resume(&r.summary.slug, "## Profile", "## X", Some(&r.hash)).is_err());

        let r3 = ws.set_meta(&r.summary.slug, None, Some("ready"), None).unwrap();
        assert_eq!(r3.summary.status, "ready");
        assert!(ws.set_meta(&r.summary.slug, None, Some("bogus"), None).is_err());

        let copy = ws.create_resume("Staff Engineer, Platform", None, Some(&r.summary.slug), None, vec![]).unwrap();
        assert_eq!(copy.summary.slug, "staff-engineer-platform-2");
        assert_eq!(copy.summary.status, "draft");

        let before = ws.revision();
        ws.delete_resume(&copy.summary.slug).unwrap();
        assert_ne!(before, ws.revision());
        assert_eq!(ws.list_resumes().unwrap().len(), 1);
        let t = Target::Note(Note::Profile);
        let p = ws.read_parts(&t).unwrap();
        let p2 = ws.write_body(&t, "New notes\n", Some(&p.hash)).unwrap();
        assert_eq!(p2.body, "New notes\n");
        assert_eq!(p2.fields["name"], "Your Name");
        let mut f = serde_json::Map::new();
        f.insert("name".into(), "Ada".into());
        f.insert("links".into(), serde_json::json!(["a.dev", ""]));
        f.insert("phone".into(), serde_json::Value::Null);
        let p3 = ws.set_fields(&t, &f, Some(&p2.hash)).unwrap();
        assert_eq!(p3.fields["name"], "Ada");
        assert!(p3.fields.get("phone").is_none());
        assert_eq!(p3.body, "New notes\n");
        assert_eq!(p3.body_offset, p3.fields.len() + 3);
        fs::remove_dir_all(&ws.root).ok();
    }

    #[test]
    fn rejects_traversal() {
        assert!(check_slug("../etc").is_err());
        assert!(check_slug("a/b").is_err());
        assert!(check_slug("Good").is_err());
        assert!(check_slug("good-1").is_ok());
    }
}
