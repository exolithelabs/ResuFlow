use crate::memory::{append_memory, load_memory, save_memory};
use crate::releases::CURRENT_VERSION;
use crate::skills_store::{create_skill, delete_skill, list_skills, load_skill, save_skill};
use crate::workspace::{
    create_resume, list_resumes, load_profile, load_resume, save_profile, save_resume,
    WorkspaceError,
};
use serde_json::{json, Value};
use std::path::Path;

pub fn mcp_snippet(url: &str, token: &str) -> Value {
    json!({
        "url": url,
        "config": {
            "mcpServers": {
                "resuflow": {
                    "url": url,
                    "headers": {
                        "Authorization": format!("Bearer {token}"),
                    }
                }
            }
        }
    })
}

/// Handles a JSON-RPC 2.0 MCP request over Streamable HTTP.
/// Returns `Ok(Some(json))` for requests with an `id`, or `Ok(None)` for notifications.
pub fn handle_mcp_payload(body: &Value, workspace_root: &Path) -> Result<Option<Value>, WorkspaceError> {
    if let Some(arr) = body.as_array() {
        let mut responses = Vec::new();
        for item in arr {
            if let Some(resp) = handle_single_rpc(item, workspace_root) {
                responses.push(resp);
            }
        }
        if responses.is_empty() {
            return Ok(None);
        }
        return Ok(Some(Value::Array(responses)));
    }
    Ok(handle_single_rpc(body, workspace_root))
}

fn handle_single_rpc(req: &Value, workspace_root: &Path) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    if method.starts_with("notifications/") || id.is_none() {
        return None;
    }
    let id = id.unwrap_or(Value::Null);

    match dispatch_method(method, &params, workspace_root) {
        Ok(result) => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })),
        Err((code, message)) => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message,
            }
        })),
    }
}

fn dispatch_method(
    method: &str,
    params: &Value,
    workspace_root: &Path,
) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {},
                "prompts": {},
            },
            "serverInfo": {
                "name": "resuflow",
                "version": CURRENT_VERSION,
            }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": tool_definitions(),
        })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| (-32602, "Missing tool name".to_string()))?;
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(name, &args, workspace_root) {
                Ok(output) => Ok(text_content(&output, false)),
                Err(err) => Ok(text_content(&json!({ "error": err.message }), true)),
            }
        }
        "prompts/list" => Ok(json!({
            "prompts": [{
                "name": "draft_resume",
                "description": "Draft or improve a role-specific resume using workspace files.",
                "arguments": [
                    { "name": "slug", "description": "Resume slug to edit", "required": true },
                    { "name": "role", "description": "Target role", "required": false }
                ]
            }]
        })),
        "prompts/get" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name != "draft_resume" {
                return Err((-32602, format!("Unknown prompt: {name}")));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let slug = args.get("slug").and_then(|v| v.as_str()).unwrap_or("");
            let role = args
                .get("role")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("the target role");
            Ok(json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!("Use the resuflow MCP tools. Read profile, memory, skills, and resume \"{slug}\". Improve that resume for {role} without inventing employers, dates, or metrics.")
                    }
                }]
            }))
        }
        _ => Err((-32601, format!("Method not found: {method}"))),
    }
}

fn text_content(value: &Value, is_error: bool) -> Value {
    let text = if let Some(s) = value.as_str() {
        s.to_string()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    };
    let mut obj = json!({
        "content": [{
            "type": "text",
            "text": text,
        }]
    });
    if is_error {
        obj["isError"] = json!(true);
    }
    obj
}

