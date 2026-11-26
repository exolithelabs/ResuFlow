//! The workspace: a plain folder of Markdown files the user owns.
//!
//! ```text
//! <workspace>/
//!   resuflow.json              marker file
//!   profile.mdx                default template: copied into each new resume
//!   instructions.json          rules the user writes; agents read them and never change them
//!   resumes/<slug>/resume.mdx  one folder per resume: Markdown plus styling components
//!   resumes/<slug>/posting.txt optional: the job posting this resume targets
//!   .backups/                  copies of the folder taken before each format upgrade
//! ```
//!
//! Archiving a resume only sets its status to `archived`; deleting one erases its folder.
//!
//! Upgrading the folder to a newer format (see [`Workspace::upgrade`]) first copies it to
//! `.backups/format-<old>-<ms>/`, and an upgrade that fails or is cut short is rolled back from
//! that copy. A folder in a newer format than this build knows is never opened, so going back to
//! an older ResuFlow cannot rewrite files it does not understand.
//!
//! Skills are not stored here: they ship inside the app (see `skills.rs`).
//!
//! Every write goes through [`atomic_write`] and takes an optional `expected_hash`. If the file
//! changed since the caller read it, the write is refused with a `conflict:` error instead of
//! clobbering an edit made by the desktop app or an agent.

use crate::doc::{Doc, Value};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const STATUSES: [&str; 4] = ["draft", "ready", "sent", "archived"];

/// Front matter fields that held the resume header before format 3 (see [`migrate_header`]).
const LEGACY_HEADER_FIELDS: [&str; 6] = ["name", "headline", "email", "phone", "location", "links"];

/// Written to `resuflow.json`. Format 3 keeps the header and section rules in the Markdown body;
/// format 4 names resumes and the template `.mdx` (Markdown plus the components in `render.rs`);
/// format 5 stores instructions as JSON and postings as plain text.
const MARKER: &str = "{\n  \"app\": \"resuflow\",\n  \"format\": 5\n}\n";
/// The format in [`MARKER`]: the newest this build reads, and the one it writes.
pub const FORMAT: u64 = 5;

/// Copies taken before each upgrade, at the workspace root.
pub const BACKUPS_DIR: &str = ".backups";
/// Inside [`BACKUPS_DIR`]: held locked while an upgrade runs, so two processes never upgrade at once.
const LOCK_FILE: &str = "lock";
/// Inside [`BACKUPS_DIR`]: names the backup of an upgrade in progress. Left behind only if it was cut short.
const JOURNAL_FILE: &str = "in-progress";
/// Inside a backup: written last, once every file has been copied and flushed.
const BACKUP_DONE: &str = ".complete";
/// Upgrade backups kept; older ones are removed after a successful upgrade.
const BACKUPS_KEPT: usize = 5;

/// A resume's source file inside `resumes/<slug>/`.
pub const RESUME_FILE: &str = "resume.mdx";
/// The default template, at the workspace root.
pub const PROFILE_FILE: &str = "profile.mdx";
/// The user's instructions, at the workspace root.
pub const INSTRUCTIONS_FILE: &str = "instructions.json";
/// A resume's target job posting inside `resumes/<slug>/`.
pub const POSTING_FILE: &str = "posting.txt";

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
    pub has_posting: bool,
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

/// A Markdown file split for editing: front matter fields and body are edited separately.
#[derive(Serialize, Clone, Debug)]
pub struct Parts {
    pub fields: serde_json::Map<String, serde_json::Value>,
    pub body: String,
    /// Lines before the body starts, to map check line numbers onto the body.
    pub body_offset: usize,
    pub hash: String,
}

/// Instructions as the app edits them: one string per rule.
#[derive(Serialize, Clone, Debug)]
pub struct InstructionFile {
    pub items: Vec<String>,
    pub hash: String,
}

/// A file that can be read and written as [`Parts`].
#[derive(Clone, Debug)]
pub enum Target {
    Resume(String),
    Profile,
}

/// What a new resume starts from.
#[derive(Default, Debug)]
pub struct NewResume<'a> {
    pub title: &'a str,
    pub slug: Option<&'a str>,
    /// Copy this resume's content (the usual way to tailor a variant).
    pub from: Option<&'a str>,
    /// Use this Markdown verbatim (front matter allowed).
    pub content: Option<&'a str>,
    pub tags: Vec<String>,
}

pub struct Workspace {
    pub root: PathBuf,
    /// Set when opening this workspace upgraded it from an older format.
    pub upgraded: Option<Upgrade>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Upgrade {
    pub from: u64,
    pub to: u64,
    /// The copy of the folder as it was before the upgrade.
    pub backup: PathBuf,
}

/// Body copied into a new resume when the template body is empty. Also the body of a fresh profile.mdx.
/// The header is the `# ` name line and the lines under it (see `render::split_header`). Its look
/// is written out as `<Text>` styling, the same that "Apply resume style" in the editor adds
/// (`livemd.ts` RESUME_STYLE), so nothing about it is hidden.
pub const RESUME_TEMPLATE: &str = r##"# Your Name
<Text color="#555b66" size="11.5pt">What you do, in one line</Text>
<Text color="#555b66" size="9.5pt">City, Country · you@example.com · +00 00000 00000 · linkedin.com/in/you · github.com/you</Text>

## <Text color="#1f4e8c" size="10pt" caps spacing="0.08em">Summary</Text>
---

Two or three lines aimed at this role.

## <Text color="#1f4e8c" size="10pt" caps spacing="0.08em">Experience</Text>
---

### Role — Company
<Text color="#555b66" size="9.5pt">*Start – End · Location*</Text>

- Achievement with a measurable result.

## <Text color="#1f4e8c" size="10pt" caps spacing="0.08em">Skills</Text>
---

- Languages:
- Tools:

## <Text color="#1f4e8c" size="10pt" caps spacing="0.08em">Education</Text>
---

### Degree — Institution
<Text color="#555b66" size="9.5pt">*Year*</Text>
"##;

/// The stock memory.md of format 1, recognised so migrating it does not keep the boilerplate as a rule.
const LEGACY_MEMORY: &str = "# Memory

Durable notes for AI agents working in this workspace: preferences, decisions,
target roles, things to avoid. Agents append here; edit freely.
";

impl Workspace {
    /// Open an existing folder as a workspace.
    pub fn open(root: impl Into<PathBuf>) -> Result<Workspace> {
        let root = root.into();
        if !root.is_dir() {
            bail!("workspace folder does not exist: {}", root.display());
        }
        let mut ws = Workspace { root: root.canonicalize()?, upgraded: None };
        ws.upgraded = ws.upgrade()?;
        Ok(ws)
    }

    /// Create a workspace, or fill in what is missing. Never overwrites existing files.
    pub fn init(root: impl Into<PathBuf>) -> Result<Workspace> {
        let root = root.into();
        fs::create_dir_all(root.join("resumes"))?;
        // Files without a marker (an import from an older version) are migrated by `open`;
        // only an empty folder starts out in the current format.
        let empty = !root.join("profile.md").exists() && fs::read_dir(root.join("resumes"))?.next().is_none();
        create_new(&root.join("resuflow.json"), if empty { MARKER } else { "{\"app\": \"resuflow\"}\n" })?;
        let ws = Workspace::open(root)?;
        create_new(&ws.profile_path(), RESUME_TEMPLATE)?;
        ws.ensure_instructions()?;
        Ok(ws)
    }

    /// The format recorded in `resuflow.json`; see [`folder_format`].
    pub fn format(&self) -> u64 {
        folder_format(&self.root)
    }

