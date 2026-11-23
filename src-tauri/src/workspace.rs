use crate::markdown::{
    excerpt, parse_frontmatter, serialize_profile_markdown, serialize_resume_markdown,
    starter_markdown, starter_profile_markdown, ProfileInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const SCHEMA_VERSION: i64 = 1;
pub const WORKSPACE_FILE: &str = "resuflow.json";

#[derive(Debug, Clone)]
pub struct WorkspaceError {
    pub message: String,
    pub status: u16,
}

impl WorkspaceError {
    pub fn new(message: impl Into<String>, status: u16) -> Self {
        Self {
            message: message.into(),
            status,
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(message, 400)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(message, 401)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(message, 403)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(message, 404)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(message, 409)
    }
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for WorkspaceError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub headline: String,
    pub email: String,
    pub phone: String,
    pub location: String,
    pub website: String,
    pub linkedin: String,
    pub github: String,
    pub body: String,
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Resume {
    pub slug: String,
    pub title: String,
    pub status: String,
    pub tags: Vec<String>,
    pub updated: String,
    pub body: String,
    pub markdown: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeSummary {
    pub slug: String,
    pub title: String,
    pub status: String,
    pub updated: String,
    pub summary: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub marker: Value,
}

pub fn is_valid_slug(slug: &str) -> bool {
    if slug.is_empty() || slug.starts_with('-') || slug.ends_with('-') || slug.contains("--") {
        return false;
    }
    slug.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn assert_slug(slug: &str, kind: &str) -> Result<(), WorkspaceError> {
    if !is_valid_slug(slug) {
        let display = if slug.is_empty() { "(empty)" } else { slug };
        return Err(WorkspaceError::bad_request(format!(
            "Invalid {kind} slug: {display}"
        )));
    }
    Ok(())
}

pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    trimmed.chars().take(80).collect::<String>().trim_end_matches('-').to_string()
}

pub fn current_date_ymd() -> (i32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    // Howard Hinnant's civil_from_days algorithm
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d)
}

pub fn month_stamp() -> String {
    let (year, month, _) = current_date_ymd();
    format!("{year:04}-{month:02}")
}

pub fn date_stamp() -> String {
    let (year, month, day) = current_date_ymd();
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn iso_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day) = current_date_ymd();
    let rem = secs % 86_400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

pub fn atomic_write_file(file_path: &Path, data: &str) -> Result<(), WorkspaceError> {
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| WorkspaceError::new(format!("Failed to create directory: {e}"), 500))?;
    }
    let file_name = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let pid = std::process::id();
    let temp_name = format!(".{file_name}.{pid}.{}.tmp", Uuid::new_v4());
    let temp_path = file_path.with_file_name(temp_name);

    let write_res = (|| -> std::io::Result<()> {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file: File = opts.open(&temp_path)?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, file_path)?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&temp_path);
        return Err(WorkspaceError::new(
            format!("Failed to write file {}: {err}", file_path.display()),
            500,
        ));
    }
    Ok(())
}

pub fn find_workspace(start_dir: &Path) -> Result<Workspace, WorkspaceError> {
    let dir = resolve_abs_path(start_dir);
    let marker = dir.join(WORKSPACE_FILE);
    let marker_text = match fs::read_to_string(&marker) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(WorkspaceError::bad_request(format!(
                "No resume workspace found in {}. Run `resuflow init` first.",
                dir.display()
            )));
        }
        Err(err) => {
            return Err(WorkspaceError::new(
                format!("Could not read {}: {err}", marker.display()),
                500,
            ));
        }
    };

    let payload: Value = serde_json::from_str(&marker_text)
        .map_err(|_| WorkspaceError::bad_request(format!("Invalid {WORKSPACE_FILE}: expected valid JSON.")))?;
    let migrated = migrate_workspace_marker(&marker, payload)?;
    Ok(Workspace {
        root: dir,
        marker: migrated,
    })
}

pub fn init_workspace(target_dir: &Path) -> Result<Workspace, WorkspaceError> {
    let root = resolve_abs_path(target_dir);
    fs::create_dir_all(root.join("resumes"))
        .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
    fs::create_dir_all(root.join("skills"))
        .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;

    let marker_path = root.join(WORKSPACE_FILE);
    if !marker_path.exists() {
        write_json(
            &marker_path,
            &json!({
                "schemaVersion": SCHEMA_VERSION,
                "created": iso_timestamp(),
            }),
        )?;
    }

    let profile_path = root.join("profile.md");
    if !profile_path.exists() {
        atomic_write_file(&profile_path, &starter_profile_markdown())?;
    }

    let readme_path = root.join("README.md");
    if !readme_path.exists() {
        atomic_write_file(&readme_path, DATA_REPO_README)?;
    }

    let gitignore_path = root.join(".gitignore");
    if !gitignore_path.exists() {
        atomic_write_file(&gitignore_path, "*.log\n.DS_Store\nThumbs.db\nsettings.json\n")?;
    }

    find_workspace(&root)
}

