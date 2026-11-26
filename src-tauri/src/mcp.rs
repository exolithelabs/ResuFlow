//! Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0).
//!
//! Agents launch `resuflow mcp` as a subprocess. There is no network listener, port or token:
//! the agent reaches exactly what its own process can, and nothing else can reach it.
//! stdout carries JSON-RPC only; diagnostics go to stderr.
//!
//! Tools mirror the app's actions. The built-in skills are also exposed as MCP prompts.

use crate::config;
use crate::skills;
use crate::workspace::{NewResume, Workspace, STATUSES};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

const SUPPORTED: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "ResuFlow keeps the user's resumes as Markdown files in a local workspace. \
Start with get_workspace: it returns the default template's name and headline, the user's instructions, every resume and the skills. \
profile.mdx is the default template, copied into each new resume. Mine it for facts; never invent experience or metrics. \
The user's instructions are rules: follow them. Only the user changes them, in the app. \
Each resume is resumes/<slug>/resume.mdx: front matter (title, status, tags) and a Markdown body. The body opens with the header: \
a \"# Name\" line, a headline line under it, then a contact line (City · email · phone · links, separated by \" · \"). \
Then ## sections, each followed by a --- line that prints as the rule under it, ### Role — Company headings, \
an *dates · place* line and - bullets. \
Two components add styling, nothing else is MDX: <Text color=\"#1f4e8c\" size=\"10pt\" caps spacing=\"0.08em\">words</Text> \
(attributes: color, underline, underlineColor, size, caps, spacing, font=serif|sans|mono) and <Rule color=\"#d6d9de\" /> \
on its own line for a coloured rule. Attributes are quoted strings or bare booleans; colours are hex, CSS names or rgb()/hsl(), \
sizes are lengths such as 9.5pt or 0.08em. Keep the styling a resume already has when you edit it. No imports, exports or {expressions}. \
A resume can have a target job posting (read_posting / set_posting); check_resume then reports which of its key terms the resume covers. \
Prefer edit_resume for targeted changes and pass the expected_hash from your last read, so you never overwrite the user's \
concurrent edits in the desktop app. Run check_resume after editing. \
Load a skill with read_skill (or the matching prompt) before researching a role, writing bullets, an ATS pass or a critique.";

pub fn serve(dir: Option<PathBuf>) -> io::Result<()> {
    let server = Server { dir };
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let out: Vec<Value> = batch.iter().filter_map(|m| server.handle(m)).collect();
                (!out.is_empty()).then_some(Value::Array(out))
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

type RpcResult = Result<Value, (i64, String)>;

impl Server {
    fn handle(&self, msg: &Value) -> Option<Value> {
        let method = msg.get("method")?.as_str()?;
        let id = msg.get("id").cloned()?; // notifications get no reply
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result: RpcResult = match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or(SUPPORTED[0]);
                Ok(json!({
                    "protocolVersion": if SUPPORTED.contains(&asked) { asked } else { SUPPORTED[0] },
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
                Ok(tool_result(self.call(name, &args)))
            }
            "prompts/list" => Ok(prompts_list()),
            "prompts/get" => prompts_get(&params),
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
        let ws: Workspace = config::workspace(self.dir.clone()).map_err(|e| e.0)?;
        let opt = |k: &str| a.get(k).and_then(Value::as_str);
        let req = |k: &str| opt(k).ok_or_else(|| format!("missing required argument `{k}`"));
        let hash = opt("expected_hash");
        let strings = |k: &str| a.get(k).and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>());

        match name {
            "get_workspace" => {
                let profile = ws.profile();
                let header = crate::render::split_header(&profile.body);
                Ok(json!({
                    "root": ws.root,
                    "profile": {
                        "name": header.as_ref().map(|h| crate::render::plain_text(h.name)),
                        "headline": header.as_ref().and_then(|h| h.headline.first().map(|l| crate::render::plain_text(l))),
                        "template_words": crate::workspace::count_words(&profile.body),
                    },
                    "instructions": ws.read_instructions().map_err(|e| e.0)?.items,
                    "resumes": j(ws.list_resumes())?,
                    "skills": skills::list().into_iter().map(|s| json!({ "name": s.name, "description": s.description })).collect::<Vec<_>>(),
                    "statuses": STATUSES,
                }))
            }
            "read_profile" => j(ws.read_profile()),
            "update_profile" => j(ws.write_profile(req("content")?, hash)),
            "read_instructions" => j(ws.read_instructions()),
            "list_resumes" => {
                let mut list = ws.list_resumes().map_err(|e| e.0)?;
                if let Some(st) = opt("status") {
                    list.retain(|r| r.status == st);
                }
                if let Some(t) = opt("tag") {
                    list.retain(|r| r.tags.iter().any(|x| x.eq_ignore_ascii_case(t)));
                }
                Ok(json!({ "resumes": list }))
            }
            "read_resume" => j(ws.read_resume(req("slug")?)),
            "create_resume" => j(ws.create_resume(NewResume {
                title: req("title")?,
                slug: opt("slug"),
                from: opt("from"),
                content: opt("content"),
                tags: strings("tags").unwrap_or_default(),
            })),
            "write_resume" => j(ws.write_resume(req("slug")?, req("content")?, hash)),
            "edit_resume" => j(ws.edit_resume(req("slug")?, req("old_text")?, opt("new_text").unwrap_or(""), hash)),
            "set_resume_meta" => j(ws.set_meta(req("slug")?, opt("title"), opt("status"), strings("tags"), hash)),
            "delete_resume" => j(ws.delete_resume(req("slug")?)),
            "list_trash" => Ok(json!({ "trash": j(ws.list_trash())? })),
            "restore_resume" => j(ws.restore_resume(req("id")?)),
            "read_posting" => j(ws.read_posting(req("slug")?)),
            "set_posting" => j(ws.write_posting(req("slug")?, req("content")?, hash)),
            "check_resume" => j(ws.check(req("slug")?)),
            "render_resume" => {
                let slug = req("slug")?;
                if a["save"].as_bool().unwrap_or(false) {
                    let path = ws.save_render(slug).map_err(|e| e.0)?;
                    Ok(json!({ "saved": path }))
                } else {
                    ws.render(slug).map(Value::String).map_err(|e| e.0)
                }
            }
            "read_skill" => {
                let name = req("name")?;
                skills::get(name).map(|s| Value::String(s.content)).ok_or_else(|| format!("no skill named '{name}'"))
            }
            _ => Err(format!("unknown tool: {name}")),
        }
    }
}

fn tool_result(r: Result<Value, String>) -> Value {
    match r {
        Ok(v) => {
            let text = match &v {
                Value::String(s) => s.clone(),
                other => serde_json::to_string_pretty(other).unwrap_or_default(),
            };
            let mut out = json!({ "content": [{ "type": "text", "text": text }], "isError": false });
            if v.is_object() {
                out["structuredContent"] = v;
            }
            out
        }
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

fn prompts_list() -> Value {
    let prompts: Vec<Value> = skills::list()
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "description": s.description,
                "arguments": [{ "name": "slug", "description": "Resume to apply it to (optional)", "required": false }],
            })
        })
        .collect();
    json!({ "prompts": prompts })
}

