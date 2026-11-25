//! Markdown → print-ready A4 HTML. One renderer serves the preview, PDF export, the CLI and MCP.

use crate::doc::Doc;
use crate::workspace::{atomic_write, Result, Workspace};
use pulldown_cmark::{html, Event, Options, Parser};
use std::path::PathBuf;

const CSS: &str = include_str!("resume.css");

impl Workspace {
    pub fn render(&self, slug: &str) -> Result<String> {
        let resume = Doc::parse(&self.read_resume(slug)?.content);
        Ok(render_doc(&self.profile(), &resume))
    }

    /// Render next to the resume as `resume.html` and return its path.
    pub fn save_render(&self, slug: &str) -> Result<PathBuf> {
        let html = self.render(slug)?;
        let path = self.root.join("resumes").join(slug).join("resume.html");
        atomic_write(&path, &html)?;
        Ok(path)
    }
}

pub fn render_doc(profile: &Doc, resume: &Doc) -> String {
    let pick = |k: &str| resume.str(k).or_else(|| profile.str(k)).unwrap_or_default();
    let name = pick("name");
    let headline = pick("headline");

    let mut contacts: Vec<String> = Vec::new();
    for key in ["location", "email", "phone"] {
        let v = pick(key);
        if v.is_empty() {
            continue;
        }
        contacts.push(match key {
            "email" => format!("<a href=\"mailto:{0}\">{0}</a>", esc(&v)),
            "phone" => format!("<a href=\"tel:{}\">{}</a>", esc(&v.replace([' ', '-', '(', ')'], "")), esc(&v)),
            _ => esc(&v),
        });
    }
    let links = if resume.get("links").is_some() { resume.list("links") } else { profile.list("links") };
    for l in &links {
        contacts.push(format!("<a href=\"{}\">{}</a>", esc(&href(l)), esc(display_link(l))));
    }

    let title = if name.is_empty() { resume.str("title").unwrap_or_default() } else { name.clone() };
    let mut out = String::with_capacity(8 * 1024);
    out.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    out.push_str(&format!("<title>{}</title><style>{CSS}</style></head><body><main class=\"page\">", esc(&title)));
    if !name.is_empty() || !headline.is_empty() || !contacts.is_empty() {
        out.push_str("<header>");
        if !name.is_empty() {
            out.push_str(&format!("<h1>{}</h1>", esc(&name)));
        }
        if !headline.is_empty() {
            out.push_str(&format!("<p class=\"headline\">{}</p>", esc(&headline)));
        }
        if !contacts.is_empty() {
            out.push_str(&format!("<p class=\"contact\">{}</p>", contacts.join("<span class=\"sep\"> · </span>")));
        }
        out.push_str("</header>");
    }
    out.push_str(&markdown(&resume.body));
    out.push_str("</main></body></html>");
    out
}

/// A link as written in front matter, made absolute. Only web and mail links survive.
fn href(l: &str) -> String {
    let lower = l.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:") {
        l.to_string()
    } else if l.contains(':') && !l.contains('.') {
        "#".to_string()
    } else {
        format!("https://{}", l.trim_start_matches('/'))
    }
}

fn display_link(l: &str) -> &str {
    let l = l.strip_prefix("https://").or_else(|| l.strip_prefix("http://")).unwrap_or(l);
    let l = l.strip_prefix("www.").unwrap_or(l);
    l.trim_end_matches('/')
}

/// Markdown to HTML with raw HTML shown as text, so resumes stay plain, portable and safe to preview.
pub fn markdown(src: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_SMART_PUNCTUATION;
    let parser = Parser::new_ext(src, opts).map(|e| match e {
        Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_escaping() {
        let p = Doc::parse("---\nname: Ada <L>\nemail: a@b.c\nlinks: [github.com/ada, \"javascript:alert(1)\", https://www.ada.dev/]\n---\n");
        let r = Doc::parse("---\ntitle: X\nheadline: Engineer\n---\n## Exp\n<script>x</script>\n");
        let h = render_doc(&p, &r);
        assert!(h.contains("<h1>Ada &lt;L&gt;</h1>"));
        assert!(h.contains("Engineer"));
        assert!(h.contains("href=\"https://github.com/ada\""));
        assert!(h.contains(">ada.dev</a>"));
        assert!(!h.contains("href=\"javascript"));
        assert!(!h.contains("<script>"));
    }

    #[test]
    fn resume_links_override_profile() {
        let p = Doc::parse("---\nlinks: [a.dev]\n---\n");
        let r = Doc::parse("---\nlinks: [b.dev]\n---\n");
        let h = render_doc(&p, &r);
        assert!(h.contains("b.dev") && !h.contains("a.dev"));
    }
}
