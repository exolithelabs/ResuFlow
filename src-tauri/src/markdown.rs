use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMarkdown {
    pub meta: BTreeMap<String, String>,
    pub body: String,
}

pub fn parse_frontmatter(text: &str) -> ParsedMarkdown {
    let source = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let normalized = source.replace("\r\n", "\n");

    if let Some(rest) = normalized.strip_prefix("---\n") {
        if let Some((frontmatter, after)) = rest.split_once("\n---") {
            let body_raw = after.strip_prefix('\n').unwrap_or(after);
            let mut meta = BTreeMap::new();
            for line in frontmatter.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();
                    if !key.is_empty() {
                        meta.insert(key.to_string(), value.to_string());
                    }
                }
            }
            return ParsedMarkdown {
                meta,
                body: body_raw.trim_start().to_string(),
            };
        }
    }

    ParsedMarkdown {
        meta: BTreeMap::new(),
        body: source.trim().to_string(),
    }
}

pub struct ProfileInput<'a> {
    pub name: &'a str,
    pub headline: &'a str,
    pub email: &'a str,
    pub phone: &'a str,
    pub location: &'a str,
    pub website: &'a str,
    pub linkedin: &'a str,
    pub github: &'a str,
    pub body: &'a str,
}

pub fn serialize_profile_markdown(profile: &ProfileInput<'_>) -> String {
    format!(
        "---\nname: {}\nheadline: {}\nemail: {}\nphone: {}\nlocation: {}\nwebsite: {}\nlinkedin: {}\ngithub: {}\n---\n\n{}\n",
        profile.name.trim(),
        profile.headline.trim(),
        profile.email.trim(),
        profile.phone.trim(),
        profile.location.trim(),
        profile.website.trim(),
        profile.linkedin.trim(),
        profile.github.trim(),
        profile.body.trim(),
    )
}

pub fn starter_profile_markdown() -> String {
    serialize_profile_markdown(&ProfileInput {
        name: "",
        headline: "",
        email: "",
        phone: "",
        location: "",
        website: "",
        linkedin: "",
        github: "",
        body: "Add a short bio here if you want it on every resume header page.",
    })
}

pub fn serialize_resume_markdown(
    title: &str,
    status: &str,
    tags: &[String],
    updated: &str,
    body: &str,
) -> String {
    let status_val = if status.trim().is_empty() { "draft" } else { status.trim() };
    let tag_str = tags.join(", ");
    format!(
        "---\ntitle: {}\nstatus: {}\ntags: {}\nupdated: {}\n---\n\n{}\n",
        title.trim(),
        status_val,
        tag_str,
        updated.trim(),
        body.trim(),
    )
}

pub fn starter_markdown(title: &str) -> String {
    let body = "## Summary

Write a short professional summary.

## Skills

- Skill one
- Skill two

## Experience

### Role, Company (2022 – Present)

- What you shipped or changed
- The constraint or scale
- The result

## Projects

### Project name

What it is and why it matters.

## Education

**School** — Degree (Year)";
    serialize_resume_markdown(title, "draft", &[], "", body)
}

pub fn excerpt(body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("- ")
            && !trimmed.starts_with("* ")
        {
            return trimmed.to_string();
        }
    }
    String::new()
}