pub fn load_profile(root: &Path) -> Result<Profile, WorkspaceError> {
    let md_file = root.join("profile.md");
    if md_file.exists() {
        let content = fs::read_to_string(&md_file)
            .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
        return Ok(markdown_to_profile(&content));
    }

    let profile = markdown_to_profile(&starter_profile_markdown());
    atomic_write_file(&md_file, &profile.markdown)?;
    Ok(profile)
}

pub fn save_profile(root: &Path, input: &Value) -> Result<Profile, WorkspaceError> {
    let markdown = if let Some(md) = input.get("markdown").and_then(|v| v.as_str()) {
        md.to_string()
    } else {
        serialize_profile_markdown(&ProfileInput {
            name: input.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            headline: input.get("headline").and_then(|v| v.as_str()).unwrap_or(""),
            email: input.get("email").and_then(|v| v.as_str()).unwrap_or(""),
            phone: input.get("phone").and_then(|v| v.as_str()).unwrap_or(""),
            location: input.get("location").and_then(|v| v.as_str()).unwrap_or(""),
            website: input.get("website").and_then(|v| v.as_str()).unwrap_or(""),
            linkedin: input.get("linkedin").and_then(|v| v.as_str()).unwrap_or(""),
            github: input.get("github").and_then(|v| v.as_str()).unwrap_or(""),
            body: input.get("body").and_then(|v| v.as_str()).unwrap_or(""),
        })
    };
    let next = markdown_to_profile(&markdown);
    atomic_write_file(&root.join("profile.md"), &next.markdown)?;
    Ok(next)
}

pub fn list_resumes(root: &Path) -> Result<Vec<ResumeSummary>, WorkspaceError> {
    let resumes_dir = root.join("resumes");
    fs::create_dir_all(&resumes_dir)
        .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
    let entries = fs::read_dir(&resumes_dir)
        .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;

    let mut resumes = Vec::new();
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let slug = entry.file_name().to_string_lossy().to_string();
        if let Ok(resume) = load_resume(root, &slug) {
            resumes.push(summarize_resume(&resume));
        }
    }

    resumes.sort_by(|a, b| {
        b.updated
            .cmp(&a.updated)
            .then_with(|| a.title.cmp(&b.title))
    });
    Ok(resumes)
}

pub fn load_resume(root: &Path, slug: &str) -> Result<Resume, WorkspaceError> {
    assert_slug(slug, "resume")?;
    let md_file = resume_markdown_file(root, slug);
    if md_file.exists() {
        let markdown = fs::read_to_string(&md_file)
            .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
        return Ok(markdown_to_resume(&markdown, slug));
    }
    Err(WorkspaceError::not_found(format!("Resume not found: {slug}")))
}

pub fn create_resume(root: &Path, input: &Value) -> Result<Resume, WorkspaceError> {
    let title = input
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return Err(WorkspaceError::bad_request("A resume title is required."));
    }
    let slug_source = input
        .get("slug")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&title);
    let slug = slugify(slug_source);
    assert_slug(&slug, "resume")?;

    let file = resume_markdown_file(root, &slug);
    if file.exists() {
        return Err(WorkspaceError::conflict(format!(
            "A resume with slug \"{slug}\" already exists."
        )));
    }

    let mut resume = markdown_to_resume(&starter_markdown(&title), &slug);
    resume.updated = month_stamp();
    resume.markdown = serialize_resume_markdown(
        &resume.title,
        &resume.status,
        &resume.tags,
        &resume.updated,
        &resume.body,
    );
    atomic_write_file(&file, &resume.markdown)?;
    Ok(resume)
}

pub fn save_resume(root: &Path, slug: &str, input: &Value) -> Result<Resume, WorkspaceError> {
    assert_slug(slug, "resume")?;
    load_resume(root, slug)?;

    let markdown = input
        .get("markdown")
        .and_then(|v| v.as_str())
        .ok_or_else(|| WorkspaceError::bad_request("Resume Markdown is required."))?;
    let mut next = markdown_to_resume(markdown, slug);
    next.updated = month_stamp();
    next.markdown = serialize_resume_markdown(
        &next.title,
        &next.status,
        &next.tags,
        &next.updated,
        &next.body,
    );
    atomic_write_file(&resume_markdown_file(root, slug), &next.markdown)?;
    Ok(next)
}

