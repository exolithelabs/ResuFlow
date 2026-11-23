use crate::workspace::{Profile, Resume};

pub fn esc(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn safe_url(value: &str) -> String {
    let url = value.trim();
    if url.is_empty() {
        return String::new();
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        return url.to_string();
    }
    if is_email_like(url) {
        return format!("mailto:{url}");
    }
    if is_domain_like(url) {
        return format!("https://{url}");
    }
    String::new()
}

fn is_email_like(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
        && is_domain_like(domain)
}

fn is_domain_like(value: &str) -> bool {
    let host = value.split('/').next().unwrap_or("");
    let Some((_, tld)) = host.rsplit_once('.') else {
        return false;
    };
    tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
        && host.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
}

fn display_url(value: &str) -> String {
    let trimmed = value.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("HTTPS://"))
        .or_else(|| trimmed.strip_prefix("HTTP://"))
        .unwrap_or(trimmed);
    without_scheme.trim_end_matches('/').to_string()
}

fn icon(name: &str) -> &'static str {
    match name {
        "mail" => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="20" height="16" rx="2"></rect><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"></path></svg>"#,
        "pin" => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0 1 18 0z"></path><circle cx="12" cy="10" r="3"></circle></svg>"#,
        "linkedin" => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M16 8a6 6 0 0 1 6 6v7h-4v-7a2 2 0 0 0-2-2 2 2 0 0 0-2 2v7h-4v-7a6 6 0 0 1 6-6z"></path><rect x="2" y="9" width="4" height="12"></rect><circle cx="4" cy="4" r="2"></circle></svg>"#,
        "github" => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22"></path></svg>"#,
        "phone" => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72c.13.81.32 1.6.59 2.36a2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.72-1.16a2 2 0 0 1 2.11-.45c.76.27 1.55.46 2.36.59A2 2 0 0 1 22 16.92z"></path></svg>"#,
        _ => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="2" y1="12" x2="22" y2="12"></line><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path></svg>"#,
    }
}

fn contact_link(icon_name: &str, href: &str, label: &str) -> String {
    if label.trim().is_empty() {
        return String::new();
    }
    let inner = format!("{} {}", icon(icon_name), esc(label));
    if !href.is_empty() {
        format!(
            r#"<a href="{}" class="contact-link" target="_blank" rel="noreferrer">{}</a>"#,
            esc(href),
            inner
        )
    } else {
        format!(r#"<span class="contact-link">{}</span>"#, inner)
    }
}

pub fn render_resume(profile: &Profile, resume: &Resume) -> String {
    let body_html = render_safe_markdown(&resume.body);
    let name = if profile.name.trim().is_empty() {
        "Your name"
    } else {
        profile.name.trim()
    };
    let title = if resume.title.trim().is_empty() {
        "Resume title"
    } else {
        resume.title.trim()
    };

    let website_display = {
        let d = display_url(&profile.website);
        if d.is_empty() { profile.website.clone() } else { d }
    };
    let linkedin_display = {
        let d = display_url(&profile.linkedin);
        if !d.is_empty() {
            d
        } else if !profile.linkedin.is_empty() {
            "LinkedIn".to_string()
        } else {
            String::new()
        }
    };
    let github_display = {
        let d = display_url(&profile.github);
        if !d.is_empty() {
            d
        } else if !profile.github.is_empty() {
            "GitHub".to_string()
        } else {
            String::new()
        }
    };

    let contacts: Vec<String> = [
        contact_link(
            "mail",
            &if profile.email.is_empty() {
                String::new()
            } else {
                format!("mailto:{}", profile.email)
            },
            &profile.email,
        ),
        contact_link(
            "phone",
            &if profile.phone.is_empty() {
                String::new()
            } else {
                format!("tel:{}", profile.phone)
            },
            &profile.phone,
        ),
        contact_link("web", &safe_url(&profile.website), &website_display),
        contact_link("pin", "", &profile.location),
        contact_link("linkedin", &safe_url(&profile.linkedin), &linkedin_display),
        contact_link("github", &safe_url(&profile.github), &github_display),
    ]
    .into_iter()
    .filter(|item| !item.is_empty())
    .collect();

    let contacts_html = if contacts.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="contact-links">{}</div>"#, contacts.join(""))
    };

    let content_html = if body_html.trim().is_empty() {
        r#"<p class="profile-text">Start writing in Edit mode.</p>"#.to_string()
    } else {
        body_html
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{} · {}</title>
  <style>{}</style>
</head>
<body>
  <div class="resume-container">
    <main class="main-content">
      <header>
        <div class="name-title">
          <h1>{}</h1>
          <p>{}</p>
        </div>
        {}
      </header>
      <div class="markdown-body">{}</div>
    </main>
  </div>
</body>
</html>"#,
        esc(name),
        esc(title),
        CLASSIC_CSS,
        esc(name),
        esc(title),
        contacts_html,
        content_html
    )
}