fn prompts_get(p: &Value) -> RpcResult {
    let name = p["name"].as_str().unwrap_or_default();
    let skill = skills::get(name).ok_or((-32602, format!("no prompt named '{name}'")))?;
    let mut text = skill.content;
    if let Some(slug) = p["arguments"]["slug"].as_str().filter(|s| !s.is_empty()) {
        text.push_str(&format!("\n\nApply this to the resume `{slug}` (read it with read_resume)."));
    }
    Ok(json!({
        "description": skill.description,
        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }],
    }))
}

fn j<T: serde::Serialize>(v: crate::workspace::Result<T>) -> Result<Value, String> {
    v.map_err(|e| e.0).map(|x| serde_json::to_value(x).unwrap_or(Value::Null))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[derive(Clone, Copy)]
enum Kind {
    Read,
    Write,
    /// Moves or removes something (recoverable, but worth a confirmation).
    Destructive,
}

fn tool(name: &str, description: &str, props: Value, required: &[&str], kind: Kind) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required, "additionalProperties": false },
        "annotations": {
            "readOnlyHint": matches!(kind, Kind::Read),
            "destructiveHint": matches!(kind, Kind::Destructive),
            "idempotentHint": matches!(kind, Kind::Read),
            "openWorldHint": false,
        },
    })
}