    /// Refuse to touch a folder a newer ResuFlow has upgraded. Checked on open and before every
    /// write, so a process started before an update (an agent's MCP server, say) stops writing
    /// the old format once the new version has upgraded the folder.
    fn check_format(&self) -> Result<()> {
        let format = self.format();
        if format > FORMAT {
            bail!(
                "this data folder was upgraded by a newer version of ResuFlow (data format {format}; this version reads up to {FORMAT}). \
                 Update ResuFlow to open it. Nothing was changed."
            );
        }
        Ok(())
    }

    /// Bring an older folder up to the current format, safely:
    ///
    /// 1. Take an exclusive lock, so the app and an agent's MCP server never upgrade at once.
    ///    The OS releases it if the process dies.
    /// 2. Roll back an upgrade an earlier process started but never finished (see [`Self::recover`]).
    /// 3. Copy the folder to `.backups/`, recording the copy in a journal before touching anything.
    /// 4. Migrate. If that fails, put the copy back and report the error.
    ///
    /// A folder already in the current format costs one small read and takes no lock.
    fn upgrade(&self) -> Result<Option<Upgrade>> {
        self.check_format()?;
        let backups = self.root.join(BACKUPS_DIR);
        let pending = |ws: &Self| ws.format() < FORMAT || backups.join(JOURNAL_FILE).exists() || !ws.old_trash().is_empty();
        if !pending(self) {
            return Ok(None);
        }
        fs::create_dir_all(&backups)?;
        let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(backups.join(LOCK_FILE))?;
        lock.lock()?;
        // Another process may have upgraded the folder while this one waited for the lock.
        self.recover()?;
        self.check_format()?;
        if !pending(self) {
            return Ok(None);
        }
        let from = self.format();
        let backup = self.backup(from)?;
        if let Err(e) = self.migrate().and_then(|()| self.migrate_trash()) {
            return Err(match self.restore_backup(&backup) {
                Ok(()) => {
                    let _ = fs::remove_file(backups.join(JOURNAL_FILE));
                    Error(format!("could not upgrade the data folder from format {from}: {e}. It was left as it was; a copy is in {}", backup.display()))
                }
                // Keep the journal: the next start retries the rollback.
                Err(r) => Error(format!("could not upgrade the data folder ({e}) or put it back ({r}); the original files are in {}", backup.display())),
            });
        }
        fs::remove_file(backups.join(JOURNAL_FILE))?;
        self.prune_backups();
        Ok(Some(Upgrade { from, to: FORMAT, backup }))
    }

    /// Finish what an interrupted upgrade left behind. Its backup is complete only once every file
    /// was copied, and the folder is only touched after that: a complete backup is put back, an
    /// incomplete one is discarded (the folder was never changed). Call with the lock held.
    fn recover(&self) -> Result<()> {
        let journal = self.root.join(BACKUPS_DIR).join(JOURNAL_FILE);
        let Ok(name) = fs::read_to_string(&journal) else { return Ok(()) };
        let name = name.trim();
        if check_slug(name).is_ok() {
            let backup = self.root.join(BACKUPS_DIR).join(name);
            if backup.join(BACKUP_DONE).is_file() {
                eprintln!("resuflow: an earlier upgrade did not finish; restoring {}", backup.display());
                self.restore_backup(&backup)?;
            } else if backup.is_dir() {
                fs::remove_dir_all(&backup)?;
            }
        }
        fs::remove_file(&journal)?;
        Ok(())
    }

    /// Copy everything but `.backups/` itself to `.backups/format-<from>-<ms>/`.
    fn backup(&self, from: u64) -> Result<PathBuf> {
        let backups = self.root.join(BACKUPS_DIR);
        let name = format!("format-{from}-{}", now_ms());
        fs::create_dir_all(&backups)?;
        atomic_write(&backups.join(JOURNAL_FILE), &name)?;
        let to = backups.join(&name);
        copy_tree(&self.root, &to)?;
        atomic_write(&to.join(BACKUP_DONE), "")?;
        Ok(to)
    }

    /// Make the folder exactly what `backup` holds, leaving `.backups/` alone.
    fn restore_backup(&self, backup: &Path) -> Result<()> {
        for entry in fs::read_dir(&self.root)?.flatten() {
            if entry.file_name() == BACKUPS_DIR {
                continue;
            }
            let path = entry.path();
            if path.is_dir() { fs::remove_dir_all(&path)? } else { fs::remove_file(&path)? }
        }
        copy_tree(backup, &self.root)?;
        let _ = fs::remove_file(self.root.join(BACKUP_DONE));
        Ok(())
    }