/// Converts Markdown to sanitized HTML. Raw HTML tags in input are never emitted
/// as elements, preventing script injection, event handlers, or unsafe schemes.
pub fn render_safe_markdown(markdown: &str) -> String {
    let normalized = markdown.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Fenced code block
        if trimmed.starts_with("```") {
            i += 1;
            let mut code_lines = Vec::new();
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                code_lines.push(esc(lines[i]));
                i += 1;
            }
            if i < lines.len() {
                i += 1;
            }
            out.push_str("<pre><code>");
            out.push_str(&code_lines.join("\n"));
            out.push_str("</code></pre>\n");
            continue;
        }

        // Horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            out.push_str("<hr />\n");
            i += 1;
            continue;
        }

        // Headings (# through ######)
        if let Some((level, heading_text)) = parse_heading(trimmed) {
            out.push_str(&format!(
                "<h{level}>{}</h{level}>\n",
                render_inline(heading_text)
            ));
            i += 1;
            continue;
        }

        // Blockquote
        if let Some(quote_line) = trimmed.strip_prefix('>') {
            let mut quote_lines = vec![render_inline(quote_line.trim())];
            i += 1;
            while i < lines.len() {
                if let Some(next_q) = lines[i].trim().strip_prefix('>') {
                    quote_lines.push(render_inline(next_q.trim()));
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("<blockquote><p>");
            out.push_str(&quote_lines.join("<br />"));
            out.push_str("</p></blockquote>\n");
            continue;
        }

        // Unordered list
        if let Some(item_text) = strip_ul_marker(trimmed) {
            out.push_str("<ul>\n");
            out.push_str(&format!("<li>{}</li>\n", render_inline(item_text)));
            i += 1;
            while i < lines.len() {
                if let Some(next_item) = strip_ul_marker(lines[i].trim()) {
                    out.push_str(&format!("<li>{}</li>\n", render_inline(next_item)));
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("</ul>\n");
            continue;
        }

        // Ordered list
        if let Some(item_text) = strip_ol_marker(trimmed) {
            out.push_str("<ol>\n");
            out.push_str(&format!("<li>{}</li>\n", render_inline(item_text)));
            i += 1;
            while i < lines.len() {
                if let Some(next_item) = strip_ol_marker(lines[i].trim()) {
                    out.push_str(&format!("<li>{}</li>\n", render_inline(next_item)));
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("</ol>\n");
            continue;
        }

        // Paragraph (with GFM line breaks)
        let mut para_lines = vec![render_inline(trimmed)];
        i += 1;
        while i < lines.len() {
            let next = lines[i].trim();
            if next.is_empty()
                || next.starts_with("```")
                || next == "---"
                || next == "***"
                || next == "___"
                || parse_heading(next).is_some()
                || next.starts_with('>')
                || strip_ul_marker(next).is_some()
                || strip_ol_marker(next).is_some()
            {
                break;
            }
            para_lines.push(render_inline(next));
            i += 1;
        }
        out.push_str("<p>");
        out.push_str(&para_lines.join("<br />\n"));
        out.push_str("</p>\n");
    }

    out
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes) {
        let rest = &line[hashes..];
        if let Some(stripped) = rest.strip_prefix(' ') {
            return Some((hashes, stripped.trim()));
        }
    }
    None
}

fn strip_ul_marker(line: &str) -> Option<&str> {
    line.strip_prefix("- ").or_else(|| line.strip_prefix("* "))
}

fn strip_ol_marker(line: &str) -> Option<&str> {
    let digits = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(stripped) = rest.strip_prefix(". ") {
            return Some(stripped);
        }
    }
    None
}

/// Strips raw HTML tags (`<script>...</script>`, `<...>`) and renders inline
/// Markdown formatting (`**bold**`, `*italic*`, `` `code` ``, `[label](url)`).
fn render_inline(input: &str) -> String {
    let stripped = strip_raw_html(input);
    let chars: Vec<char> = stripped.chars().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < chars.len() {
        // Ignore image syntax ![alt](url) -> render nothing or alt text only
        if chars[i] == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            if let Some((_alt, _url, next_idx)) = parse_link(&chars, i + 1) {
                i = next_idx;
                continue;
            }
        }

        // Link [label](url)
        if chars[i] == '[' {
            if let Some((label, url, next_idx)) = parse_link(&chars, i) {
                let label_html = esc(&label);
                if let Some(safe) = sanitize_link_href(&url) {
                    out.push_str(&format!(
                        r#"<a href="{}" target="_blank" rel="noopener noreferrer">{}</a>"#,
                        esc(&safe),
                        label_html
                    ));
                } else {
                    out.push_str(&format!("<a>{}</a>", label_html));
                }
                i = next_idx;
                continue;
            }
        }

        // Inline code `...`
        if chars[i] == '`' {
            if let Some((content, next_idx)) = take_delimited(&chars, i + 1, &['`']) {
                out.push_str("<code>");
                out.push_str(&esc(&content));
                out.push_str("</code>");
                i = next_idx;
                continue;
            }
        }

        // Bold **...**
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some((content, next_idx)) = take_delimited(&chars, i + 2, &['*', '*']) {
                out.push_str("<strong>");
                out.push_str(&render_inline(&content));
                out.push_str("</strong>");
                i = next_idx;
                continue;
            }
        }

        // Strikethrough ~~...~~
        if i + 1 < chars.len() && chars[i] == '~' && chars[i + 1] == '~' {
            if let Some((content, next_idx)) = take_delimited(&chars, i + 2, &['~', '~']) {
                out.push_str("<del>");
                out.push_str(&render_inline(&content));
                out.push_str("</del>");
                i = next_idx;
                continue;
            }
        }

        // Italic *...*
        if chars[i] == '*' {
            if let Some((content, next_idx)) = take_delimited(&chars, i + 1, &['*']) {
                out.push_str("<em>");
                out.push_str(&render_inline(&content));
                out.push_str("</em>");
                i = next_idx;
                continue;
            }
        }

        match chars[i] {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            ch => out.push(ch),
        }
        i += 1;
    }

    out
}