fn tools() -> Vec<Value> {
    use Kind::*;
    let slug = json!({ "type": "string", "description": "Resume slug, e.g. backend-acme" });
    let hash = json!({ "type": "string", "description": "Hash from your last read. If the file changed since, the write is refused instead of overwriting." });
    let tags = json!({ "type": "array", "items": { "type": "string" } });
    let status = json!({ "type": "string", "enum": STATUSES });
    let text = json!({ "type": "string" });
    vec![
        tool("get_workspace", "Overview: data folder, template name and headline, the user's instructions, all resumes, skills. Call first.", json!({}), &[], Read),
        tool("read_profile", "Read the default template (profile.mdx): the source, header included, that each new resume starts as.", json!({}), &[], Read),
        tool("update_profile", "Replace the default template (profile.mdx) entirely.", json!({ "content": text, "expected_hash": hash }), &["content"], Write),
        tool("read_instructions", "Read the user's instructions. Follow them. Only the user changes them, in the app.", json!({}), &[], Read),
        tool("list_resumes", "List resumes, most recently edited first, optionally filtered by status or tag.", json!({ "status": status, "tag": text }), &[], Read),
        tool("read_resume", "Read a resume's full Markdown source, metadata and hash.", json!({ "slug": slug }), &["slug"], Read),
        tool("create_resume", "Create a resume. Use `from` to copy an existing one (the usual way to tailor a variant; its posting is copied too), or `content` for full Markdown. Without either it starts from the default template. The slug is derived from the title if omitted.", json!({ "title": text, "slug": slug, "from": { "type": "string", "description": "Slug to copy" }, "content": text, "tags": tags }), &["title"], Write),
        tool("write_resume", "Replace a resume's full source (front matter and body). Prefer edit_resume for partial changes.", json!({ "slug": slug, "content": text, "expected_hash": hash }), &["slug", "content"], Write),
        tool("edit_resume", "Replace one exact, unique occurrence of old_text with new_text in a resume's source.", json!({ "slug": slug, "old_text": text, "new_text": text, "expected_hash": hash }), &["slug", "old_text", "new_text"], Write),
        tool("set_resume_meta", "Change a resume's title, status or tags.", json!({ "slug": slug, "title": text, "status": status, "tags": tags, "expected_hash": hash }), &["slug"], Write),
        tool("delete_resume", "Move a resume to the trash. It can be restored with restore_resume.", json!({ "slug": slug }), &["slug"], Destructive),
        tool("list_trash", "List deleted resumes, most recent first, with the id restore_resume needs.", json!({}), &[], Read),
        tool("restore_resume", "Bring a deleted resume back from the trash. If its slug was reused meanwhile, it gets a numbered one.", json!({ "id": { "type": "string", "description": "Trash id from list_trash" } }), &["id"], Write),
        tool("read_posting", "Read the job posting a resume targets (empty if none).", json!({ "slug": slug }), &["slug"], Read),
        tool("set_posting", "Save the job posting a resume targets, as plain text or Markdown. Empty content removes it. check_resume then reports keyword coverage.", json!({ "slug": slug, "content": text, "expected_hash": hash }), &["slug", "content"], Write),
        tool("check_resume", "Deterministic checks: missing contact, ATS-hostile tables or images, weak or repetitive openers, unquantified or overlong bullets, leftovers, length, and keyword coverage against the target posting. Line numbers refer to resume.mdx.", json!({ "slug": slug }), &["slug"], Read),
        tool("render_resume", "Render a resume to print-ready A4 HTML, exactly what the app exports. With save=true, writes resumes/<slug>/resume.html and returns its path.", json!({ "slug": slug, "save": { "type": "boolean" } }), &["slug"], Write),
        tool("read_skill", "Load one of ResuFlow's built-in writing skills (role-research, impact-bullets, ats-pass, recruiter-review).", json!({ "name": text }), &["name"], Read),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::TempWs;

    fn call(s: &Server, name: &str, args: Value) -> Value {
        let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } });
        s.handle(&msg).unwrap()["result"].clone()
    }

    #[test]
    fn tools_roundtrip() {
        let t = TempWs::new();
        let s = Server { dir: Some(t.0.root.clone()) };
        assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let init = s.handle(&json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");

        let r = call(&s, "create_resume", json!({ "title": "Platform Engineer" }));
        assert_eq!(r["isError"], false);
        let hash = r["structuredContent"]["hash"].as_str().unwrap().to_string();
        let r = call(&s, "edit_resume", json!({ "slug": "platform-engineer", "old_text": "- Tools:", "new_text": "- Tools: Kafka", "expected_hash": hash }));
        assert_eq!(r["isError"], false);
        let r = call(&s, "edit_resume", json!({ "slug": "platform-engineer", "old_text": "- Tools: Kafka", "new_text": "x", "expected_hash": hash }));
        assert_eq!(r["isError"], true);

        call(&s, "set_posting", json!({ "slug": "platform-engineer", "content": "We use Kafka and Terraform. Kafka at scale." }));
        let r = call(&s, "check_resume", json!({ "slug": "platform-engineer" }));
        let kw = &r["structuredContent"]["keywords"];
        assert!(kw["found"].as_array().unwrap().contains(&json!("kafka")));
        assert!(kw["missing"].as_array().unwrap().contains(&json!("terraform")));

        let r = call(&s, "delete_resume", json!({ "slug": "platform-engineer" }));
        let id = r["structuredContent"]["id"].as_str().unwrap().to_string();
        let r = call(&s, "restore_resume", json!({ "id": id }));
        assert_eq!(r["structuredContent"]["slug"], "platform-engineer");

        let r = call(&s, "render_resume", json!({ "slug": "platform-engineer", "save": true }));
        assert!(t.0.root.join("resumes/platform-engineer/resume.html").is_file(), "{r}");
        assert_eq!(call(&s, "nope", json!({}))["isError"], true);
    }

    #[test]
    fn every_tool_has_a_valid_schema() {
        for t in tools() {
            let props = t["inputSchema"]["properties"].as_object().unwrap();
            for r in t["inputSchema"]["required"].as_array().unwrap() {
                assert!(props.contains_key(r.as_str().unwrap()), "{} requires unknown {r}", t["name"]);
            }
        }
    }
}
