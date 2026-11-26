//! Built-in writing skills. They ship inside the binary and are read-only: agents load them
//! over MCP (tools, prompts and resources).

use crate::doc::Doc;
use serde::Serialize;

const SKILLS: [(&str, &str); 4] = [
    ("role-research", include_str!("../../skills/role-research/SKILL.md")),
    ("impact-bullets", include_str!("../../skills/impact-bullets/SKILL.md")),
    ("ats-pass", include_str!("../../skills/ats-pass/SKILL.md")),
    ("recruiter-review", include_str!("../../skills/recruiter-review/SKILL.md")),
];

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

#[cfg(test)]
mod tests {
    #[test]
    fn skills_have_descriptions() {
        for s in super::list() {
            assert!(!s.description.is_empty(), "{} lacks a description", s.name);
        }
    }
}
