//! Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0).
//!
//! Agents launch `resuflow mcp` as a subprocess. There is no network listener, port or
//! token: the agent can reach exactly what its own process can, and nothing else can reach it.
//!
//! Tools mirror the app's actions one-to-one. Skills in the workspace are also exposed
//! as MCP prompts.

use crate::config;
use crate::workspace::{Note, Workspace, STATUSES};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

const SUPPORTED: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "ResuFlow keeps the user's resumes as Markdown files in a local workspace. \
Start with get_workspace: it returns the default template, instructions, resume list and available skills. \
profile.md is the default template. Its front matter (name, headline, email, phone, location, links) is the printed header of every resume. \
Its body is copied into each new resume, so edit that body to change what a new resume starts as. Mine it for facts; never invent experience or metrics. \
instructions.md is a list of rules the user writes. Read them with read_instructions and follow them. Do not try to change them; only the user edits instructions in the app. \
Each resume is resumes/<slug>/resume.md with front matter (title, status, tags, optional headline/links overrides) and a Markdown body using ## sections, ### Role — Company, and - bullets. \
Prefer edit_resume for targeted changes and pass the expected_hash you got from a read, so you never overwrite the user's \
concurrent edits in the desktop app. Run check_resume after editing. \
Load a skill with read_skill (or the matching prompt) before resume research, bullet writing, ATS passes or critiques.";

pub fn serve(dir: Option<PathBuf>) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut server = Server { dir };
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let out: Vec<Value> = batch.iter().filter_map(|m| server.handle(m)).collect();
                (!out.is_empty()).then(|| Value::Array(out))
            }
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = reply {
            serde_json::to_writer(&mut stdout, &r)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

struct Server {
    dir: Option<PathBuf>,
}

impl Server {
    fn workspace(&self) -> Result<Workspace, String> {
        config::workspace(self.dir.clone()).map_err(|e| e.0)
    }

    fn handle(&mut self, msg: &Value) -> Option<Value> {
        let method = msg.get("method")?.as_str()?;
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let id = id?; // notifications (no id) get no reply

        let result = match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or(SUPPORTED[0]);
                let version = if SUPPORTED.contains(&asked) { asked } else { SUPPORTED[0] };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {}, "prompts": {} },
                    "serverInfo": { "name": "resuflow", "title": "ResuFlow", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(match self.call(name, &args) {
                    Ok(v) => {
                        let text = match &v {
                            Value::String(s) => s.clone(),
                            other => serde_json::to_string_pretty(other).unwrap_or_default(),
                        };
                        let mut r = json!({ "content": [{ "type": "text", "text": text }], "isError": false });
                        if v.is_object() {
                            r["structuredContent"] = v;
                        }
                        r
                    }
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                })
            }
            "prompts/list" => self.prompts_list(),
            "prompts/get" => self.prompts_get(&params),
            "resources/list" => Ok(json!({ "resources": [] })),
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, m)) => error(id, code, &m),
        })
    }

    fn call(&self, name: &str, a: &Value) -> Result<Value, String> {
        let ws = self.workspace()?;
        let s = |k: &str| a.get(k).and_then(Value::as_str);
        let req = |k: &str| s(k).ok_or_else(|| format!("missing required argument `{k}`"));
        let hash = s("expected_hash");
        let tags = |k: &str| {
            a.get(k).and_then(Value::as_array).map(|v| {
                v.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>()
            })
        };

        match name {
            "get_workspace" => {
                let profile = ws.profile();
                Ok(json!({
                    "root": ws.root,
                    "profile": {
                        "name": profile.str("name"),
                        "headline": profile.str("headline"),
                        "notes_words": profile.body.split_whitespace().count(),
                    },
                    "resumes": j(ws.list_resumes())?,
                    "skills": crate::skills::list().into_iter().map(|s| json!({ "name": s.name, "description": s.description })).collect::<Vec<_>>(),
                    "instructions": ws.read_instructions().map_err(|e| e.0)?.items,
                    "statuses": STATUSES,
                }))
            }
            "read_profile" => j(ws.read_note(Note::Profile)),
            "update_profile" => j(ws.write_note(Note::Profile, req("content")?, hash)),
            "read_instructions" => j(ws.read_instructions()),
            "list_resumes" => {
                let mut list = ws.list_resumes().map_err(|e| e.0)?;
                if let Some(st) = s("status") {
                    list.retain(|r| r.status == st);
                }
                if let Some(t) = s("tag") {
                    list.retain(|r| r.tags.iter().any(|x| x.eq_ignore_ascii_case(t)));
                }
                Ok(json!({ "resumes": list }))
            }
            "read_resume" => j(ws.read_resume(req("slug")?)),
            "create_resume" => j(ws.create_resume(
                req("title")?,
                s("slug"),
                s("from"),
                s("content"),
                tags("tags").unwrap_or_default(),
            )),
            "write_resume" => j(ws.write_resume(req("slug")?, req("content")?, hash)),
            "edit_resume" => j(ws.edit_resume(req("slug")?, req("old_text")?, a["new_text"].as_str().unwrap_or(""), hash)),
            "set_resume_meta" => j(ws.set_meta(req("slug")?, s("title"), s("status"), tags("tags"))),
            "delete_resume" => {
                let moved = ws.delete_resume(req("slug")?).map_err(|e| e.0)?;
                Ok(json!({ "moved_to": moved }))
            }
            "check_resume" => j(ws.check(req("slug")?)),
            "render_resume" => {
                let slug = req("slug")?;
                let html = ws.render(slug).map_err(|e| e.0)?;
                if a["save"].as_bool().unwrap_or(false) {
                    let path = ws.root.join("resumes").join(slug).join("resume.html");
                    std::fs::write(&path, &html).map_err(|e| e.to_string())?;
                    Ok(json!({ "saved": path, "bytes": html.len() }))
                } else {
                    Ok(Value::String(html))
                }
            }
            "read_skill" => {
                let name = req("name")?;
                crate::skills::get(name).map(|s| Value::String(s.content)).ok_or_else(|| format!("no skill named '{name}'"))
            }
            _ => Err(format!("unknown tool: {name}")),
        }
    }

    fn prompts_list(&self) -> Result<Value, (i64, String)> {
        let prompts: Vec<Value> = crate::skills::list()
            .into_iter()
            .map(|s| json!({
                "name": s.name,
                "description": s.description,
                "arguments": [{ "name": "slug", "description": "Resume to apply it to (optional)", "required": false }],
            }))
            .collect();
        Ok(json!({ "prompts": prompts }))
    }

    fn prompts_get(&self, p: &Value) -> Result<Value, (i64, String)> {
        let name = p["name"].as_str().unwrap_or_default();
        let skill = crate::skills::get(name).ok_or((-32602, format!("no prompt named '{name}'")))?;
        let mut text = skill.content;
        if let Some(slug) = p["arguments"]["slug"].as_str().filter(|s| !s.is_empty()) {
            text.push_str(&format!("\n\nApply this to the resume `{slug}` (read it with read_resume)."));
        }
        Ok(json!({
            "description": name,
            "messages": [{ "role": "user", "content": { "type": "text", "text": text } }],
        }))
    }
}