fn call_tool(name: &str, args: &Value, workspace_root: &Path) -> Result<Value, WorkspaceError> {
    let req_str = |key: &str| -> Result<String, WorkspaceError> {
        args.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| WorkspaceError::bad_request(format!("Missing required argument: {key}")))
    };

    match name {
        "list_resumes" => {
            let resumes = list_resumes(workspace_root)?;
            Ok(serde_json::to_value(resumes).unwrap_or_else(|_| json!([])))
        }
        "get_resume" => {
            let slug = req_str("slug")?;
            let resume = load_resume(workspace_root, &slug)?;
            Ok(serde_json::to_value(resume).unwrap_or_else(|_| json!({})))
        }
        "write_resume" => {
            let slug = req_str("slug")?;
            let markdown = req_str("markdown")?;
            let saved = save_resume(workspace_root, &slug, &json!({ "markdown": markdown }))?;
            Ok(serde_json::to_value(saved).unwrap_or_else(|_| json!({})))
        }
        "create_resume" => {
            let title = req_str("title")?;
            let created = create_resume(workspace_root, &json!({ "title": title }))?;
            Ok(serde_json::to_value(created).unwrap_or_else(|_| json!({})))
        }
        "get_profile" => {
            let profile = load_profile(workspace_root)?;
            Ok(serde_json::to_value(profile).unwrap_or_else(|_| json!({})))
        }
        "write_profile" => {
            let markdown = req_str("markdown")?;
            let saved = save_profile(workspace_root, &json!({ "markdown": markdown }))?;
            Ok(serde_json::to_value(saved).unwrap_or_else(|_| json!({})))
        }
        "list_skills" => {
            let skills = list_skills(workspace_root, false)?;
            let summary: Vec<Value> = skills
                .into_iter()
                .map(|s| {
                    json!({
                        "slug": s.slug,
                        "name": s.name,
                        "description": s.description,
                        "source": s.source,
                        "enabled": s.enabled,
                    })
                })
                .collect();
            Ok(Value::Array(summary))
        }
        "get_skill" => {
            let slug = req_str("slug")?;
            let skill = load_skill(workspace_root, &slug)?;
            Ok(serde_json::to_value(skill).unwrap_or_else(|_| json!({})))
        }
        "create_skill" => {
            let skill_name = req_str("name")?;
            let markdown = args.get("markdown").and_then(|v| v.as_str()).unwrap_or("");
            let created = create_skill(
                workspace_root,
                &json!({ "name": skill_name, "markdown": markdown }),
            )?;
            Ok(serde_json::to_value(created).unwrap_or_else(|_| json!({})))
        }
        "write_skill" => {
            let slug = req_str("slug")?;
            let markdown = req_str("markdown")?;
            let saved = save_skill(workspace_root, &slug, &json!({ "markdown": markdown }))?;
            Ok(serde_json::to_value(saved).unwrap_or_else(|_| json!({})))
        }
        "delete_skill" => {
            let slug = req_str("slug")?;
            delete_skill(workspace_root, &slug)?;
            Ok(json!({ "ok": true, "slug": slug }))
        }
        "get_memory" => {
            let mem = load_memory(workspace_root)?;
            Ok(serde_json::to_value(mem).unwrap_or_else(|_| json!({})))
        }
        "write_memory" => {
            let markdown = req_str("markdown")?;
            let saved = save_memory(workspace_root, &json!({ "markdown": markdown }))?;
            Ok(serde_json::to_value(saved).unwrap_or_else(|_| json!({})))
        }
        "append_memory" => {
            let note = req_str("note")?;
            let saved = append_memory(workspace_root, &note)?;
            Ok(serde_json::to_value(saved).unwrap_or_else(|_| json!({})))
        }
        _ => Err(WorkspaceError::bad_request(format!("Unknown tool: {name}"))),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "list_resumes",
            "description": "List resumes in the local workspace.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_resume",
            "description": "Read a resume Markdown file by slug.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Resume slug" } },
                "required": ["slug"]
            }
        },
        {
            "name": "write_resume",
            "description": "Overwrite a resume Markdown file. Include YAML frontmatter.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "markdown": { "type": "string" }
                },
                "required": ["slug", "markdown"]
            }
        },
        {
            "name": "create_resume",
            "description": "Create a new resume from a title.",
            "inputSchema": {
                "type": "object",
                "properties": { "title": { "type": "string" } },
                "required": ["title"]
            }
        },
        {
            "name": "get_profile",
            "description": "Read shared profile Markdown.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "write_profile",
            "description": "Overwrite shared profile Markdown.",
            "inputSchema": {
                "type": "object",
                "properties": { "markdown": { "type": "string" } },
                "required": ["markdown"]
            }
        },
        {
            "name": "list_skills",
            "description": "List enabled built-in and user skills.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_skill",
            "description": "Read a skill by slug.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "create_skill",
            "description": "Create a custom user skill.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "markdown": { "type": "string" }
                },
                "required": ["name"]
            }
        },
        {
            "name": "write_skill",
            "description": "Update a custom skill Markdown file. Built-in skills cannot be edited.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "markdown": { "type": "string" }
                },
                "required": ["slug", "markdown"]
            }
        },
        {
            "name": "delete_skill",
            "description": "Delete a custom skill. Built-in skills cannot be deleted.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "get_memory",
            "description": "Read durable memory notes.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "write_memory",
            "description": "Replace memory Markdown.",
            "inputSchema": {
                "type": "object",
                "properties": { "markdown": { "type": "string" } },
                "required": ["markdown"]
            }
        },
        {
            "name": "append_memory",
            "description": "Append a dated note to memory.",
            "inputSchema": {
                "type": "object",
                "properties": { "note": { "type": "string" } },
                "required": ["note"]
            }
        }
    ])
}
