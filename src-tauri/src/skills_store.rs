use crate::markdown::parse_frontmatter;
use crate::workspace::{
    assert_slug, atomic_write_file, is_valid_slug, slugify, WorkspaceError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const BUILTIN_SKILLS: &[(&str, &str)] = &[
    (
        "ats-optimizer",
        include_str!("../../skills/ats-optimizer/SKILL.md"),
    ),
    (
        "impact-writer",
        include_str!("../../skills/impact-writer/SKILL.md"),
    ),
    (
        "resume-critic",
        include_str!("../../skills/resume-critic/SKILL.md"),
    ),
    (
        "resume-research",
        include_str!("../../skills/resume-research/SKILL.md"),
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Skill {
    pub slug: String,
    pub source: String,
    pub name: String,
    pub description: String,
    pub markdown: String,
    pub body: String,
    pub enabled: bool,
}

pub fn list_skills(workspace_root: &Path, include_disabled: bool) -> Result<Vec<Skill>, WorkspaceError> {
    let disabled = load_disabled(workspace_root);
    let mut by_slug: BTreeMap<String, Skill> = BTreeMap::new();

    for (slug, md) in BUILTIN_SKILLS {
        let mut skill = parse_skill_markdown(md, slug, "builtin");
        skill.enabled = !disabled.contains(*slug);
        by_slug.insert((*slug).to_string(), skill);
    }

    for mut skill in read_user_skill_dir(&workspace_root.join("skills")) {
        skill.enabled = !disabled.contains(&skill.slug);
        by_slug.insert(skill.slug.clone(), skill);
    }

    let mut skills: Vec<Skill> = by_slug
        .into_values()
        .filter(|s| include_disabled || s.enabled)
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

pub fn load_skill(workspace_root: &Path, slug: &str) -> Result<Skill, WorkspaceError> {
    assert_slug(slug, "skill")?;
    let disabled = load_disabled(workspace_root);
    let user_file = workspace_root.join("skills").join(slug).join("SKILL.md");
    if user_file.exists() {
        let md = fs::read_to_string(&user_file)
            .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
        let mut skill = parse_skill_markdown(&md, slug, "user");
        skill.enabled = !disabled.contains(slug);
        return Ok(skill);
    }

    if let Some((_, md)) = BUILTIN_SKILLS.iter().find(|(s, _)| *s == slug) {
        let mut skill = parse_skill_markdown(md, slug, "builtin");
        skill.enabled = !disabled.contains(slug);
        return Ok(skill);
    }

    Err(WorkspaceError::not_found(format!("Skill not found: {slug}")))
}

pub fn set_skill_enabled(
    workspace_root: &Path,
    slug: &str,
    enabled: bool,
) -> Result<Skill, WorkspaceError> {
    assert_slug(slug, "skill")?;
    load_skill(workspace_root, slug)?;
    let mut disabled = load_disabled(workspace_root);
    if enabled {
        disabled.remove(slug);
    } else {
        disabled.insert(slug.to_string());
    }
    save_disabled(workspace_root, &disabled)?;
    load_skill(workspace_root, slug)
}

pub fn create_skill(workspace_root: &Path, input: &Value) -> Result<Skill, WorkspaceError> {
    let name = input
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(WorkspaceError::bad_request("A skill name is required."));
    }
    let slug_source = input
        .get("slug")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&name);
    let slug = slugify(slug_source);
    assert_slug(&slug, "skill")?;

    if BUILTIN_SKILLS.iter().any(|(s, _)| *s == slug) {
        return Err(WorkspaceError::conflict(format!(
            "\"{slug}\" is a built-in skill. Choose another name."
        )));
    }

    let file = workspace_root.join("skills").join(&slug).join("SKILL.md");
    if file.exists() {
        return Err(WorkspaceError::conflict(format!(
            "Skill already exists: {slug}"
        )));
    }

    let custom_md = input
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let markdown = if custom_md.is_empty() {
        starter_skill(&name)
    } else {
        custom_md.to_string()
    };

    atomic_write_file(&file, &markdown)?;
    Ok(parse_skill_markdown(&markdown, &slug, "user"))
}

pub fn save_skill(
    workspace_root: &Path,
    slug: &str,
    input: &Value,
) -> Result<Skill, WorkspaceError> {
    assert_slug(slug, "skill")?;
    let current = load_skill(workspace_root, slug)?;
    if current.source == "builtin" {
        return Err(WorkspaceError::forbidden(
            "Built-in skills cannot be edited. Duplicate it as a custom skill.",
        ));
    }
    let markdown = input
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if markdown.is_empty() {
        return Err(WorkspaceError::bad_request("Skill Markdown is required."));
    }
    let file = workspace_root.join("skills").join(slug).join("SKILL.md");
    atomic_write_file(&file, &markdown)?;
    let mut updated = parse_skill_markdown(&markdown, slug, "user");
    updated.enabled = current.enabled;
    Ok(updated)
}

pub fn delete_skill(workspace_root: &Path, slug: &str) -> Result<(), WorkspaceError> {
    assert_slug(slug, "skill")?;
    let current = load_skill(workspace_root, slug)?;
    if current.source == "builtin" {
        return Err(WorkspaceError::forbidden(
            "Built-in skills cannot be deleted.",
        ));
    }
    let dir = workspace_root.join("skills").join(slug);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
    }
    Ok(())
}

fn read_user_skill_dir(dir: &Path) -> Vec<Skill> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let slug = entry.file_name().to_string_lossy().to_string();
        if !is_valid_slug(&slug) {
            continue;
        }
        let skill_file = entry.path().join("SKILL.md");
        if let Ok(md) = fs::read_to_string(&skill_file) {
            skills.push(parse_skill_markdown(&md, &slug, "user"));
        }
    }
    skills
}