fn strip_raw_html(input: &str) -> String {
    let mut current = input.to_string();
    // Remove <script...>...</script> and <style...>...</style> blocks completely
    for tag in ["script", "style"] {
        loop {
            let lower = current.to_ascii_lowercase();
            let open_pat = format!("<{tag}");
            let close_pat = format!("</{tag}>");
            if let Some(start) = lower.find(&open_pat) {
                if let Some(end_rel) = lower[start..].find(&close_pat) {
                    let end = start + end_rel + close_pat.len();
                    current.replace_range(start..end, "");
                    continue;
                }
            }
            break;
        }
    }

    // Strip any remaining <...> HTML tags
    let mut out = String::with_capacity(current.len());
    let mut in_tag = false;
    for ch in current.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' && in_tag {
            in_tag = false;
        } else if !in_tag {
            out.push(ch);
        }
    }
    out
}

fn parse_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    if chars.get(start) != Some(&'[') {
        return None;
    }
    let mut idx = start + 1;
    let mut label = String::new();
    while idx < chars.len() && chars[idx] != ']' {
        label.push(chars[idx]);
        idx += 1;
    }
    if idx + 1 >= chars.len() || chars[idx] != ']' || chars[idx + 1] != '(' {
        return None;
    }
    idx += 2;
    let mut url = String::new();
    while idx < chars.len() && chars[idx] != ')' {
        url.push(chars[idx]);
        idx += 1;
    }
    if idx >= chars.len() || chars[idx] != ')' {
        return None;
    }
    Some((label, url.trim().to_string(), idx + 1))
}

