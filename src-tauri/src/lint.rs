//! Deterministic resume checks: ATS-hostile structure, weak bullets, leftovers, length.
//! No AI involved; agents and the app both use these as a fast, objective first pass.

use crate::doc::Doc;
use crate::workspace::{Result, Workspace};
use serde::Serialize;

#[derive(Serialize, Debug, Clone)]
pub struct Finding {
    /// "error" | "warn" | "info"
    pub level: &'static str,
    pub message: String,
    /// 1-based line in resume.md, when the finding points at one line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Serialize, Debug)]
pub struct Report {
    pub words: usize,
    pub bullets: usize,
    pub bullets_with_numbers: usize,
    pub est_pages: f64,
    pub findings: Vec<Finding>,
}

const WEAK_OPENERS: [&str; 9] = [
    "responsible for", "worked on", "helped", "assisted", "involved in", "tasked with",
    "participated in", "duties included", "in charge of",
];

const PLACEHOLDERS: [&str; 6] = [
    "Your Name", "you@example.com", "Achievement with a measurable result", "Role — Company",
    "Two or three lines aimed at this role", "Degree — Institution",
];

impl Workspace {
    pub fn check(&self, slug: &str) -> Result<Report> {
        let content = self.read_resume(slug)?.content;
        Ok(check_doc(&self.profile(), &content))
    }
}

pub fn check_doc(profile: &Doc, content: &str) -> Report {
    let resume = Doc::parse(content);
    let mut f: Vec<Finding> = Vec::new();
    let mut add = |level, message: String, line| f.push(Finding { level, message, line });

    let field = |k: &str| resume.str(k).or_else(|| profile.str(k));
    if field("name").is_none() {
        add("error", "No name: set it on the default template.".into(), None);
    }
    if field("email").is_none() && field("phone").is_none() {
        add("error", "No email or phone: recruiters and ATS parsers need a contact.".into(), None);
    }

    for (k, placeholder) in [("name", "Your Name"), ("email", "you@example.com"), ("headline", "What you do, in one line")] {
        if field(k).as_deref() == Some(placeholder) {
            add("warn", format!("`{k}` is still the template value: edit the default template."), None);
        }
    }

    // Body starts after the front matter; keep real file line numbers.
    let body_start = content.lines().count() - resume.body.lines().count();
    let mut words = 0usize;
    let mut bullets = 0usize;
    let mut numbered = 0usize;
    let mut headings: Vec<String> = Vec::new();

    for (i, raw) in resume.body.lines().enumerate() {
        let line = body_start + i + 1;
        let t = raw.trim();
        words += t.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count();

        if let Some(h) = t.strip_prefix("## ") {
            headings.push(h.trim().to_lowercase());
        }
        if t.starts_with('|') {
            add("warn", "Table: many ATS parsers scramble tables. Prefer headings and bullets.".into(), Some(line));
        }
        if t.contains("![") {
            add("warn", "Image: ATS parsers ignore images and they waste space.".into(), Some(line));
        }
        for p in PLACEHOLDERS {
            if t.contains(p) {
                add("warn", format!("Template placeholder left in: \"{p}\"."), Some(line));
            }
        }

        let bullet = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| {
            let (n, rest) = t.split_once(". ")?;
            n.chars().all(|c| c.is_ascii_digit()).then_some(rest)
        });
        let Some(b) = bullet else { continue };
        if b.contains(':') && b.split(':').next().unwrap_or("").split_whitespace().count() <= 3 {
            continue; // "Languages: Rust, Go" style skill lines are not achievement bullets
        }
        bullets += 1;
        if b.chars().any(|c| c.is_ascii_digit()) {
            numbered += 1;
        }
        let lower = b.to_lowercase();
        if let Some(w) = WEAK_OPENERS.iter().find(|w| lower.starts_with(*w)) {
            add("warn", format!("Weak opener \"{w}\": lead with a strong verb and the result."), Some(line));
        }
        let n = b.split_whitespace().count();
        if n > 34 {
            add("warn", format!("Long bullet ({n} words): aim for one line, two at most."), Some(line));
        }
        if lower.starts_with("i ") || lower.contains(" i ") || lower.starts_with("my ") {
            add("info", "First person: resumes usually drop \"I\" and \"my\".".into(), Some(line));
        }
    }

    if !headings.iter().any(|h| h.contains("experience") || h.contains("employment") || h.contains("work")) {
        add("warn", "No \"Experience\" section heading (## Experience): ATS parsers look for it.".into(), None);
    }
    if !headings.iter().any(|h| h.contains("skill")) {
        add("info", "No \"Skills\" section: a plain keyword list helps ATS matching.".into(), None);
    }
    if bullets >= 4 && numbered * 2 < bullets {
        add(
            "warn",
            format!("Only {numbered} of {bullets} bullets contain a number. Quantify impact where you can."),
            None,
        );
    }
    if words < 180 {
        add("info", format!("Short ({words} words). Most strong one-page resumes run 350–600."), None);
    } else if words > 950 {
        add("warn", format!("Long ({words} words, ~{:.1} pages). Trim to what this role needs.", words as f32 / 520.0), None);
    }

    f.sort_by_key(|x| match x.level { "error" => 0, "warn" => 1, _ => 2 });
    Report {
        words,
        bullets,
        bullets_with_numbers: numbered,
        est_pages: ((words as f64 / 520.0) * 10.0).ceil() / 10.0,
        findings: f,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_common_problems() {
        let p = Doc::parse("---\nname: A\nemail: a@b.c\n---\n");
        let src = "---\ntitle: T\n---\n\n## Experience\n\n- Responsible for the build\n- Cut costs 30%\n| a | b |\n";
        let r = check_doc(&p, src);
        assert_eq!(r.bullets, 2);
        assert_eq!(r.bullets_with_numbers, 1);
        let weak = r.findings.iter().find(|f| f.message.starts_with("Weak")).unwrap();
        assert_eq!(weak.line, Some(7));
        assert!(r.findings.iter().any(|f| f.message.starts_with("Table")));
        assert!(!r.findings.iter().any(|f| f.level == "error"));
    }
}