pub fn delete_resume(root: &Path, slug: &str) -> Result<(), WorkspaceError> {
    assert_slug(slug, "resume")?;
    let dir = root.join("resumes").join(slug);
    if !dir.exists() {
        return Err(WorkspaceError::not_found(format!("Resume not found: {slug}")));
    }
    fs::remove_dir_all(&dir).map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
    Ok(())
}

fn resume_markdown_file(root: &Path, slug: &str) -> PathBuf {
    root.join("resumes").join(slug).join("resume.md")
}

pub fn markdown_to_profile(markdown: &str) -> Profile {
    let parsed = parse_frontmatter(markdown);
    let get = |k: &str| parsed.meta.get(k).cloned().unwrap_or_default();
    let name = get("name");
    let headline = get("headline");
    let email = get("email");
    let phone = get("phone");
    let location = get("location");
    let website = get("website");
    let linkedin = get("linkedin");
    let github = get("github");
    let body = parsed.body;
    let normalized_md = serialize_profile_markdown(&ProfileInput {
        name: &name,
        headline: &headline,
        email: &email,
        phone: &phone,
        location: &location,
        website: &website,
        linkedin: &linkedin,
        github: &github,
        body: &body,
    });
    Profile {
        name,
        headline,
        email,
        phone,
        location,
        website,
        linkedin,
        github,
        body,
        markdown: normalized_md,
    }
}

pub fn markdown_to_resume(markdown: &str, slug: &str) -> Resume {
    let parsed = parse_frontmatter(markdown);
    let tags: Vec<String> = parsed
        .meta
        .get("tags")
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let raw_status = parsed.meta.get("status").map(|s| s.as_str()).unwrap_or("draft");
    let status = match raw_status {
        "draft" | "active" | "archived" => raw_status.to_string(),
        _ => "draft".to_string(),
    };

    let title = parsed
        .meta
        .get("title")
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| slug.to_string());

    let updated = parsed
        .meta
        .get("updated")
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .unwrap_or_else(month_stamp);

    let body = parsed.body;
    let summary = excerpt(&body);
    let normalized_md = serialize_resume_markdown(&title, &status, &tags, &updated, &body);

    Resume {
        slug: slug.to_string(),
        title,
        status,
        tags,
        updated,
        body,
        markdown: normalized_md,
        summary,
    }
}

fn summarize_resume(resume: &Resume) -> ResumeSummary {
    ResumeSummary {
        slug: resume.slug.clone(),
        title: resume.title.clone(),
        status: resume.status.clone(),
        updated: resume.updated.clone(),
        summary: resume.summary.clone(),
        tags: resume.tags.clone(),
    }
}

fn write_json(file_path: &Path, value: &Value) -> Result<(), WorkspaceError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
    atomic_write_file(file_path, &format!("{text}\n"))
}

fn migrate_workspace_marker(marker_path: &Path, payload: Value) -> Result<Value, WorkspaceError> {
    let Some(obj) = payload.as_object() else {
        return Err(WorkspaceError::bad_request(format!(
            "Invalid {WORKSPACE_FILE}: expected a JSON object."
        )));
    };

    let version = match obj.get("schemaVersion") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().unwrap_or(-1),
    };
    if version < 0 {
        return Err(WorkspaceError::bad_request(format!(
            "Invalid {WORKSPACE_FILE}: schemaVersion must be a non-negative integer."
        )));
    }
    if version > SCHEMA_VERSION {
        return Err(WorkspaceError::conflict(format!(
            "This workspace uses schema version {version}, but this app supports up to {SCHEMA_VERSION}. Update ResuFlow first."
        )));
    }
    if version == SCHEMA_VERSION {
        return Ok(payload);
    }

    let mut migrated = obj.clone();
    migrated.insert("schemaVersion".to_string(), json!(SCHEMA_VERSION));
    migrated.insert("migrated".to_string(), json!(iso_timestamp()));
    let next_val = Value::Object(migrated);
    write_json(marker_path, &next_val)?;
    Ok(next_val)
}

pub fn resolve_abs_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        clean_path(path)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        clean_path(&cwd.join(path))
    }
}

fn clean_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

const DATA_REPO_README: &str = "# Resume workspace

This folder is **your resume data**, not the ResuFlow app.

- `profile.md` — shared name, contact details, and links
- `memory.md` — durable notes for the resume agent
- `skills/<slug>/SKILL.md` — custom agent skills
- `resumes/<slug>/resume.md` — each role-specific resume (Markdown)
- MCP server (while the app is running): `http://127.0.0.1:4173/mcp` with the per-process bearer token shown in the app

Edit everything in the local web UI:

```bash
resuflow
```

This folder is your data, not the app. Git is optional; the app does not initialize a repository.
";