fn parse_skill_markdown(markdown: &str, slug: &str, source: &str) -> Skill {
    let parsed = parse_frontmatter(markdown);
    let heading = parsed
        .body
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(|h| h.trim().to_string()));
    let paragraph = parsed
        .body
        .split("\n\n")
        .map(|block| block.trim())
        .find(|block| !block.is_empty() && !block.starts_with('#'))
        .map(|p| p.replace('\n', " ").chars().take(180).collect::<String>());

    let name = parsed
        .meta
        .get("name")
        .filter(|s| !s.is_empty())
        .cloned()
        .or(heading)
        .unwrap_or_else(|| slug.to_string());

    let description = parsed
        .meta
        .get("description")
        .filter(|s| !s.is_empty())
        .cloned()
        .or(paragraph)
        .unwrap_or_default();

    Skill {
        slug: slug.to_string(),
        source: source.to_string(),
        name,
        description,
        markdown: markdown.to_string(),
        body: parsed.body,
        enabled: true,
    }
}

fn starter_skill(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Describe when the agent should use this skill.\n---\n\n# {name}\n\nInstructions for the resume agent. Be specific about process, tone, and what not to invent.\n"
    )
}

fn disabled_file(workspace_root: &Path) -> PathBuf {
    workspace_root.join("skills").join("disabled.json")
}

fn load_disabled(workspace_root: &Path) -> BTreeSet<String> {
    let file = disabled_file(workspace_root);
    let Ok(text) = fs::read_to_string(&file) else {
        return BTreeSet::new();
    };
    let Ok(payload) = serde_json::from_str::<Value>(&text) else {
        return BTreeSet::new();
    };
    let list = if let Some(arr) = payload.as_array() {
        Some(arr)
    } else {
        payload.get("disabled").and_then(|v| v.as_array())
    };
    list.map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect()
    })
    .unwrap_or_default()
}

fn save_disabled(workspace_root: &Path, disabled: &BTreeSet<String>) -> Result<(), WorkspaceError> {
    let file = disabled_file(workspace_root);
    let list: Vec<&String> = disabled.iter().collect();
    let payload = serde_json::to_string_pretty(&json!({ "disabled": list }))
        .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
    atomic_write_file(&file, &format!("{payload}\n"))
}