    /// Keep the newest [`BACKUPS_KEPT`] upgrade backups.
    fn prune_backups(&self) {
        let Ok(entries) = fs::read_dir(self.root.join(BACKUPS_DIR)) else { return };
        let mut found: Vec<(u64, PathBuf)> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join(BACKUP_DONE).is_file())
            .filter_map(|p| Some((p.file_name()?.to_str()?.rsplit_once('-')?.1.parse().ok()?, p)))
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, old) in found.into_iter().skip(BACKUPS_KEPT) {
            let _ = fs::remove_dir_all(old);
        }
    }

    /// Bring an older workspace up to date, once. Called by [`Self::upgrade`]. Format 5 turns `instructions.md` into
    /// `instructions.json` and renames `posting.md` to `posting.txt`; format 4 renames `.md`
    /// sources to `.mdx`; format 3 writes the header into the body and a `---` under each section heading.
    /// Resumes still in an old `.trash/` are converted too, before [`Self::migrate_trash`] moves them.
    fn migrate(&self) -> Result<()> {
        let marker = self.root.join("resuflow.json");
        let format = self.format();
        if format >= FORMAT {
            return Ok(());
        }
        let mut dirs: Vec<PathBuf> = self.slugs().iter().map(|s| self.root.join("resumes").join(s)).collect();
        if let Ok(entries) = fs::read_dir(self.root.join(".trash")) {
            dirs.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
        // Formats 4 and 5: the same text under its new name. A file already there wins.
        let renames = dirs
            .iter()
            .flat_map(|d| [(d.as_path(), "resume.md", RESUME_FILE), (d.as_path(), "posting.md", POSTING_FILE)])
            .chain([(self.root.as_path(), "profile.md", PROFILE_FILE)]);
        for (dir, old, new) in renames {
            if dir.join(old).is_file() && !dir.join(new).exists() {
                fs::rename(dir.join(old), dir.join(new))?;
            }
        }
        // Format 5: the rules from instructions.md, one per `---`-separated block.
        let legacy = self.root.join("instructions.md");
        if legacy.is_file() {
            if !self.instructions_path().exists() {
                atomic_write(&self.instructions_path(), &encode_instructions(&decode_markdown_instructions(&fs::read_to_string(&legacy)?)))?;
            }
            fs::remove_file(&legacy)?;
        }
        if format < 3 {
            let profile = self.profile();
            for path in dirs.iter().map(|d| d.join(RESUME_FILE)).filter(|p| p.is_file()) {
                migrate_file(&path, Some(&profile))?;
            }
            if self.profile_path().is_file() {
                migrate_file(&self.profile_path(), None)?;
            }
        }
        atomic_write(&marker, MARKER)
    }

    /// Older versions moved deleted resumes to `.trash/<slug>-<ms>/`. They come back as archived
    /// resumes, so nothing the user deleted back then is lost.
    fn migrate_trash(&self) -> Result<()> {
        for dir in self.old_trash() {
            let id = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let base = id.rsplit_once('-').map_or(id.as_str(), |(s, _)| s);
            let slug = self.unique_slug(if check_slug(base).is_ok() { base } else { "resume" });
            fs::create_dir_all(self.root.join("resumes"))?;
            let to = self.resume_dir(&slug)?;
            fs::rename(&dir, &to)?;
            let mut doc = Doc::parse(&fs::read_to_string(to.join(RESUME_FILE))?);
            doc.set("status", Value::Str("archived".into()));
            atomic_write(&to.join(RESUME_FILE), &doc.to_string())?;
        }
        let _ = fs::remove_dir(self.root.join(".trash")); // only if empty
        Ok(())
    }

    /// Resumes in an old `.trash/` that [`Self::migrate_trash`] still has to bring back.
    fn old_trash(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.root.join(".trash")) else { return vec![] };
        entries.flatten().map(|e| e.path()).filter(|p| p.join(RESUME_FILE).is_file()).collect()
    }

    /// Every write checks here first: the folder is still in a format this build writes, and
    /// the file still matches the hash the caller read.
    fn guard(&self, path: &Path, expected_hash: Option<&str>) -> Result<()> {
        self.check_format()?;
        guard(path, expected_hash)
    }

    pub fn is_workspace(dir: &Path) -> bool {
        dir.join("resuflow.json").is_file() || dir.join("resumes").is_dir()
    }

    // ---- resumes -------------------------------------------------------------

    fn resume_dir(&self, slug: &str) -> Result<PathBuf> {
        check_slug(slug)?;
        Ok(self.root.join("resumes").join(slug))
    }

    fn resume_path(&self, slug: &str) -> Result<PathBuf> {
        Ok(self.resume_dir(slug)?.join(RESUME_FILE))
    }

    fn existing_resume(&self, slug: &str) -> Result<PathBuf> {
        let path = self.resume_path(slug)?;
        if !path.is_file() {
            bail!("no resume named '{slug}'");
        }
        Ok(path)
    }

    /// All resumes, most recently edited first.
    pub fn list_resumes(&self) -> Result<Vec<ResumeSummary>> {
        let mut out = Vec::new();
        for slug in self.slugs() {
            let path = self.root.join("resumes").join(&slug).join(RESUME_FILE);
            if let Ok(text) = fs::read_to_string(&path) {
                out.push(self.summarize(&slug, &text, &path));
            }
        }
        out.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then_with(|| a.slug.cmp(&b.slug)));
        Ok(out)
    }

    fn slugs(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.root.join("resumes")) else { return vec![] };
        let mut slugs: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|s| check_slug(s).is_ok())
            .collect();
        slugs.sort();
        slugs
    }

    pub fn read_resume(&self, slug: &str) -> Result<ResumeFile> {
        let path = self.existing_resume(slug)?;
        let content = fs::read_to_string(&path)?;
        Ok(ResumeFile { summary: self.summarize(slug, &content, &path), hash: hash(&content), content })
    }

    pub fn create_resume(&self, new: NewResume) -> Result<ResumeFile> {
        let title = new.title.trim();
        if title.is_empty() {
            bail!("title is required");
        }
        let slug = match new.slug {
            Some(s) => s.to_string(),
            None => self.unique_slug(&slugify(title)),
        };
        let path = self.resume_path(&slug)?;
        self.check_format()?;
        if path.parent().is_some_and(Path::exists) {
            bail!("a resume named '{slug}' already exists");
        }
        // A copy may be as long as what it copies; new content must fit one page.
        let (mut doc, copied) = match (new.content, new.from) {
            (Some(c), _) => (Doc::parse(c), None),
            (None, Some(src)) => {
                let src = Doc::parse(&self.read_resume(src)?.content);
                (src.clone(), Some(src))
            }
            (None, None) => {
                let template = Doc { meta: vec![], body: self.template_body() };
                (template.clone(), Some(template))
            }
        };
        check_page(&format!("resume '{slug}'"), &doc, copied.as_ref())?;
        doc.set("title", Value::Str(title.into()));
        if new.from.is_some() {
            doc.set("status", Value::Str(String::new())); // a copy starts over as a draft
        }
        if !new.tags.is_empty() {
            doc.set("tags", Value::List(new.tags));
        }
        fs::create_dir_all(path.parent().unwrap())?;
        atomic_write(&path, &doc.to_string())?;
        // The posting travels with a copy: a variant usually targets the same job.
        if let Some(src) = new.from {
            let posting = self.read_posting(src)?;
            if !posting.content.is_empty() {
                atomic_write(&self.resume_dir(&slug)?.join(POSTING_FILE), &posting.content)?;
            }
        }
        self.read_resume(&slug)
    }

    pub fn write_resume(&self, slug: &str, content: &str, expected_hash: Option<&str>) -> Result<ResumeFile> {
        let path = self.resume_path(slug)?;
        if !path.is_file() {
            bail!("no resume named '{slug}' (use create_resume)");
        }
        self.guard(&path, expected_hash)?;
        let before = Doc::parse(&fs::read_to_string(&path)?);
        check_page(&format!("resume '{slug}'"), &Doc::parse(content), Some(&before))?;
        atomic_write(&path, content)?;
        self.read_resume(slug)
    }

    /// Replace exactly one occurrence of `old` with `new` in the resume source.
    pub fn edit_resume(&self, slug: &str, old: &str, new: &str, expected_hash: Option<&str>) -> Result<ResumeFile> {
        let current = self.read_resume(slug)?;
        if expected_hash.is_some_and(|h| h != current.hash) {
            bail!("conflict: resume '{slug}' changed since it was read (re-read and retry)");
        }
        if old.is_empty() {
            bail!("old_text must not be empty");
        }
        match current.content.matches(old).count() {
            0 => bail!("old_text not found in resume '{slug}'"),
            1 => {}
            n => bail!("old_text appears {n} times; include more surrounding text to make it unique"),
        }
        self.write_resume(slug, &current.content.replacen(old, new, 1), Some(&current.hash))
    }

    pub fn set_meta(
        &self,
        slug: &str,
        title: Option<&str>,
        status: Option<&str>,
        tags: Option<Vec<String>>,
        expected_hash: Option<&str>,
    ) -> Result<ResumeFile> {
        let current = self.read_resume(slug)?;
        let mut doc = Doc::parse(&current.content);
        if let Some(t) = title {
            if t.trim().is_empty() {
                bail!("title must not be empty");
            }
            doc.set("title", Value::Str(t.into()));
        }
        if let Some(s) = status {
            if !STATUSES.contains(&s) {
                bail!("status must be one of: {}", STATUSES.join(", "));
            }
            doc.set("status", Value::Str(s.into()));
        }
        if let Some(t) = tags {
            let mut clean: Vec<String> = Vec::new();
            for tag in t.iter().map(|x| x.trim()).filter(|x| !x.is_empty()) {
                if !clean.iter().any(|c| c.eq_ignore_ascii_case(tag)) {
                    clean.push(tag.to_string());
                }
            }
            doc.set("tags", Value::List(clean));
        }
        self.write_resume(slug, &doc.to_string(), Some(expected_hash.unwrap_or(&current.hash)))
    }

    /// Archive a resume: it leaves the list of active resumes but keeps every file.
    pub fn archive_resume(&self, slug: &str) -> Result<ResumeFile> {
        self.set_meta(slug, None, Some("archived"), None, None)
    }

    /// Bring an archived resume back as a draft.
    pub fn restore_resume(&self, slug: &str) -> Result<ResumeFile> {
        if self.read_resume(slug)?.summary.status != "archived" {
            bail!("resume '{slug}' is not archived");
        }
        self.set_meta(slug, None, Some("draft"), None, None)
    }

    /// Erase a resume and its posting for good. Only the app offers this, after asking;
    /// agents can archive but never delete.
    pub fn delete_resume(&self, slug: &str) -> Result<()> {
        let dir = self.resume_dir(slug)?;
        if !dir.join(RESUME_FILE).is_file() {
            bail!("no resume named '{slug}'");
        }
        self.check_format()?;
        fs::remove_dir_all(&dir)?;
        Ok(())
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

    /// Body of the default template; an empty one falls back to the built-in skeleton.
    fn template_body(&self) -> String {
        let body = self.profile().body;
        if body.trim().is_empty() { RESUME_TEMPLATE.to_string() } else { body }
    }

    fn summarize(&self, slug: &str, text: &str, path: &Path) -> ResumeSummary {
        let d = Doc::parse(text);
        ResumeSummary {
            slug: slug.to_string(),
            title: title_of(&d, slug),
            status: d.str("status").filter(|s| STATUSES.contains(&s.as_str())).unwrap_or_else(|| "draft".into()),
            tags: d.list("tags"),
            updated_ms: mtime_ms(path),
            words: count_words(&d.body),
            has_posting: path.with_file_name(POSTING_FILE).is_file(),
        }
    }

    // ---- target posting ------------------------------------------------------

    /// The job posting a resume targets. Empty when none is saved.
    pub fn read_posting(&self, slug: &str) -> Result<TextFile> {
        let path = self.existing_resume(slug)?.with_file_name(POSTING_FILE);
        let content = fs::read_to_string(path).unwrap_or_default();
        Ok(TextFile { hash: hash(&content), content })
    }

    /// Save (or, with blank content, remove) the posting a resume targets.
    pub fn write_posting(&self, slug: &str, content: &str, expected_hash: Option<&str>) -> Result<TextFile> {
        let path = self.existing_resume(slug)?.with_file_name(POSTING_FILE);
        self.guard(&path, expected_hash)?;
        if content.trim().is_empty() {
            if path.exists() {
                fs::remove_file(&path)?;
            }
        } else {
            atomic_write(&path, content)?;
        }
        self.read_posting(slug)
    }

    // ---- profile & instructions ----------------------------------------------

    fn profile_path(&self) -> PathBuf {
        self.root.join(PROFILE_FILE)
    }

    fn instructions_path(&self) -> PathBuf {
        self.root.join(INSTRUCTIONS_FILE)
    }

    pub fn read_profile(&self) -> Result<TextFile> {
        let content = fs::read_to_string(self.profile_path()).unwrap_or_default();
        Ok(TextFile { hash: hash(&content), content })
    }

    pub fn write_profile(&self, content: &str, expected_hash: Option<&str>) -> Result<TextFile> {
        self.guard(&self.profile_path(), expected_hash)?;
        check_page("the default template", &Doc::parse(content), Some(&self.profile()))?;
        atomic_write(&self.profile_path(), content)?;
        self.read_profile()
    }

    pub fn profile(&self) -> Doc {
        Doc::parse(&self.read_profile().map(|f| f.content).unwrap_or_default())
    }

    pub fn read_instructions(&self) -> Result<InstructionFile> {
        self.ensure_instructions()?;
        let content = fs::read_to_string(self.instructions_path())?;
        Ok(InstructionFile { items: decode_instructions(&content)?, hash: hash(&content) })
    }

    /// The instructions as a Markdown document, for agents that read them as a resource.
    pub fn instructions_markdown(&self) -> Result<String> {
        let mut s = String::from("# Instructions\n");
        for item in self.read_instructions()?.items {
            s.push_str("\n---\n\n");
            s.push_str(&item);
            s.push('\n');
        }
        Ok(s)
    }

    /// Replace the instructions. Only the desktop app does this; agents read them.
    pub fn write_instructions(&self, items: &[String], expected_hash: Option<&str>) -> Result<InstructionFile> {
        self.ensure_instructions()?;
        self.guard(&self.instructions_path(), expected_hash)?;
        atomic_write(&self.instructions_path(), &encode_instructions(items))?;
        self.read_instructions()
    }

    /// Create `instructions.json`. A leftover `memory.md` from format 1 is migrated into it once.
    fn ensure_instructions(&self) -> Result<()> {
        let path = self.instructions_path();
        if path.exists() {
            return Ok(());
        }
        let memory = self.root.join("memory.md");
        let items = fs::read_to_string(&memory).map(|t| migrate_memory(&t)).unwrap_or_default();
        atomic_write(&path, &encode_instructions(&items))?;
        if memory.is_file() {
            fs::remove_file(&memory)?;
        }
        Ok(())
    }

    // ---- parts (front matter + body edited separately) -----------------------

    fn target_path(&self, t: &Target) -> Result<PathBuf> {
        match t {
            Target::Resume(slug) => self.existing_resume(slug),
            Target::Profile => Ok(self.profile_path()),
        }
    }

    pub fn read_parts(&self, t: &Target) -> Result<Parts> {
        let content = fs::read_to_string(self.target_path(t)?).unwrap_or_default();
        let doc = Doc::parse(&content);
        let fields = doc
            .meta
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    Value::Str(s) => serde_json::Value::String(s.clone()),
                    Value::List(l) => serde_json::json!(l),
                };
                (k.clone(), v)
            })
            .collect();
        Ok(Parts {
            body_offset: content.lines().count().saturating_sub(doc.body.lines().count()),
            hash: hash(&content),
            fields,
            body: doc.body,
        })
    }

    /// Replace the body, keeping the front matter as it is on disk. The app's editor saves through
    /// this and holds the text to one page itself, measuring the real layout, so the estimate in
    /// [`check_page`] isn't applied here: it must never refuse text the editor shows fitting.
    pub fn write_body(&self, t: &Target, body: &str, expected_hash: Option<&str>) -> Result<Parts> {
        let path = self.target_path(t)?;
        self.guard(&path, expected_hash)?;
        let mut doc = Doc::parse(&fs::read_to_string(&path).unwrap_or_default());
        doc.body = body.to_string();
        atomic_write(&path, &doc.to_string())?;
        self.read_parts(t)
    }

    /// Set front matter fields, keeping the body. Empty or null values remove the field.
    pub fn set_fields(
        &self,
        t: &Target,
        fields: &serde_json::Map<String, serde_json::Value>,
        expected_hash: Option<&str>,
    ) -> Result<Parts> {
        let path = self.target_path(t)?;
        self.guard(&path, expected_hash)?;
        let mut doc = Doc::parse(&fs::read_to_string(&path).unwrap_or_default());
        for (k, v) in fields {
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                bail!("invalid field name '{k}'");
            }
            if k == "status" && v.as_str().is_some_and(|s| !s.is_empty() && !STATUSES.contains(&s)) {
                bail!("status must be one of: {}", STATUSES.join(", "));
            }
            let value = match v {
                serde_json::Value::Array(a) => Value::List(a.iter().filter_map(|x| x.as_str()).map(String::from).collect()),
                serde_json::Value::String(s) => Value::Str(s.clone()),
                serde_json::Value::Null => Value::Str(String::new()),
                other => Value::Str(other.to_string()),
            };
            doc.set(k, value);
        }
        atomic_write(&path, &doc.to_string())?;
        self.read_parts(t)
    }

    // ---- change detection ----------------------------------------------------

    /// A cheap fingerprint of every file the app shows. It changes whenever anything is
    /// edited on disk, by an agent or by hand, so the UI knows to refresh.
    pub fn revision(&self) -> String {
        let _ = self.ensure_instructions();
        let mut h = Fnv::new();
        let mut visit = |p: &Path| {
            if let Ok(m) = fs::metadata(p) {
                h.feed(p.to_string_lossy().as_bytes());
                h.feed(&m.len().to_le_bytes());
                h.feed(&mtime_ms(p).to_le_bytes());
            }
        };
        visit(&self.profile_path());
        visit(&self.instructions_path());
        for slug in self.slugs() {
            let dir = self.root.join("resumes").join(slug);
            visit(&dir.join(RESUME_FILE));
            visit(&dir.join(POSTING_FILE));
        }
        format!("{:016x}", h.0)
    }
}