fn j<T: serde::Serialize>(v: crate::workspace::Result<T>) -> Result<Value, String> {
    v.map_err(|e| e.0).map(|x| serde_json::to_value(x).unwrap_or(Value::Null))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool(name: &str, description: &str, props: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required, "additionalProperties": false },
        "annotations": { "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": false },
    })
}

fn tools() -> Vec<Value> {
    let slug = json!({ "type": "string", "description": "Resume slug, e.g. backend-acme" });
    let hash = json!({ "type": "string", "description": "Hash from your last read. If the file changed since, the write is refused instead of overwriting." });
    let tags = json!({ "type": "array", "items": { "type": "string" } });
    let status = json!({ "type": "string", "enum": STATUSES });
    vec![
        tool("get_workspace", "Overview: workspace path, template name and headline, instruction boxes, all resumes, skills. Call first.", json!({}), &[], true),
        tool("read_profile", "Read the default template (profile.md). Front matter is the printed header of every resume. The body is copied into each new resume.", json!({}), &[], true),
        tool("update_profile", "Replace the default template (profile.md) entirely. New resumes copy its body.", json!({ "content": { "type": "string" }, "expected_hash": hash }), &["content"], false),
        tool("read_instructions", "Read the user's instruction boxes. Follow them. Do not try to change them; only the user edits instructions in the app.", json!({}), &[], true),
        tool("list_resumes", "List resumes (newest first), optionally filtered.", json!({ "status": status, "tag": { "type": "string" } }), &[], true),
        tool("read_resume", "Read a resume's full Markdown source, metadata and hash.", json!({ "slug": slug }), &["slug"], true),
        tool("create_resume", "Create a resume. Use `from` to copy an existing resume (the usual way to tailor a variant), or `content` for full Markdown. Slug is derived from title if omitted.", json!({ "title": { "type": "string" }, "slug": slug, "from": { "type": "string", "description": "Slug to copy" }, "content": { "type": "string" }, "tags": tags }), &["title"], false),
        tool("write_resume", "Replace a resume's full source (front matter + body). Prefer edit_resume for partial changes.", json!({ "slug": slug, "content": { "type": "string" }, "expected_hash": hash }), &["slug", "content"], false),
        tool("edit_resume", "Replace one exact, unique occurrence of old_text with new_text in a resume's source.", json!({ "slug": slug, "old_text": { "type": "string" }, "new_text": { "type": "string" }, "expected_hash": hash }), &["slug", "old_text", "new_text"], false),
        tool("set_resume_meta", "Change a resume's title, status or tags.", json!({ "slug": slug, "title": { "type": "string" }, "status": status, "tags": tags }), &["slug"], false),
        tool("delete_resume", "Move a resume to the workspace's .trash folder (recoverable).", json!({ "slug": slug }), &["slug"], false),
        tool("check_resume", "Deterministic checks: missing contact, ATS-hostile tables or images, weak openers, unquantified or overlong bullets, leftovers, length. Line numbers refer to resume.md.", json!({ "slug": slug }), &["slug"], true),
        tool("render_resume", "Render a resume to print-ready A4 HTML, exactly what the app prints. With save=true, writes resumes/<slug>/resume.html and returns the path.", json!({ "slug": slug, "save": { "type": "boolean" } }), &["slug"], false),
        tool("read_skill", "Load one of ResuFlow's built-in writing skills (role-research, impact-bullets, ats-pass, recruiter-review).", json!({ "name": { "type": "string" } }), &["name"], true),
    ]
}