fn take_delimited(chars: &[char], start: usize, delim: &[char]) -> Option<(String, usize)> {
    let dlen = delim.len();
    if start + dlen > chars.len() {
        return None;
    }
    let mut idx = start;
    let mut content = String::new();
    while idx + dlen <= chars.len() {
        if &chars[idx..idx + dlen] == delim {
            if content.is_empty() {
                return None;
            }
            return Some((content, idx + dlen));
        }
        content.push(chars[idx]);
        idx += 1;
    }
    None
}

fn sanitize_link_href(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
    {
        return Some(trimmed.to_string());
    }
    None
}

const CLASSIC_CSS: &str = r#"
:root {
  --bg: #f8fafc;
  --paper: #ffffff;
  --ink: #0f172a;
  --ink-soft: #334155;
  --ink-muted: #64748b;
  --line: #cbd5e1;
  --line-soft: #f1f5f9;
  --primary: #4f46e5;
}
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  font-family: "Segoe UI", system-ui, sans-serif;
  background: radial-gradient(circle at top, #ffffff 0%, #f1f5f9 60%, #e2e8f0 100%);
  color: var(--ink);
  line-height: 1.45;
  -webkit-print-color-adjust: exact;
  print-color-adjust: exact;
}
h1, h2, h3, h4 { font-family: "Segoe UI", system-ui, sans-serif; letter-spacing: -0.02em; }
.resume-container {
  max-width: 960px;
  margin: 24px auto;
  background: var(--paper);
  box-shadow: 0 20px 50px rgba(15, 23, 42, 0.05), 0 4px 12px rgba(15, 23, 42, 0.02);
  overflow: hidden;
  border-radius: 16px;
  border: 1px solid var(--line);
}
.main-content { padding: 32px 40px; }
header { margin-bottom: 16px; }
.name-title h1 { font-size: 2.3rem; line-height: 1; margin-bottom: 4px; font-weight: 800; }
.name-title p { font-size: 1rem; color: var(--primary); font-weight: 600; }
.contact-links {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 16px;
  margin-top: 12px;
  padding: 6px 0;
  border-top: 1px solid var(--line-soft);
  border-bottom: 1px solid var(--line-soft);
}
.contact-link {
  display: flex;
  align-items: center;
  gap: 6px;
  text-decoration: none;
  color: var(--ink-soft);
  font-size: 0.8rem;
  font-weight: 500;
}
.contact-link svg { width: 14px; height: 14px; flex-shrink: 0; }
.profile-text { color: var(--ink-soft); font-size: 0.85rem; line-height: 1.5; }
.markdown-body { color: var(--ink-soft); font-size: 0.88rem; }
.markdown-body h1, .markdown-body h2, .markdown-body h3 {
  color: var(--ink);
  margin: 1.1em 0 0.45em;
}
.markdown-body h2 {
  font-size: 1.05rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  display: flex;
  align-items: center;
  gap: 12px;
}
.markdown-body h2::after {
  content: '';
  flex: 1;
  height: 2px;
  background: linear-gradient(to right, #cbd5e1, transparent);
}
.markdown-body h3 { font-size: 0.95rem; }
.markdown-body p, .markdown-body ul, .markdown-body ol { margin: 0 0 0.7em; }
.markdown-body ul, .markdown-body ol { padding-left: 1.2em; }
.markdown-body a { color: var(--primary); }
.markdown-body strong { color: var(--ink); }
@media print {
  @page { size: A4; margin: 5mm 8mm; }
  body { background: white; margin: 0; font-size: 11pt; line-height: 1.3; }
  .resume-container { margin: 0; box-shadow: none; max-width: none; border-radius: 0; border: 0; }
  .main-content { padding: 0; }
}
@media (max-width: 768px) {
  .resume-container { margin: 16px; }
  .main-content { padding: 24px 20px; }
  .name-title h1, .name-title p { text-align: center; }
}
"#;