// ---- helpers -----------------------------------------------------------------

/// The format a folder's `resuflow.json` records; 0 for none (a folder from before the marker
/// had one, or one that is not a workspace yet).
pub fn folder_format(root: &Path) -> u64 {
    fs::read_to_string(root.join("resuflow.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["format"].as_u64())
        .unwrap_or(0)
}

fn migrate_file(path: &Path, fallback: Option<&Doc>) -> Result<()> {
    let before = fs::read_to_string(path)?;
    let mut doc = Doc::parse(&before);
    migrate_header(&mut doc, fallback);
    doc.body = rule_sections(&doc.body);
    let after = doc.to_string();
    if after != before {
        atomic_write(path, &after)?;
    }
    Ok(())
}

/// Format 2 kept the header in front matter, with fields a resume left out taken from the
/// template. Write the header it printed as Markdown at the top of the body instead.
fn migrate_header(doc: &mut Doc, fallback: Option<&Doc>) {
    let pick = |k: &str| doc.str(k).or_else(|| fallback.and_then(|p| p.str(k))).unwrap_or_default();
    let (name, headline) = (pick("name"), pick("headline"));
    let mut contact: Vec<String> = ["location", "email", "phone"].iter().map(|k| pick(k)).filter(|v| !v.is_empty()).collect();
    contact.extend(if doc.get("links").is_some() { doc.list("links") } else { fallback.map(|p| p.list("links")).unwrap_or_default() });

    let mut header = String::new();
    if !doc.body.trim_start().starts_with("# ") {
        if !name.is_empty() {
            header.push_str(&format!("# {name}\n"));
        }
        if !headline.is_empty() {
            header.push_str(&format!("{headline}\n"));
        }
        if !contact.is_empty() {
            header.push_str(&format!("{}\n", contact.join(" · ")));
        }
    }
    for k in LEGACY_HEADER_FIELDS {
        doc.set(k, Value::Str(String::new()));
    }
    if !header.is_empty() {
        doc.body = format!("{header}\n{}", doc.body.trim_start_matches(['\r', '\n']));
    }
}

/// Section headings used to draw their own underline. Make each one a `---` the user can edit.
fn rule_sections(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut out = String::with_capacity(body.len() + 64);
    let mut fenced = false;
    for (i, line) in lines.iter().enumerate() {
        out.push_str(line);
        out.push('\n');
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
        }
        let ruled = lines.get(i + 1).is_some_and(|n| {
            n.trim() == "---" || matches!(crate::render::component(n), Some(crate::render::Component::Rule(_)))
        });
        if !fenced && t.starts_with("## ") && !ruled {
            out.push_str("---\n");
        }
    }
    if !body.ends_with('\n') {
        out.pop();
    }
    out
}

fn title_of(d: &Doc, slug: &str) -> String {
    d.str("title").unwrap_or_else(|| slug.to_string())
}

/// A resume, and the default template each one starts from, is one A4 page. Refuse `doc` (`what`
/// names it) if it would print past one, unless it runs no further past than `before` (the version
/// it replaces or copies), so one that is already long stays editable while it shrinks.
fn check_page(what: &str, doc: &Doc, before: Option<&Doc>) -> Result<()> {
    let fill = crate::render::page_fill(doc);
    let limit = before.map_or(1.0, |b| crate::render::page_fill(b).max(1.0));
    if fill <= limit + 1e-6 {
        return Ok(());
    }
    let pct = |f: f64| (f * 100.0).ceil();
    if limit > 1.0 {
        bail!(
            "{what} is already longer than one A4 page ({}%), and this would make it longer ({}%). \
             Shorten it to fit one page instead.",
            pct(limit),
            pct(fill)
        );
    }
    bail!(
        "{what} would run past one A4 page (about {}% of a page). It must fit on one page: \
         cut or tighten it, then write it again.",
        pct(fill)
    )
}

pub fn count_words(text: &str) -> usize {
    crate::render::strip_components(text)
        .split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

/// Names that become paths: lowercase letters, digits and dashes, at most 64 bytes.
pub fn check_slug(s: &str) -> Result<()> {
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-');
    if !ok {
        bail!("invalid name '{s}': use lowercase letters, digits and dashes (max 64)");
    }
    Ok(())
}


pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Refuse the write if the file no longer matches the hash the caller read.
/// A missing file hashes like an empty one.
fn guard(path: &Path, expected_hash: Option<&str>) -> Result<()> {
    let Some(expected) = expected_hash else { return Ok(()) };
    if hash(&fs::read_to_string(path).unwrap_or_default()) != expected {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        bail!("conflict: {name} changed since it was read (re-read and retry)");
    }
    Ok(())
}

/// Write to a temporary sibling, flush it to disk, then rename over the target, so readers
/// (the app, an agent, a sync client) never see a half-written file.
pub(crate) fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(result?)
}

/// Copy a folder tree, skipping `.backups/` and half-written temporary files. Each file is
/// flushed to disk, so a backup marked complete really is.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if name == BACKUPS_DIR || (text.starts_with('.') && text.ends_with(".tmp")) {
            continue;
        }
        let (src, dest) = (entry.path(), to.join(&name));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&src, &dest)?;
        } else if kind.is_file() {
            fs::copy(&src, &dest)?;
            fs::OpenOptions::new().write(true).open(&dest)?.sync_all()?; // Windows flushes only writable handles
        }
    }
    Ok(())
}

