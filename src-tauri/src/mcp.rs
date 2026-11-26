//! Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0).
//!
//! Agents launch `resuflow mcp` as a subprocess. There is no network listener, port or token:
//! the agent reaches exactly what its own process can, and nothing else can reach it.
//! stdout carries JSON-RPC only; diagnostics go to stderr.
//!
//! Tools mirror the app's actions. The profile, instructions, resumes and built-in skills are also
//! readable as resources, and the skills as prompts. `initialize` hands the agent the user's
//! instructions up front, and `completion/complete` suggests resume slugs and skill names.

use crate::config;
use crate::skills;
use crate::workspace::{NewResume, Workspace, STATUSES};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

const PROFILE_URI: &str = "resuflow://profile";
const INSTRUCTIONS_URI: &str = "resuflow://instructions";
const RESUME_PREFIX: &str = "resuflow://resumes/";
const RESUME_TEMPLATE: &str = "resuflow://resumes/{slug}";
const SKILL_TEMPLATE: &str = "skill://{name}/SKILL.md";
/// Most completion values one reply carries (the protocol's limit).
const MAX_COMPLETIONS: usize = 100;

const SUPPORTED: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "ResuFlow keeps the user's resumes as Markdown files in a local workspace. \
Start with get_workspace: it returns the default template's name and headline, the user's instructions, every resume and the skills. \
profile.mdx is the default template, copied into each new resume. Mine it for facts; never invent experience or metrics. \
The user's instructions are rules for resume work: follow them. Only the user changes them, in the app. \
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
The same files are resources: resuflow://profile, resuflow://instructions, resuflow://resumes/{slug} and skill://{name}/SKILL.md. \
Load a skill with read_skill (or the matching prompt or resource) before researching a role, writing bullets, an ATS pass or a critique.";

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
                    "capabilities": { "tools": {}, "prompts": {}, "resources": {}, "completions": {} },
                    "serverInfo": {
                        "name": "resuflow",
                        "title": "ResuFlow",
                        "version": env!("CARGO_PKG_VERSION"),
                        "description": "Local-first resume builder: read, tailor, check and render the user's Markdown resumes.",
                    },
                    "instructions": self.instructions(),
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
            "resources/list" => self.resources_list(),
            "resources/templates/list" => Ok(resource_templates()),
            "resources/read" => self.resources_read(params["uri"].as_str().unwrap_or_default()),
            "completion/complete" => Ok(self.complete(&params)),
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, m)) => error(id, code, &m),
        })
    }

    fn workspace(&self) -> Result<Workspace, (i64, String)> {
        config::workspace(self.dir.clone()).map_err(|e| (-32603, e.0))
    }

    /// The fixed server instructions, then the user's own instructions, scoped to resume work.
    fn instructions(&self) -> String {
        let rules = self.workspace().ok().and_then(|ws| ws.read_instructions().ok()).map(|i| i.items).unwrap_or_default();
        if rules.is_empty() {
            return INSTRUCTIONS.to_string();
        }
        let mut s = format!(
            "{INSTRUCTIONS}\n\nThe user's instructions. Follow them whenever you read, write, tailor or review their \
             ResuFlow resumes, template or postings; they say nothing about other work. These are as they stood when you \
             connected; get_workspace returns the current ones.\n"
        );
        for (i, rule) in rules.iter().enumerate() {
            s.push_str(&format!("\n{}. {}", i + 1, rule.replace('\n', "\n   ")));
        }
        s
    }

    fn resources_list(&self) -> RpcResult {
        let ws = self.workspace()?;
        let doc = |uri: &str, name: &str, title: &str, description: &str| {
            json!({ "uri": uri, "name": name, "title": title, "description": description, "mimeType": "text/markdown" })
        };
        let mut out = vec![
            doc(PROFILE_URI, "profile", "Default template", "profile.mdx: the source each new resume starts as."),
            doc(INSTRUCTIONS_URI, "instructions", "Instructions", "The user's rules for resume work. Read-only."),
        ];
        for r in ws.list_resumes().map_err(|e| (-32603, e.0))? {
            out.push(doc(&format!("{RESUME_PREFIX}{}", r.slug), &r.slug, &r.title, &format!("Resume ({}).", r.status)));
        }
        for s in skills::list() {
            out.push(doc(&format!("skill://{}/SKILL.md", s.name), &s.name, &s.name, &s.description));
        }
        Ok(json!({ "resources": out }))
    }

    fn resources_read(&self, uri: &str) -> RpcResult {
        let not_found = |why: String| (-32002, format!("resource not found: {uri} ({why})"));
        let text = if let Some(name) = uri.strip_prefix("skill://").and_then(|r| r.strip_suffix("/SKILL.md")) {
            skills::get(name).map(|s| s.content).ok_or_else(|| not_found(format!("no skill named '{name}'")))?
        } else if uri == PROFILE_URI {
            self.workspace()?.read_profile().map_err(|e| (-32603, e.0))?.content
        } else if uri == INSTRUCTIONS_URI {
            self.workspace()?.instructions_markdown().map_err(|e| (-32603, e.0))?
        } else if let Some(slug) = uri.strip_prefix(RESUME_PREFIX) {
            self.workspace()?.read_resume(slug).map_err(|e| not_found(e.0))?.content
        } else {
            return Err(not_found("unknown URI".into()));
        };
        Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": text }] }))
    }

    /// Suggest resume slugs (most recently edited first) for a prompt's `slug` argument or the
    /// resume template, and skill names for the skill template. Anything else gets no suggestions.
    fn complete(&self, p: &Value) -> Value {
        let r = &p["ref"];
        let arg = p["argument"]["name"].as_str().unwrap_or_default();
        let typed = p["argument"]["value"].as_str().unwrap_or_default().to_lowercase();
        let slugs = || self.workspace().map(|ws| ws.list_resumes().unwrap_or_default().into_iter().map(|r| r.slug).collect()).unwrap_or_default();
        let candidates: Vec<String> = match (r["type"].as_str(), arg) {
            (Some("ref/prompt"), "slug") if skills::get(r["name"].as_str().unwrap_or_default()).is_some() => slugs(),
            (Some("ref/resource"), "slug") if r["uri"] == RESUME_TEMPLATE => slugs(),
            (Some("ref/resource"), "name") if r["uri"] == SKILL_TEMPLATE => skills::list().into_iter().map(|s| s.name).collect(),
            _ => Vec::new(),
        };
        let matches: Vec<String> = candidates.into_iter().filter(|c| c.to_lowercase().starts_with(&typed)).collect();
        let total = matches.len();
        let values: Vec<String> = matches.into_iter().take(MAX_COMPLETIONS).collect();
        json!({ "completion": { "values": values, "total": total, "hasMore": total > MAX_COMPLETIONS } })
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
            "archive_resume" => j(ws.archive_resume(req("slug")?)),
            "restore_resume" => j(ws.restore_resume(req("slug")?)),
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

fn resource_templates() -> Value {
    json!({ "resourceTemplates": [
        { "uriTemplate": RESUME_TEMPLATE, "name": "resume", "title": "Resume", "description": "A resume's full source: front matter and Markdown body.", "mimeType": "text/markdown" },
        { "uriTemplate": SKILL_TEMPLATE, "name": "skill", "title": "Skill", "description": "One of ResuFlow's built-in writing skills.", "mimeType": "text/markdown" },
    ] })
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
}

fn tool(name: &str, description: &str, props: Value, required: &[&str], kind: Kind) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required, "additionalProperties": false },
        "annotations": {
            "readOnlyHint": matches!(kind, Kind::Read),
            "destructiveHint": false, // nothing over MCP erases data: resumes are archived, not deleted
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
    let skill_names: Vec<String> = skills::list().into_iter().map(|s| s.name).collect();
    vec![
        tool("get_workspace", "Overview: data folder, template name and headline, the user's instructions, all resumes, skills. Call first.", json!({}), &[], Read),
        tool("read_profile", "Read the default template (profile.mdx): the source, header included, that each new resume starts as.", json!({}), &[], Read),
        tool("update_profile", "Replace the default template (profile.mdx) entirely.", json!({ "content": text, "expected_hash": hash }), &["content"], Write),
        tool("list_resumes", "List resumes, most recently edited first, optionally filtered by status or tag.", json!({ "status": status, "tag": text }), &[], Read),
        tool("read_resume", "Read a resume's full Markdown source, metadata and hash.", json!({ "slug": slug }), &["slug"], Read),
        tool("create_resume", "Create a resume. Use `from` to copy an existing one (the usual way to tailor a variant; its posting is copied too), or `content` for full Markdown. Without either it starts from the default template. The slug is derived from the title if omitted.", json!({ "title": text, "slug": slug, "from": { "type": "string", "description": "Slug to copy" }, "content": text, "tags": tags }), &["title"], Write),
        tool("write_resume", "Replace a resume's full source (front matter and body). Prefer edit_resume for partial changes.", json!({ "slug": slug, "content": text, "expected_hash": hash }), &["slug", "content"], Write),
        tool("edit_resume", "Replace one exact, unique occurrence of old_text with new_text in a resume's source.", json!({ "slug": slug, "old_text": text, "new_text": text, "expected_hash": hash }), &["slug", "old_text", "new_text"], Write),
        tool("set_resume_meta", "Change a resume's title, status or tags.", json!({ "slug": slug, "title": text, "status": status, "tags": tags, "expected_hash": hash }), &["slug"], Write),
        tool("archive_resume", "Archive a resume: it moves to the user's Archive and keeps every file. Undo with restore_resume. Resumes can't be deleted over MCP; only the user can do that in the app.", json!({ "slug": slug }), &["slug"], Write),
        tool("restore_resume", "Bring an archived resume back as a draft. list_resumes with status=archived shows the archive.", json!({ "slug": slug }), &["slug"], Write),
        tool("read_posting", "Read the job posting a resume targets (empty if none).", json!({ "slug": slug }), &["slug"], Read),
        tool("set_posting", "Save the job posting a resume targets, as plain text or Markdown. Empty content removes it. check_resume then reports keyword coverage.", json!({ "slug": slug, "content": text, "expected_hash": hash }), &["slug", "content"], Write),
        tool("check_resume", "Deterministic checks: missing contact, ATS-hostile tables or images, weak or repetitive openers, unquantified or overlong bullets, leftovers, length, and keyword coverage against the target posting. Line numbers refer to resume.mdx.", json!({ "slug": slug }), &["slug"], Read),
        tool("render_resume", "Render a resume to print-ready A4 HTML, exactly what the app exports. With save=true, writes resumes/<slug>/resume.html and returns its path.", json!({ "slug": slug, "save": { "type": "boolean" } }), &["slug"], Write),
        tool("read_skill", "Load one of ResuFlow's built-in writing skills.", json!({ "name": { "type": "string", "enum": skill_names } }), &["name"], Read),
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

        let r = call(&s, "archive_resume", json!({ "slug": "platform-engineer" }));
        assert_eq!(r["structuredContent"]["status"], "archived");
        let r = call(&s, "restore_resume", json!({ "slug": "platform-engineer" }));
        assert_eq!(r["structuredContent"]["status"], "draft");
        assert_eq!(call(&s, "delete_resume", json!({ "slug": "platform-engineer" }))["isError"], true); // agents never delete

        let r = call(&s, "render_resume", json!({ "slug": "platform-engineer", "save": true }));
        assert!(t.0.root.join("resumes/platform-engineer/resume.html").is_file(), "{r}");
        assert_eq!(call(&s, "nope", json!({}))["isError"], true);
    }

    fn rpc(s: &Server, method: &str, params: Value) -> Value {
        s.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap()
    }

    #[test]
    fn initialize_carries_the_users_instructions() {
        let t = TempWs::new();
        let s = Server { dir: Some(t.0.root.clone()) };
        let init = rpc(&s, "initialize", json!({}))["result"].clone();
        assert_eq!(init["instructions"], INSTRUCTIONS);
        assert!(init["serverInfo"]["description"].is_string());
        assert!(init["capabilities"]["resources"].is_object() && init["capabilities"]["completions"].is_object());

        let h = t.0.read_instructions().unwrap().hash;
        t.0.write_instructions(&["Lead with the target role.".into(), "No tables.\nEver.".into()], Some(&h)).unwrap();
        let text = rpc(&s, "initialize", json!({}))["result"]["instructions"].as_str().unwrap().to_string();
        assert!(text.starts_with(INSTRUCTIONS));
        assert!(text.contains("ResuFlow resumes"), "{text}");
        assert!(text.ends_with("1. Lead with the target role.\n2. No tables.\n   Ever."), "{text}");
    }

    #[test]
    fn tools_drop_read_instructions_and_enumerate_skills() {
        let tools = tools();
        assert!(!tools.iter().any(|t| t["name"] == "read_instructions"));
        let read_skill = tools.iter().find(|t| t["name"] == "read_skill").unwrap();
        let names = read_skill["inputSchema"]["properties"]["name"]["enum"].as_array().unwrap();
        assert_eq!(names.len(), skills::list().len());
        assert!(names.contains(&json!("ats-pass")));
    }

    #[test]
    fn resources_list_and_read() {
        let t = TempWs::new();
        let s = Server { dir: Some(t.0.root.clone()) };
        call(&s, "create_resume", json!({ "title": "Platform Engineer" }));

        let list = rpc(&s, "resources/list", json!({}))["result"]["resources"].clone();
        let uris: Vec<&str> = list.as_array().unwrap().iter().map(|r| r["uri"].as_str().unwrap()).collect();
        for u in [PROFILE_URI, INSTRUCTIONS_URI, "resuflow://resumes/platform-engineer", "skill://ats-pass/SKILL.md"] {
            assert!(uris.contains(&u), "{u} missing from {uris:?}");
        }
        let templates = rpc(&s, "resources/templates/list", json!({}))["result"]["resourceTemplates"].clone();
        assert_eq!(templates.as_array().unwrap().len(), 2);

        let read = |uri: &str| rpc(&s, "resources/read", json!({ "uri": uri }));
        let resume = read("resuflow://resumes/platform-engineer");
        assert_eq!(resume["result"]["contents"][0]["text"], t.0.read_resume("platform-engineer").unwrap().content);
        assert_eq!(read(PROFILE_URI)["result"]["contents"][0]["text"], t.0.read_profile().unwrap().content);
        assert!(read(INSTRUCTIONS_URI)["result"]["contents"][0]["text"].as_str().unwrap().starts_with("# Instructions"));
        assert_eq!(read("skill://ats-pass/SKILL.md")["result"]["contents"][0]["text"], skills::get("ats-pass").unwrap().content);

        for bad in ["resuflow://resumes/nope", "resuflow://resumes/../profile", "skill://nope/SKILL.md", "file:///etc/passwd"] {
            assert_eq!(read(bad)["error"]["code"], -32002, "{bad}");
        }
    }

    #[test]
    fn completion_suggests_slugs_and_skills() {
        let t = TempWs::new();
        let s = Server { dir: Some(t.0.root.clone()) };
        for title in ["Platform Engineer", "Platform Lead", "Data Scientist"] {
            call(&s, "create_resume", json!({ "title": title }));
        }
        let complete = |r: Value, arg: &str, value: &str| {
            rpc(&s, "completion/complete", json!({ "ref": r, "argument": { "name": arg, "value": value } }))["result"]["completion"].clone()
        };
        let c = complete(json!({ "type": "ref/prompt", "name": "ats-pass" }), "slug", "plat");
        let mut got: Vec<&str> = c["values"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        got.sort();
        assert_eq!(got, ["platform-engineer", "platform-lead"]);
        assert_eq!(c["total"], 2);
        assert_eq!(c["hasMore"], false);
        let c = complete(json!({ "type": "ref/resource", "uri": RESUME_TEMPLATE }), "slug", "");
        assert_eq!(c["total"], 3);
        let c = complete(json!({ "type": "ref/resource", "uri": SKILL_TEMPLATE }), "name", "Imp");
        assert_eq!(c["values"], json!(["impact-bullets"]));
        let c = complete(json!({ "type": "ref/prompt", "name": "nope" }), "slug", "");
        assert_eq!(c["values"], json!([]));
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
