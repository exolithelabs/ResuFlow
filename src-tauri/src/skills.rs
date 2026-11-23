//! Built-in writing skills. They ship inside the binary and are read-only: agents load them
//! over MCP (tools and prompts), or the user installs copies into an agent's own skills folder.

use crate::doc::{Doc, Value};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

const SKILLS: [(&str, &str); 4] = [
    ("role-research", include_str!("../../skills/role-research/SKILL.md")),
    ("impact-bullets", include_str!("../../skills/impact-bullets/SKILL.md")),
    ("ats-pass", include_str!("../../skills/ats-pass/SKILL.md")),
    ("recruiter-review", include_str!("../../skills/recruiter-review/SKILL.md")),
];

/// Installed skill folders get this prefix so they never collide with the user's own skills.
const PREFIX: &str = "resuflow-";

#[derive(Serialize, Clone, Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
}

pub fn list() -> Vec<Skill> {
    SKILLS
        .iter()
        .map(|(name, content)| Skill {
            name: name.to_string(),
            description: Doc::parse(content).str("description").unwrap_or_default(),
            content: content.to_string(),
        })
        .collect()
}

pub fn get(name: &str) -> Option<Skill> {
    list().into_iter().find(|s| s.name == name)
}

/// Where an agent looks for user-level skills.
pub fn agent_dir(agent: &str) -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("could not find your home folder")?;
    match agent {
        "claude" => Ok(home.join(".claude").join("skills")),
        "agents" => Ok(home.join(".agents").join("skills")),
        _ => Err(format!("unknown agent '{agent}' (use claude or agents)")),
    }
}

/// Copy every built-in skill into an agent's skills folder as `resuflow-<name>/SKILL.md`.
/// Only ResuFlow's own folders are written; re-running updates them in place.
pub fn install(agent: &str) -> Result<PathBuf, String> {
    let root = agent_dir(agent)?;
    for (name, content) in SKILLS {
        let dir = root.join(format!("{PREFIX}{name}"));
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut doc = Doc::parse(content);
        doc.set("name", Value::Str(format!("{PREFIX}{name}")));
        doc.body.push_str(
            "\n---\n*Installed by ResuFlow. Needs the `resuflow` MCP server connected to read and edit resumes.*\n",
        );
        fs::write(dir.join("SKILL.md"), doc.to_string()).map_err(|e| e.to_string())?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    #[test]
    fn skills_have_descriptions() {
        for s in super::list() {
            assert!(!s.description.is_empty(), "{} lacks a description", s.name);
        }
    }
}