fn create_new(path: &Path, content: &str) -> Result<()> {
    if !path.exists() {
        atomic_write(path, content)?;
    }
    Ok(())
}

fn mtime_ms(p: &Path) -> u64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
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

/// instructions.json: `{"instructions": ["rule", ...]}`, blank rules dropped.
fn encode_instructions(items: &[String]) -> String {
    let items: Vec<&str> = items.iter().map(|i| i.trim()).filter(|i| !i.is_empty()).collect();
    let mut s = serde_json::to_string_pretty(&serde_json::json!({ "instructions": items })).unwrap_or_default();
    s.push('\n');
    s
}

/// Refuses a file it cannot read, so a hand-edit gone wrong is reported instead of overwritten.
fn decode_instructions(content: &str) -> Result<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(content).map_err(|e| Error(format!("{INSTRUCTIONS_FILE} is not valid JSON: {e}")))?;
    let Some(list) = v["instructions"].as_array() else {
        bail!("{INSTRUCTIONS_FILE} needs an \"instructions\" list");
    };
    let mut items = Vec::new();
    for item in list {
        let Some(text) = item.as_str() else { bail!("{INSTRUCTIONS_FILE}: every instruction must be a string") };
        if !text.trim().is_empty() {
            items.push(text.trim().to_string());
        }
    }
    Ok(items)
}

/// The instructions.md of format 4: a heading, then one rule per `---`-separated block.
fn decode_markdown_instructions(content: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur: Option<String> = None;
    for line in content.lines() {
        if line.trim() == "---" {
            items.extend(cur.replace(String::new()));
        } else if let Some(c) = cur.as_mut() {
            c.push_str(line);
            c.push('\n');
        }
    }
    items.extend(cur);
    items.into_iter().map(|i| i.trim().to_string()).filter(|i| !i.is_empty()).collect()
}

fn migrate_memory(text: &str) -> Vec<String> {
    let text = text.trim();
    if text == LEGACY_MEMORY.trim() {
        return vec![];
    }
    let rest = text.strip_prefix("# Memory").unwrap_or(text).trim();
    if rest.is_empty() { vec![] } else { vec![rest.to_string()] }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh workspace in the system temp folder, removed when dropped.
    pub(crate) struct TempWs(pub Workspace);

    impl TempWs {
        pub(crate) fn new() -> TempWs {
            static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("resuflow-test-{}-{}-{n}", std::process::id(), now_ms()));
            TempWs(Workspace::init(dir).unwrap())
        }
    }

    impl Drop for TempWs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0.root);
        }
    }

    fn create(ws: &Workspace, title: &str) -> ResumeFile {
        ws.create_resume(NewResume { title, ..Default::default() }).unwrap()
    }

    /// `n` bullets, each a line or two long: about 36 lines fill the page.
    fn bullets(n: usize) -> String {
        (0..n).map(|i| format!("- Shipped ledger migration {i}, cutting settlement latency by 42% across payment rails\n")).collect()
    }

    #[test]
    fn resumes_stay_on_one_page() {
        let t = TempWs::new();
        let ws = &t.0;
        let r = create(ws, "One Page");
        let slug = &r.summary.slug;
        let long = format!("# Ada\n\n## Experience\n{}", bullets(80));
        let err = ws.write_resume(slug, &long, None).unwrap_err().0;
        assert!(err.contains("past one A4 page"), "{err}");
        assert!(ws.create_resume(NewResume { title: "Long", content: Some(&long), ..Default::default() }).is_err());
        let fits = format!("# Ada\n\n## Experience\n{}", bullets(20));
        let r = ws.write_resume(slug, &fits, None).unwrap();
        assert!(ws.edit_resume(slug, "- Shipped ledger migration 0,", &bullets(60), Some(&r.hash)).is_err());

        // A resume that is already long (written in the app or by hand) may shrink, never grow.
        ws.write_body(&Target::Resume(slug.clone()), &long, None).unwrap();
        ws.set_meta(slug, None, Some("ready"), None, None).unwrap();
        let copy = ws.create_resume(NewResume { title: "Copy", from: Some(slug), ..Default::default() }).unwrap();
        assert!(ws.edit_resume(&copy.summary.slug, "- Shipped ledger migration 1,", "- Shipped ledger migration 1, and more words to wrap the line well past its end and onto two,", None).is_err());
        ws.edit_resume(slug, &bullets(80)[..200], "", None).unwrap();

        // The default template is held to the same page.
        assert!(ws.write_profile(&long, None).unwrap_err().0.contains("default template"));
        ws.write_profile(&fits, None).unwrap();
        ws.write_body(&Target::Profile, &long, None).unwrap(); // a long one from before the limit
        assert!(ws.write_profile(&format!("{long}{}", bullets(1)), None).is_err());
        ws.write_profile(&format!("# Ada\n\n## Experience\n{}", bullets(60)), None).unwrap();
        create(ws, "From Template"); // copying the template is never refused
    }

    #[test]
    fn create_edit_conflict() {
        let t = TempWs::new();
        let ws = &t.0;
        let r = ws.create_resume(NewResume { title: "Staff Engineer, Platform", tags: vec!["rust".into()], ..Default::default() }).unwrap();
        assert_eq!(r.summary.slug, "staff-engineer-platform");
        assert_eq!(r.summary.tags, vec!["rust"]);
        assert_eq!(r.summary.status, "draft");
        assert!(!r.content.contains("status:"));
        assert!(r.content.contains("\n# Your Name\n"));
        assert!(!r.content.contains("name:"));

        let r2 = ws.edit_resume(&r.summary.slug, "Two or three lines aimed at this role.", "Profile.", Some(&r.hash)).unwrap();
        assert!(r2.content.contains("\nProfile.\n"));
        assert!(ws.edit_resume(&r.summary.slug, "Profile.", "X", Some(&r.hash)).unwrap_err().0.starts_with("conflict"));
        assert!(ws.edit_resume(&r.summary.slug, "- ", "* ", None).unwrap_err().0.contains("times"));

        let r3 = ws.set_meta(&r.summary.slug, None, Some("ready"), Some(vec!["Rust".into(), "rust".into(), " ".into()]), None).unwrap();
        assert_eq!(r3.summary.status, "ready");
        assert_eq!(r3.summary.tags, vec!["Rust"]);
        assert!(ws.set_meta(&r.summary.slug, None, Some("bogus"), None, None).is_err());
        assert!(ws.set_meta(&r.summary.slug, Some("x"), None, None, Some(&r.hash)).is_err());
    }

    #[test]
    fn copies_carry_posting_and_reset_status() {
        let t = TempWs::new();
        let ws = &t.0;
        let r = create(ws, "Backend");
        ws.set_meta("backend", None, Some("sent"), None, None).unwrap();
        let p = ws.write_posting("backend", "We need Rust and Kafka.", Some(&hash(""))).unwrap();
        assert!(ws.write_posting("backend", "stale", Some(&hash(""))).is_err());
        assert!(ws.read_resume("backend").unwrap().summary.has_posting);

        let copy = ws.create_resume(NewResume { title: "Backend", from: Some(&r.summary.slug), ..Default::default() }).unwrap();
        assert_eq!(copy.summary.slug, "backend-2");
        assert_eq!(copy.summary.status, "draft");
        assert_eq!(ws.read_posting("backend-2").unwrap().content, p.content);

        ws.write_posting("backend", "  ", Some(&p.hash)).unwrap();
        assert!(!ws.read_resume("backend").unwrap().summary.has_posting);
        assert!(ws.read_posting("nope").is_err());
    }

    #[test]
    fn archive_restore_and_delete() {
        let t = TempWs::new();
        let ws = &t.0;
        create(ws, "Data Engineer");
        ws.write_posting("data-engineer", "posting", None).unwrap();
        assert_eq!(ws.archive_resume("data-engineer").unwrap().summary.status, "archived");
        let restored = ws.restore_resume("data-engineer").unwrap();
        assert_eq!(restored.summary.status, "draft");
        assert!(restored.summary.has_posting);
        assert!(ws.restore_resume("data-engineer").is_err()); // not archived any more

        let before = ws.revision();
        ws.delete_resume("data-engineer").unwrap();
        assert_ne!(before, ws.revision());
        assert!(ws.list_resumes().unwrap().is_empty());
        assert!(!ws.root.join("resumes/data-engineer").exists());
        assert!(ws.delete_resume("data-engineer").is_err());
        assert!(ws.delete_resume("../x").is_err());
    }

    #[test]
    fn old_trash_comes_back_archived() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        create(&t.0, "Old");
        fs::write(root.join("resuflow.json"), "{\"app\": \"resuflow\", \"format\": 4}").unwrap();
        fs::create_dir_all(root.join(".trash/old-123")).unwrap();
        fs::write(root.join(".trash/old-123").join(RESUME_FILE), "---\ntitle: Old\nstatus: sent\n---\nBody\n").unwrap();
        fs::write(root.join(".trash/old-123/posting.md"), "posting").unwrap();
        let ws = Workspace::open(&root).unwrap();
        let r = ws.read_resume("old-2").unwrap();
        assert_eq!(r.summary.status, "archived");
        assert!(r.summary.has_posting);
        assert!(!root.join(".trash").exists());
    }

    #[test]
    fn parts_and_template() {
        let t = TempWs::new();
        let ws = &t.0;
        let p = ws.read_parts(&Target::Profile).unwrap();
        assert!(p.fields.is_empty());
        assert!(p.body.starts_with("# Your Name\n"));
        let p2 = ws.write_body(&Target::Profile, "# Ada\nNew notes\n", Some(&p.hash)).unwrap();
        assert_eq!(p2.body, "# Ada\nNew notes\n");

        let mut f = serde_json::Map::new();
        f.insert("note".into(), "kept".into());
        f.insert("links".into(), serde_json::json!(["a.dev", ""]));
        f.insert("phone".into(), serde_json::Value::Null);
        let p3 = ws.set_fields(&Target::Profile, &f, Some(&p2.hash)).unwrap();
        assert_eq!(p3.fields["note"], "kept");
        assert_eq!(p3.fields["links"], serde_json::json!(["a.dev"]));
        assert!(p3.fields.get("phone").is_none());
        assert_eq!(p3.body_offset, p3.fields.len() + 3);

        let fresh = create(ws, "From the template");
        assert!(fresh.content.contains("# Ada\nNew notes"));
        assert!(!fresh.content.contains("note: kept"));

        let mut bad = serde_json::Map::new();
        bad.insert("status".into(), "nope".into());
        assert!(ws.set_fields(&Target::Resume(fresh.summary.slug.clone()), &bad, None).is_err());
        assert!(ws.read_parts(&Target::Resume("missing".into())).is_err());
    }

    #[test]
    fn instructions_roundtrip() {
        let t = TempWs::new();
        let ws = &t.0;
        let ins = ws.read_instructions().unwrap();
        assert!(ins.items.is_empty());
        let ins = ws.write_instructions(&["Lead with the target role.".into(), "".into(), "No tables.\n\nEver.".into()], Some(&ins.hash)).unwrap();
        assert_eq!(ins.items, vec!["Lead with the target role.", "No tables.\n\nEver."]);
        assert!(ws.write_instructions(&["stale".into()], Some("not-the-hash")).is_err());
    }

    #[test]
    fn memory_file_becomes_instructions() {
        for (memory, expected) in [("# Memory\n\nPrefer one page.\n", vec!["Prefer one page."]), (LEGACY_MEMORY, vec![])] {
            let t = TempWs::new();
            fs::remove_file(t.0.root.join(INSTRUCTIONS_FILE)).unwrap();
            fs::write(t.0.root.join("memory.md"), memory).unwrap();
            assert_eq!(t.0.read_instructions().unwrap().items, expected);
            assert!(!t.0.root.join("memory.md").exists());
        }
    }

    #[test]
    fn format_2_header_moves_into_the_body() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        fs::write(root.join("resuflow.json"), "{\"app\": \"resuflow\", \"format\": 2}").unwrap();
        fs::remove_file(root.join(PROFILE_FILE)).unwrap(); // a format 2 workspace has profile.md instead
        fs::write(
            root.join("profile.md"),
            "---\nname: Ada\nheadline: Engineer\nemail: a@b.c\nlinks: [a.dev]\n---\n\n## Summary\n\nText\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("resumes/own")).unwrap();
        fs::write(root.join("resumes/own/resume.md"), "---\ntitle: Own\nheadline: Data person\nlocation: Leeds\n---\n## Skills\n---\n- Rust\n```\n## not a heading\n```\n").unwrap();
        fs::create_dir_all(root.join(".trash/old-1")).unwrap();
        fs::write(root.join(".trash/old-1/resume.md"), "---\ntitle: Old\n---\n## Work\n## Ruled\n<Rule color=\"red\" />\n").unwrap();

        let ws = Workspace::open(&root).unwrap();
        assert!(!root.join("profile.md").exists() && !root.join("resumes/own/resume.md").exists());
        assert_eq!(fs::read_to_string(root.join(PROFILE_FILE)).unwrap(), "# Ada\nEngineer\na@b.c · a.dev\n\n## Summary\n---\n\nText\n");
        assert_eq!(
            ws.read_resume("own").unwrap().content,
            "---\ntitle: Own\n---\n\n# Ada\nData person\nLeeds · a@b.c · a.dev\n\n## Skills\n---\n- Rust\n```\n## not a heading\n```\n"
        );
        let old = ws.read_resume("old").unwrap();
        assert_eq!(old.summary.status, "archived"); // the old trash comes back archived
        let old = old.content;
        assert!(old.contains("# Ada\nEngineer\n") && old.contains("## Work\n---\n## Ruled\n<Rule color=\"red\" />\n"));
        assert!(fs::read_to_string(root.join("resuflow.json")).unwrap().contains("\"format\": 5"));

        // Once migrated, opening again changes nothing.
        let own = ws.read_resume("own").unwrap().hash;
        assert_eq!(Workspace::open(&root).unwrap().read_resume("own").unwrap().hash, own);
    }

    #[test]
    fn format_3_sources_become_mdx() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        fs::write(root.join("resuflow.json"), "{\"app\": \"resuflow\", \"format\": 3}").unwrap();
        fs::rename(root.join(PROFILE_FILE), root.join("profile.md")).unwrap();
        fs::create_dir_all(root.join("resumes/a")).unwrap();
        fs::write(root.join("resumes/a/resume.md"), "---\ntitle: A\n---\n\n# Ada\n").unwrap();
        fs::write(root.join("resumes/a/posting.md"), "Rust").unwrap();

        let ws = Workspace::open(&root).unwrap();
        let a = ws.read_resume("a").unwrap();
        assert_eq!(a.content, "---\ntitle: A\n---\n\n# Ada\n"); // renamed, not rewritten
        assert!(a.summary.has_posting && root.join("resumes/a").join(POSTING_FILE).is_file());
        assert!(ws.read_profile().unwrap().content.starts_with("# Your Name"));
        assert!(!root.join("resumes/a/resume.md").exists() && !root.join("profile.md").exists());
    }

    #[test]
    fn format_4_instructions_and_postings_are_migrated() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        fs::write(root.join("resuflow.json"), "{\"app\": \"resuflow\", \"format\": 4}").unwrap();
        fs::remove_file(root.join(INSTRUCTIONS_FILE)).unwrap();
        fs::write(root.join("instructions.md"), "# Instructions\n\n---\n\nLead with the role.\n\n---\n\nNo tables.\n\nEver.\n").unwrap();
        create(&t.0, "A");
        fs::write(root.join("resumes/a/posting.md"), "We need Rust.").unwrap();

        let ws = Workspace::open(&root).unwrap();
        assert_eq!(ws.read_instructions().unwrap().items, vec!["Lead with the role.", "No tables.\n\nEver."]);
        assert!(!root.join("instructions.md").exists());
        assert_eq!(ws.read_posting("a").unwrap().content, "We need Rust.");
        assert!(!root.join("resumes/a/posting.md").exists());
        assert!(ws.instructions_markdown().unwrap().contains("\n---\n\nNo tables.\n\nEver.\n"));

        fs::write(root.join(INSTRUCTIONS_FILE), "not json").unwrap();
        assert!(ws.read_instructions().unwrap_err().0.contains("not valid JSON"));
    }

    #[test]
    fn imported_format_1_files_are_migrated() {
        let dir = std::env::temp_dir().join(format!("resuflow-import-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(dir.join("resumes/a")).unwrap();
        fs::write(dir.join("profile.md"), "---\nname: Ada\n---\n\nNotes\n").unwrap();
        fs::write(dir.join("resumes/a/resume.md"), "---\ntitle: A\n---\n## Work\n").unwrap();
        let t = TempWs(Workspace::init(&dir).unwrap());
        assert_eq!(t.0.read_profile().unwrap().content, "# Ada\n\nNotes\n");
        // Resumes but no template: the migration still runs, and the template is created after.
        let bare = std::env::temp_dir().join(format!("resuflow-bare-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(bare.join("resumes/b")).unwrap();
        fs::write(bare.join("resumes/b/resume.md"), "## Work\n").unwrap();
        let b = TempWs(Workspace::init(&bare).unwrap());
        assert_eq!(b.0.read_resume("b").unwrap().content, "## Work\n---\n");
        assert!(b.0.read_profile().unwrap().content.starts_with("# Your Name"));
        assert!(t.0.read_resume("a").unwrap().content.contains("# Ada\n\n## Work\n---\n"));
    }

    fn backups(root: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(root.join(BACKUPS_DIR)).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn upgrade_keeps_a_copy_of_the_old_folder() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        assert!(t.0.upgraded.is_none() && !root.join(BACKUPS_DIR).exists(), "a new folder needs no upgrade");
        let old_marker = "{\"app\": \"resuflow\", \"format\": 4}";
        fs::write(root.join("resuflow.json"), old_marker).unwrap();
        create(&t.0, "A");
        fs::write(root.join("resumes/a/posting.md"), "We need Rust.").unwrap();

        let ws = Workspace::open(&root).unwrap();
        let up = ws.upgraded.clone().expect("upgraded");
        assert_eq!((up.from, up.to), (4, FORMAT));
        assert_eq!(fs::read_to_string(up.backup.join("resumes/a/posting.md")).unwrap(), "We need Rust.");
        assert_eq!(fs::read_to_string(up.backup.join("resuflow.json")).unwrap(), old_marker);
        assert!(up.backup.join(BACKUP_DONE).is_file());
        assert!(!root.join(BACKUPS_DIR).join(JOURNAL_FILE).exists());
        assert!(!root.join(BACKUP_DONE).exists());
        // Already current: opening again takes no new backup.
        assert!(Workspace::open(&root).unwrap().upgraded.is_none());
        assert_eq!(backups(&root).len(), 1);
    }

    #[test]
    fn newer_formats_are_never_touched() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        create(&t.0, "A");
        let before = t.0.read_resume("a").unwrap();
        // A newer ResuFlow upgrades the folder while this one is running...
        fs::write(root.join("resuflow.json"), "{\"app\": \"resuflow\", \"format\": 99}").unwrap();
        let err = t.0.write_resume("a", "changed", Some(&before.hash)).unwrap_err().0;
        assert!(err.contains("newer version of ResuFlow"), "{err}");
        assert!(t.0.create_resume(NewResume { title: "B", ..Default::default() }).is_err());
        assert!(t.0.delete_resume("a").is_err());
        assert!(t.0.write_profile("x", None).is_err());
        assert_eq!(t.0.read_resume("a").unwrap().content, before.content, "reads still work");
        // ...and an older one refuses to open it at all.
        assert!(Workspace::open(&root).err().unwrap().0.contains("data format 99"));
        assert!(Workspace::init(&root).is_err());
        assert!(!root.join(BACKUPS_DIR).exists());
    }

    #[test]
    fn a_failed_upgrade_puts_everything_back() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        fs::write(root.join("resuflow.json"), "{\"app\": \"resuflow\", \"format\": 2}").unwrap();
        fs::remove_file(root.join(PROFILE_FILE)).unwrap();
        fs::create_dir_all(root.join("resumes/a")).unwrap();
        fs::create_dir_all(root.join("resumes/b")).unwrap();
        fs::write(root.join("resumes/a/resume.md"), "---\nname: Ada\n---\n## Work\n").unwrap();
        fs::write(root.join("resumes/b/resume.md"), b"not \xff utf-8").unwrap(); // `a` migrates, then `b` fails

        let err = Workspace::open(&root).err().expect("upgrade fails").0;
        assert!(err.contains("left as it was"), "{err}");
        assert_eq!(fs::read_to_string(root.join("resumes/a/resume.md")).unwrap(), "---\nname: Ada\n---\n## Work\n");
        assert!(!root.join("resumes/a").join(RESUME_FILE).exists());
        assert_eq!(fs::read(root.join("resumes/b/resume.md")).unwrap(), b"not \xff utf-8");
        assert!(fs::read_to_string(root.join("resuflow.json")).unwrap().contains("\"format\": 2"));
        assert!(!root.join(BACKUPS_DIR).join(JOURNAL_FILE).exists());
        assert_eq!(backups(&root).len(), 1, "the copy is kept");
    }

    #[test]
    fn an_interrupted_upgrade_is_rolled_back_then_redone() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        fs::write(root.join("resuflow.json"), "{\"app\": \"resuflow\", \"format\": 4}").unwrap();
        create(&t.0, "A");
        fs::write(root.join("resumes/a/posting.md"), "We need Rust.").unwrap();
        // Crash after the backup completed, halfway through migrating.
        let backup = t.0.backup(4).unwrap();
        fs::rename(root.join("resumes/a/posting.md"), root.join("resumes/a/posting.txt")).unwrap();
        fs::write(root.join("resumes/a/stray.tmp-from-migration"), "").unwrap();

        let ws = Workspace::open(&root).unwrap();
        assert_eq!(ws.read_posting("a").unwrap().content, "We need Rust.");
        assert!(!root.join("resumes/a/stray.tmp-from-migration").exists());
        assert!(!root.join("resumes/a/posting.md").exists());
        assert!(backup.join(BACKUP_DONE).is_file(), "the rolled-back copy is kept");
        assert_eq!(ws.upgraded.unwrap().from, 4);

        // Crash while the backup was still being copied: the folder was never touched.
        let partial = root.join(BACKUPS_DIR).join("format-5-1");
        fs::create_dir_all(&partial).unwrap();
        fs::write(root.join(BACKUPS_DIR).join(JOURNAL_FILE), "format-5-1").unwrap();
        let ws = Workspace::open(&root).unwrap();
        assert!(!partial.exists() && !root.join(BACKUPS_DIR).join(JOURNAL_FILE).exists());
        assert_eq!(ws.read_posting("a").unwrap().content, "We need Rust.");
    }

    #[test]
    fn concurrent_opens_upgrade_once() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        fs::write(root.join("resuflow.json"), "{\"app\": \"resuflow\", \"format\": 4}").unwrap();
        create(&t.0, "A");
        fs::write(root.join("resumes/a/posting.md"), "We need Rust.").unwrap();
        let opened: Vec<Workspace> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..4).map(|_| s.spawn(|| Workspace::open(&root).ok().unwrap())).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(opened.iter().filter(|w| w.upgraded.is_some()).count(), 1);
        assert_eq!(backups(&root).len(), 1);
        assert_eq!(opened[0].read_posting("a").unwrap().content, "We need Rust.");
    }

    #[test]
    fn only_recent_backups_are_kept() {
        let t = TempWs::new();
        let root = t.0.root.clone();
        for i in 0..BACKUPS_KEPT + 2 {
            let dir = root.join(BACKUPS_DIR).join(format!("format-4-{}", 1000 + i));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(BACKUP_DONE), "").unwrap();
        }
        fs::create_dir_all(root.join(BACKUPS_DIR).join("mine")).unwrap(); // not ours: left alone
        t.0.prune_backups();
        let names: Vec<String> = backups(&root).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), BACKUPS_KEPT + 1);
        assert!(!names.contains(&"format-4-1000".to_string()) && !names.contains(&"format-4-1001".to_string()));
        assert!(names.contains(&"mine".to_string()));
    }

    #[test]
    fn marker_matches_format() {
        assert!(MARKER.contains(&format!("\"format\": {FORMAT}")));
    }

    #[test]
    fn rejects_bad_names() {
        assert!(check_slug("../etc").is_err());
        assert!(check_slug("a/b").is_err());
        assert!(check_slug("Good").is_err());
        assert!(check_slug("good-1").is_ok());
    }
}
