//! Resume MDX → print-ready A4 HTML. One renderer serves printing, PDF export and MCP.
//!
//! Resumes are `.mdx`: ordinary Markdown plus two styling components, parsed here and never run.
//!
//! ```text
//! <Text color="#1f4e8c" size="10pt" caps spacing="0.08em">styled words</Text>
//! <Rule color="#d6d9de" />
//! ```
//!
//! `<Text>` takes `color`, `underline`, `underlineColor`, `size`, `caps`, `spacing` and `font`
//! (`serif`, `sans` or `mono`); `<Rule />` takes `color`. Attributes are string literals
//! (`name="…"`) or bare booleans (`caps`). Colours and lengths are checked ([`css_color`],
//! [`css_length`]). Anything else, other HTML or JSX included, prints as plain text.
//! The live editor (`livemd.ts`) reads the same syntax.

use crate::doc::Doc;
use crate::workspace::{atomic_write, Result, Workspace};
use pulldown_cmark::{html, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::PathBuf;

const CSS: &str = include_str!("resume.css");

impl Workspace {
    pub fn render(&self, slug: &str) -> Result<String> {
        Ok(render_doc(&Doc::parse(&self.read_resume(slug)?.content)))
    }

    /// Render next to the resume as `resume.html` and return its path.
    pub fn save_render(&self, slug: &str) -> Result<PathBuf> {
        let html = self.render(slug)?;
        let path = self.root.join("resumes").join(slug).join("resume.html");
        atomic_write(&path, &html)?;
        Ok(path)
    }
}

/// The resume header, written as Markdown at the top of the body:
///
/// ```text
/// # Ada Lovelace
/// Payments platform engineer
/// London · ada@example.com · +44 20 7946 0000 · github.com/ada
/// ```
///
/// A `# ` line opening the body is the name. The plain lines right under it (blank lines
/// allowed only straight after the name) are the headline and contact lines: with two or
/// more, the first is the headline and the rest are contact lines; a lone line is a contact
/// line if it looks like one. The editor (`livemd.ts`) applies the same rule.
pub struct Header<'a> {
    pub name: &'a str,
    pub headline: Vec<&'a str>,
    pub contact: Vec<&'a str>,
    /// Everything after the header.
    pub rest: &'a str,
}

pub fn split_header(body: &str) -> Option<Header<'_>> {
    let mut lines = line_spans(body).skip_while(|(_, l)| l.trim().is_empty());
    let (_, first) = lines.next()?;
    let name = first.strip_prefix("# ")?.trim();
    let mut found: Vec<&str> = Vec::new();
    let mut rest = body.len();
    for (at, line) in lines {
        let t = line.trim();
        if t.is_empty() && found.is_empty() {
            continue;
        }
        if t.is_empty() || starts_block(t) {
            rest = at;
            break;
        }
        found.push(t);
    }
    let (headline, contact) = match found.len() {
        0 => (vec![], vec![]),
        1 if !looks_like_contact(found[0]) => (found, vec![]),
        1 => (vec![], found),
        _ => (vec![found[0]], found[1..].to_vec()),
    };
    Some(Header { name, headline, contact, rest: &body[rest..] })
}

/// Each line of `s` with its byte offset, line endings stripped.
fn line_spans(s: &str) -> impl Iterator<Item = (usize, &str)> {
    s.split_inclusive('\n').scan(0, |at, l| {
        let start = *at;
        *at += l.len();
        Some((start, l.trim_end_matches(['\r', '\n'])))
    })
}

/// A line that opens a Markdown block other than a paragraph: heading, list, quote, rule, fence, table.
fn starts_block(t: &str) -> bool {
    let list = t.split_once(' ').is_some_and(|(m, _)| {
        matches!(m, "-" | "*" | "+") || (m.len() > 1 && m[..m.len() - 1].bytes().all(|b| b.is_ascii_digit()) && m.ends_with(['.', ')']))
    });
    let rule = t.len() >= 3 && ['-', '*', '_'].iter().any(|&c| t.chars().all(|x| x == c || x == ' ') && t.contains(c));
    let component_rule = matches!(component(t), Some(Component::Rule(_)));
    list || rule || component_rule || t.starts_with(['#', '>', '|']) || t.starts_with("```") || t.starts_with("~~~")
}

fn looks_like_contact(t: &str) -> bool {
    t.contains('·') || t.contains(" | ") || t.contains('@') || t.contains("](") || t.contains("://") || t.bytes().filter(u8::is_ascii_digit).count() >= 7
}

pub fn render_doc(resume: &Doc) -> String {
    let header = split_header(&resume.body);
    let name = header.as_ref().map(|h| plain_text(h.name)).unwrap_or_default();
    let title = if name.is_empty() { resume.str("title").unwrap_or_default() } else { name };
    let mut out = String::with_capacity(8 * 1024);
    out.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    out.push_str(&format!("<title>{}</title><style>{CSS}</style></head><body><main class=\"page\">", esc(&title)));
    match &header {
        Some(h) => {
            out.push_str("<header>");
            if !h.name.is_empty() {
                out.push_str(&format!("<h1>{}</h1>", inline(h.name)));
            }
            for l in &h.headline {
                out.push_str(&format!("<p>{}</p>", inline(l)));
            }
            for l in &h.contact {
                out.push_str(&format!("<p>{}</p>", contact_line(l)));
            }
            out.push_str("</header>");
            out.push_str(&markdown(h.rest));
        }
        None => out.push_str(&markdown(&resume.body)),
    }
    out.push_str("</main></body></html>");
    out
}

/// The words of a line of resume Markdown, without its Markdown or component tags:
/// `# <Text color="#1f4e8c">**Ada**</Text>` reads "Ada". For titles, file names and checks.
pub fn plain_text(src: &str) -> String {
    let mut out = String::new();
    for e in Parser::new_ext(&strip_components(src), Options::ENABLE_STRIKETHROUGH) {
        match e {
            Event::Text(t) | Event::Code(t) => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak => out.push(' '),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Markdown for one line, without the paragraph around it.
fn inline(src: &str) -> String {
    let html = markdown(src);
    let html = html.trim_end();
    html.strip_prefix("<p>").and_then(|h| h.strip_suffix("</p>")).unwrap_or(html).to_string()
}

/// A contact line, printed as written. Its items (split on `·` or `|`) that are a bare email,
/// phone number or web address become links. The line stays Markdown until the end, so a
/// `<Text>` styling several items (or the whole line) still wraps them all.
fn contact_line(line: &str) -> String {
    let mut md = String::new();
    let mut rest = line;
    // Component tags pass through untouched; only the text between them is split into items.
    while !rest.is_empty() {
        let tag = rest.find('<').and_then(|i| {
            let end = i + rest[i..].find('>')? + 1;
            component(&rest[i..end]).map(|_| (i, end))
        });
        let (text, tag_text, next) = match tag {
            Some((i, end)) => (&rest[..i], &rest[i..end], &rest[end..]),
            None => (rest, "", ""),
        };
        for piece in text.split_inclusive(['·', '|']) {
            let (item, sep) = match piece.char_indices().last() {
                Some((i, c)) if c == '·' || c == '|' => (&piece[..i], &piece[i..]),
                _ => (piece, ""),
            };
            let core = item.trim();
            md.push_str(&item[..item.len() - item.trim_start().len()]);
            md.push_str(&contact_item(core));
            md.push_str(&item[item.trim_end().len()..]);
            md.push_str(sep);
        }
        md.push_str(tag_text);
        rest = next;
    }
    inline(&md)
}

/// One contact item as Markdown: a link when it is a bare email, phone number or web address.
fn contact_item(i: &str) -> String {
    let bare = !i.is_empty() && !i.contains(' ') && !i.contains("](") && !i.starts_with('<');
    let digits = i.bytes().filter(u8::is_ascii_digit).count();
    let link = |label: &str, url: &str| {
        let label: String = label.chars().flat_map(|c| if "\\`*_[]<>".contains(c) { vec!['\\', c] } else { vec![c] }).collect();
        format!("[{label}](<{}>)", url.replace(['<', '>', ' '], ""))
    };
    if bare && i.contains('@') && !i.contains('/') {
        link(i, &format!("mailto:{}", i.strip_prefix("mailto:").unwrap_or(i)))
    } else if digits >= 7 && i.chars().all(|c| c.is_ascii_digit() || " +-().".contains(c)) {
        link(i, &format!("tel:{}", i.replace([' ', '-', '(', ')', '.'], "")))
    } else if bare && i.contains('.') && !i.ends_with('.') {
        link(i, &href(i))
    } else {
        i.to_string()
    }
}

/// A bare web address from a contact line, made absolute. Only web and mail links survive.
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

/// Markdown to HTML with raw HTML shown as text, so resumes stay plain, portable and safe to preview.
pub fn markdown(src: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_SMART_PUNCTUATION;
    let src = isolate_rules(src);
    let mut events: Vec<Event> = Vec::new();
    let mut open = 0usize; // <Text> spans not yet closed
    for e in Parser::new_ext(&src, opts) {
        match e {
            Event::Html(h) | Event::InlineHtml(h) => match component(&h) {
                Some(Component::Text(style)) => {
                    open += 1;
                    events.push(Event::InlineHtml(CowStr::from(format!("<span style=\"{}\">", esc(&style)))));
                }
                Some(Component::TextEnd) if open > 0 => {
                    open -= 1;
                    events.push(Event::InlineHtml("</span>".into()));
                }
                Some(Component::Rule(style)) => {
                    let attr = if style.is_empty() { String::new() } else { format!(" style=\"{}\"", esc(&style)) };
                    events.push(Event::Html(CowStr::from(format!("<hr{attr} />\n"))));
                }
                _ => events.push(Event::Text(h)), // raw HTML and anything unknown show as text
            },
            // A <Text> left open styles the rest of its block, as in the editor.
            Event::End(end @ (TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::TableCell)) => {
                events.extend(std::iter::repeat_n(Event::InlineHtml("</span>".into()), open));
                open = 0;
                events.push(Event::End(end));
            }
            other => events.push(other),
        }
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

/// One of ResuFlow's MDX components, as the tag it opens or closes.
#[derive(Debug, PartialEq)]
pub enum Component {
    /// `<Text …>`, with the CSS its attributes ask for.
    Text(String),
    /// `</Text>`
    TextEnd,
    /// `<Rule … />`, with the CSS its attributes ask for.
    Rule(String),
}

/// Read one tag, such as `<Text color="red">`. Anything that is not exactly a component tag is None.
pub fn component(tag: &str) -> Option<Component> {
    let inner = tag.trim().strip_prefix('<')?.strip_suffix('>')?;
    if let Some(name) = inner.strip_prefix('/') {
        return (name.trim() == "Text").then_some(Component::TextEnd);
    }
    let (inner, self_closing) = match inner.strip_suffix('/') {
        Some(i) => (i, true),
        None => (inner, false),
    };
    let name_len = inner.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(inner.len());
    let (name, mut rest) = inner.split_at(name_len);
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let n = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len());
        if n == 0 {
            return None;
        }
        let (key, after) = rest.split_at(n);
        let after = after.trim_start();
        if let Some(v) = after.strip_prefix('=') {
            let v = v.trim_start();
            let q = v.chars().next().filter(|&c| c == '"' || c == '\'')?;
            let end = v[1..].find(q)? + 1;
            attrs.push((key, &v[1..end]));
            rest = &v[end + 1..];
        } else {
            attrs.push((key, "true"));
            rest = after;
        }
    }
    let get = |k: &str| attrs.iter().find(|(a, _)| *a == k).map(|(_, v)| *v);
    let color = |k: &str| get(k).and_then(css_color);
    match (name, self_closing) {
        ("Text", false) => {
            let mut css = Vec::new();
            if let Some(c) = color("color") {
                css.push(format!("color: {c}"));
            }
            let underline_color = color("underlineColor");
            if get("underline").is_some_and(|v| v != "false") || underline_color.is_some() {
                css.push("text-decoration-line: underline".into());
            }
            if let Some(c) = underline_color {
                css.push(format!("text-decoration-color: {c}"));
            }
            if let Some(v) = get("size").and_then(css_length) {
                css.push(format!("font-size: {v}"));
            }
            if get("caps").is_some_and(|v| v != "false") {
                css.push("text-transform: uppercase".into());
            }
            if let Some(v) = get("spacing").and_then(css_length) {
                css.push(format!("letter-spacing: {v}"));
            }
            if let Some(f) = get("font").and_then(font_family) {
                css.push(format!("font-family: {f}"));
            }
            Some(Component::Text(css.join("; ")))
        }
        ("Rule", true) => Some(Component::Rule(color("color").map(|c| format!("border-top-color: {c}")).unwrap_or_default())),
        _ => None,
    }
}

/// A CSS colour: `#rgb`, `#rrggbb` (with optional alpha), a name such as `teal`, or
/// `rgb()` / `hsl()` with plain numbers. Nothing else reaches a style attribute.
pub fn css_color(v: &str) -> Option<&str> {
    let v = v.trim();
    let hex = v.strip_prefix('#').is_some_and(|h| matches!(h.len(), 3 | 4 | 6 | 8) && h.bytes().all(|b| b.is_ascii_hexdigit()));
    let named = (1..=32).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_alphabetic());
    let func = ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|f| v.starts_with(f))
        && v.ends_with(')')
        && v[v.find('(').unwrap() + 1..v.len() - 1].bytes().all(|b| b.is_ascii_digit() || b" .,%/-".contains(&b) || b"deg".contains(&b));
    (hex || named || func).then_some(v)
}

/// A CSS length such as `9.5pt`, `0.08em` or `90%`: a short number and a unit, nothing else.
pub fn css_length(v: &str) -> Option<&str> {
    let v = v.trim();
    let unit = ["rem", "pt", "px", "em", "%"].into_iter().find(|u| v.ends_with(u))?;
    let num = v[..v.len() - unit.len()].strip_prefix('-').unwrap_or(&v[..v.len() - unit.len()]);
    let (int, frac) = num.split_once('.').unwrap_or((num, "0"));
    let digits = |d: &str, max| (1..=max).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit());
    (digits(int, 3) && digits(frac, 3)).then_some(v)
}

/// The fonts a `<Text font="…">` can ask for.
fn font_family(v: &str) -> Option<&'static str> {
    match v.trim() {
        "serif" => Some("Georgia, \"Times New Roman\", serif"),
        "sans" => Some("\"Inter\", \"Segoe UI\", \"Helvetica Neue\", Arial, sans-serif"),
        "mono" => Some("ui-monospace, \"SF Mono\", Menlo, monospace"),
        _ => None,
    }
}

/// A `<Rule />` alone on an unindented line is a block of its own, as in the editor: blank
/// lines around it keep it from joining the paragraph above or swallowing the lines below.
fn isolate_rules(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 32);
    let mut fenced = false;
    for line in src.split_inclusive('\n') {
        let t = line.trim_end();
        if t.trim_start().starts_with("```") || t.trim_start().starts_with("~~~") {
            fenced = !fenced;
        }
        if !fenced && !t.starts_with([' ', '\t']) && matches!(component(t), Some(Component::Rule(_))) {
            out.push('\n');
            out.push_str(t);
            out.push_str("\n\n");
        } else {
            out.push_str(line);
        }
    }
    out
}

/// The text with component tags removed, for counting words and checking the writing.
pub fn strip_components(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tag_end = rest[i..].find('>').map(|j| i + j + 1);
        match tag_end.filter(|&e| component(&rest[i..e]).is_some()) {
            Some(e) => rest = &rest[e..],
            None => {
                out.push('<');
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
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

// ---- one page ----------------------------------------------------------------

/// Points per millimetre.
const MM: f64 = 72.0 / 25.4;
/// The box a resume prints into: A4 (210 × 297mm) less resume.css's 16mm and 14mm margins, in pt.
const PAGE_WIDTH: f64 = 178.0 * MM;
const PAGE_HEIGHT: f64 = 269.0 * MM;
/// Body type, as resume.css sets it: 10.5pt on a 1.42 line.
const BODY: f64 = 10.5;
const LEADING: f64 = 1.42;
/// Inter, the font resume.css asks for first, sets about 6% wider than the Helvetica widths below
/// (Arial and Liberation Sans match them). Measuring as Inter means a resume that fits here
/// prints on one page whichever of these fonts a machine has.
const FONT_SCALE: f64 = 1.06;

/// How much of one A4 page a resume fills when printed: 1.0 is exactly full, more runs onto a
/// second page. Estimated from resume.css's type and spacing and per-character widths, without a
/// browser, so every writer (the app, agents through MCP) is held to the same page.
pub fn page_fill(resume: &Doc) -> f64 {
    let mut w = Walker::default();
    match split_header(&resume.body) {
        Some(h) => {
            // The header prints as an h1 and plain lines, spaced as `header` in resume.css.
            w.walk(&format!("# {}", h.name), (2.0, 4.0));
            for l in h.headline.iter().chain(&h.contact) {
                w.walk(l, (2.0, 0.0));
            }
            w.page.margin(8.0);
            w.walk(h.rest, (2.0, 4.0));
        }
        None => w.walk(&resume.body, (2.0, 4.0)),
    }
    w.page.y / PAGE_HEIGHT
}

/// Blocks stacked down the page, with CSS margin collapsing: adjoining margins merge into the
/// largest (less the most negative), and the first block's top margin is dropped.
#[derive(Default)]
struct Stack {
    y: f64,
    pos: f64,
    neg: f64,
    started: bool,
}

impl Stack {
    fn margin(&mut self, m: f64) {
        if m >= 0.0 {
            self.pos = self.pos.max(m);
        } else {
            self.neg = self.neg.min(m);
        }
    }

    fn content(&mut self, h: f64) {
        if self.started {
            self.y += self.pos + self.neg;
        }
        self.started = true;
        (self.pos, self.neg) = (0.0, 0.0);
        self.y += h;
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Face {
    Regular,
    Bold,
    Mono,
}

#[derive(Default)]
struct Walker {
    page: Stack,
    /// Text of the block being read, with the face of each character; '\n' is a hard break.
    text: Vec<(char, Face)>,
    bold: usize,
    /// Left indent of lists and quotes, in pt.
    indent: Vec<f64>,
    after_h2: bool,
    /// The open table: its column count, whether in the head row, and the row's tallest cell.
    table: Option<(usize, bool, f64)>,
}

impl Walker {
    /// Lay out a run of Markdown. `para` is a paragraph's top and bottom margin.
    fn walk(&mut self, src: &str, para: (f64, f64)) {
        let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_SMART_PUNCTUATION;
        for e in Parser::new_ext(&isolate_rules(src), opts) {
            match e {
                Event::Start(Tag::Paragraph) => self.page.margin(para.0),
                Event::End(TagEnd::Paragraph) => {
                    self.flush(BODY, LEADING);
                    self.page.margin(para.1);
                }
                Event::Start(Tag::Heading { level, .. }) => {
                    self.page.margin(heading(level).1);
                    self.bold += 1;
                }
                Event::End(TagEnd::Heading(level)) => {
                    let (size, _, bottom) = heading(level);
                    self.bold -= 1;
                    self.flush(size, 1.3);
                    self.page.margin(bottom);
                    self.after_h2 = level == HeadingLevel::H2;
                    continue;
                }
                Event::Start(Tag::List(_)) => {
                    self.flush(BODY, LEADING);
                    self.page.margin(2.0);
                    self.indent.push(13.0);
                }
                Event::End(TagEnd::List(_)) => {
                    self.indent.pop();
                    self.page.margin(4.0);
                }
                Event::Start(Tag::Item) => self.page.margin(1.0),
                Event::End(TagEnd::Item) => {
                    self.flush(BODY, LEADING);
                    self.page.margin(1.0);
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    self.page.margin(BODY);
                    self.indent.push(60.0);
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    self.indent.pop();
                    self.page.margin(BODY);
                }
                Event::Start(Tag::CodeBlock(_)) => self.page.margin(BODY),
                Event::End(TagEnd::CodeBlock) => {
                    // Preformatted: one printed line per source line, never wrapped.
                    let lines = self.text.iter().filter(|(c, _)| *c == '\n').count().max(1);
                    self.text.clear();
                    self.page.content(lines as f64 * BODY * LEADING);
                    self.page.margin(BODY);
                }
                Event::Start(Tag::Table(cols)) => self.table = Some((cols.len().max(1), false, 0.0)),
                Event::End(TagEnd::Table) => self.table = None,
                Event::Start(Tag::TableHead) => self.row(true),
                Event::Start(Tag::TableRow) => self.row(false),
                Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                    if let Some((_, _, tallest)) = self.table.as_mut() {
                        let h = std::mem::take(tallest);
                        self.page.content(h + 4.0 + 0.75); // 2pt padding above and below, 1px rule
                    }
                }
                Event::End(TagEnd::TableCell) => {
                    if let Some((cols, head, tallest)) = self.table {
                        let width = (self.width() / cols as f64 - 8.0).max(BODY);
                        let size = 9.5;
                        if head {
                            self.text.iter_mut().for_each(|(_, f)| *f = if *f == Face::Mono { Face::Mono } else { Face::Bold });
                        }
                        let h = lines(&std::mem::take(&mut self.text), size, width) as f64 * size * LEADING;
                        self.table = Some((cols, head, tallest.max(h)));
                    }
                }
                Event::Start(Tag::Strong) => self.bold += 1,
                Event::End(TagEnd::Strong) => self.bold -= 1,
                Event::Text(t) => self.push(&t, None),
                Event::Code(t) => self.push(&t, Some(Face::Mono)),
                Event::SoftBreak => self.push(" ", None),
                Event::HardBreak => self.push("\n", None),
                Event::Rule => self.rule(),
                Event::Html(h) | Event::InlineHtml(h) => match component(&h) {
                    Some(Component::Rule(_)) => self.rule(),
                    Some(_) => {}
                    None => self.push(&h, None),
                },
                _ => {}
            }
            self.after_h2 = false;
        }
        self.flush(BODY, LEADING);
    }

    fn width(&self) -> f64 {
        PAGE_WIDTH - self.indent.iter().sum::<f64>()
    }

    fn push(&mut self, s: &str, face: Option<Face>) {
        let face = face.unwrap_or(if self.bold > 0 { Face::Bold } else { Face::Regular });
        self.text.extend(s.chars().map(|c| (c, face)));
    }

    fn row(&mut self, head: bool) {
        if let Some(t) = self.table.as_mut() {
            t.1 = head;
        }
    }

    /// Print the text read so far as lines of `size` type.
    fn flush(&mut self, size: f64, leading: f64) {
        if self.table.is_some() {
            return;
        }
        let n = lines(&std::mem::take(&mut self.text), size, self.width());
        if n > 0 {
            self.page.content(n as f64 * size * leading);
        }
    }

    /// An `<hr>`: a 1px rule, pulled up tight under a section heading.
    fn rule(&mut self) {
        self.flush(BODY, LEADING);
        let (top, bottom) = if self.after_h2 { (-3.0, 5.0) } else { (8.0, 8.0) };
        self.page.margin(top);
        self.page.content(0.75);
        self.page.margin(bottom);
    }
}

/// A heading's size, top and bottom margin, from resume.css.
fn heading(level: HeadingLevel) -> (f64, f64, f64) {
    match level {
        HeadingLevel::H1 => (20.0, 8.0, 2.0),
        HeadingLevel::H2 => (14.0, 12.0, 4.0),
        HeadingLevel::H3 => (12.0, 8.0, 2.0),
        HeadingLevel::H4 => (11.0, 6.0, 1.0),
        _ => (10.5, 5.0, 1.0),
    }
}

/// How many lines `text` wraps to in `width` pt of `size` type, breaking between words.
fn lines(text: &[(char, Face)], size: f64, width: f64) -> usize {
    let (mut n, mut line, mut word, mut space) = (0usize, 0.0f64, 0.0f64, 0.0f64);
    let place = |line: &mut f64, word: f64, space: f64, n: &mut usize| {
        if *n == 0 {
            *n = 1;
        }
        if *line > 0.0 && *line + space + word > width {
            *n += 1;
            *line = word;
        } else if *line > 0.0 {
            *line += space + word;
        } else {
            *line = word;
        }
    };
    for &(c, face) in text {
        if c == '\n' {
            place(&mut line, word, space, &mut n);
            n = n.max(1) + 1;
            (line, word, space) = (0.0, 0.0, 0.0);
        } else if c.is_whitespace() {
            if word > 0.0 {
                place(&mut line, word, space, &mut n);
                (word, space) = (0.0, 0.0);
            }
            if line > 0.0 {
                space += char_width(' ', face, size);
            }
        } else {
            word += char_width(c, face, size);
        }
    }
    if word > 0.0 {
        place(&mut line, word, space, &mut n);
    }
    n
}

fn char_width(c: char, face: Face, size: f64) -> f64 {
    if face == Face::Mono {
        return 0.6 * 9.0; // code is 9pt monospace
    }
    // Helvetica's advance widths in 1/1000 em, for ASCII from the space to '~': regular, then bold.
    const REGULAR: [u16; 95] = [
        278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556,
        556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667,
        556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556,
        556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722,
        500, 500, 500, 334, 260, 334, 584,
    ];
    const BOLD: [u16; 95] = [
        278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556,
        556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722,
        611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556,
        611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778,
        556, 556, 500, 389, 280, 389, 584,
    ];
    let bold = face == Face::Bold;
    let em = match c {
        ' '..='~' => (if bold { BOLD } else { REGULAR })[c as usize - 32] as f64,
        '·' | '‘' | '’' | '•' => 333.0,
        '“' | '”' => 444.0,
        '—' | '…' => 1000.0,
        c if c >= '\u{2e80}' => 1000.0, // CJK and other full-width text
        _ if bold => 611.0,
        _ => 556.0,
    };
    em / 1000.0 * size * FONT_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_escaping() {
        let r = Doc::parse("---\ntitle: X\n---\n# Ada <L>\nEngineer\nLondon · a@b.c · +44 20 7946 0000 · github.com/ada · javascript:alert(1) · https://www.ada.dev/\n\n## Exp\n<script>x</script>\n");
        let h = render_doc(&r);
        assert!(h.contains("<h1>Ada &lt;L&gt;</h1>"));
        assert!(h.contains("<p>Engineer</p>"));
        // Printed as typed: the separators and each address as written.
        assert!(h.contains("<p>London · <a href=\"mailto:a@b.c\">a@b.c</a> · "));
        assert!(h.contains("href=\"mailto:a@b.c\""));
        assert!(h.contains("href=\"tel:+442079460000\""));
        assert!(h.contains("href=\"https://github.com/ada\""));
        assert!(h.contains(">https://www.ada.dev/</a>"));
        assert!(render_doc(&Doc::parse("# A\nB\nx.dev | Pune\n")).contains("<a href=\"https://x.dev\">x.dev</a> | Pune"));
        assert!(!h.contains("href=\"javascript"));
        assert!(!h.contains("<script>"));
        assert!(h.contains("<h2>Exp</h2>"));
    }

    #[test]
    fn components() {
        let h = markdown("Led <Text color=\"#b42318\" underline underlineColor='teal'>the **team**</Text> well\n");
        assert!(h.contains("<span style=\"color: #b42318; text-decoration-line: underline; text-decoration-color: teal\">the <strong>team</strong></span> well"));
        // Unsafe or unknown values never reach the style attribute; unknown tags print as text.
        let h = markdown("<Text color=\"red;background:url(x)\">a</Text> <b>x</b> <Text>open\n\nnext\n");
        assert!(h.contains("<span style=\"\">a</span>") && h.contains("&lt;b&gt;x&lt;/b&gt;"));
        assert!(h.contains("<span style=\"\">open</span></p>"));
        assert!(!h.contains("url("));
        // A rule line is its own block even straight under text, and leaves the next lines alone.
        let h = markdown("## Exp\n<Rule color=\"rgb(31, 78, 140)\" />\nText\n- a\n");
        assert!(h.contains("<h2>Exp</h2>\n<hr style=\"border-top-color: rgb(31, 78, 140)\" />\n<p>Text</p>\n<ul>"));
        assert!(markdown("```\n<Rule />\n```\n").contains("&lt;Rule /&gt;"));
        assert_eq!(component("<Rule>"), None);
        assert_eq!(component("<Text color=\"x\" broken=>"), None);
        assert_eq!(strip_components("- <Text color=\"red\">Led</Text> 3 < 4"), "- Led 3 < 4");
    }

    #[test]
    fn font_styling() {
        let Some(Component::Text(css)) = component("<Text size=\"10pt\" caps spacing='0.08em' font=\"serif\" color=\"#1f4e8c\">") else { panic!() };
        assert_eq!(
            css,
            "color: #1f4e8c; font-size: 10pt; text-transform: uppercase; letter-spacing: 0.08em; font-family: Georgia, \"Times New Roman\", serif"
        );
        // Lengths and fonts outside the whitelist are dropped.
        let Some(Component::Text(css)) = component("<Text size=\"10pt;x\" spacing=\"1e9px\" font=\"Comic\" caps=\"false\">") else { panic!() };
        assert_eq!(css, "");
        assert_eq!(css_length("-0.5em"), Some("-0.5em"));
        assert_eq!(css_length("12"), None);
    }

    #[test]
    fn styled_header() {
        let src = "# Ada\n<Text color=\"#555b66\">Engineer</Text>\n<Text color=\"#555b66\" size=\"9.5pt\">Pune · ada@x.dev · github.com/ada</Text>\n<Rule color=\"#d6d9de\" />\n## Work\n";
        let h = split_header(src).unwrap();
        assert_eq!((h.headline.len(), h.contact.len()), (1, 1));
        assert!(h.rest.starts_with("<Rule"));
        let html = render_doc(&Doc::parse(src));
        assert!(html.contains("<p><span style=\"color: #555b66; font-size: 9.5pt\">Pune · <a href=\"mailto:ada@x.dev\">ada@x.dev</a> · <a href=\"https://github.com/ada\">github.com/ada</a></span></p>"));
        assert!(html.contains("</header><hr style=\"border-top-color: #d6d9de\" />"));
    }

    #[test]
    fn styled_name_reads_plain() {
        let src = "---\ntitle: T\n---\n# <Text color=\"#1f4e8c\" caps>**Ada** Lovelace</Text>\nEngineer\n";
        assert!(render_doc(&Doc::parse(src)).contains("<title>Ada Lovelace</title>"));
        assert_eq!(plain_text(split_header("# <Text size=\"20pt\"></Text>\n").unwrap().name), "");
    }

    #[test]
    fn page_fill_follows_the_layout() {
        let fill = |body: &str| page_fill(&Doc { meta: vec![], body: body.into() });
        let template = fill(crate::workspace::RESUME_TEMPLATE);
        assert!((0.4..0.5).contains(&template), "{template}"); // Chromium lays it out at 345pt of 762
        let bullet = "- Shipped the ledger migration, cutting settlement latency by 42% across every payment rail we ran\n";
        assert!(fill(&format!("# Ada\n\n## Experience\n{}", bullet.repeat(20))) < 1.0);
        assert!(fill(&format!("# Ada\n\n## Experience\n{}", bullet.repeat(60))) > 1.0);
        // Wrapping: one bullet twice as long takes two lines; bold sets wider than regular.
        let one = fill(bullet);
        assert!(fill(&format!("- {} {}", &bullet[2..bullet.len() - 1], &bullet[2..])) > one * 1.9);
        assert!(lines(&"word ".repeat(20).chars().map(|c| (c, Face::Bold)).collect::<Vec<_>>(), BODY, 85.0)
            > lines(&"word ".repeat(20).chars().map(|c| (c, Face::Regular)).collect::<Vec<_>>(), BODY, 85.0));
    }

    #[test]
    fn header_shapes() {
        let h = split_header("# Ada\n\nLondon · a@b.c\n\n## Summary\n").unwrap();
        assert_eq!((h.name, h.headline.len(), h.contact), ("Ada", 0, vec!["London · a@b.c"]));
        assert_eq!(h.rest, "\n## Summary\n");
        let h = split_header("# Ada\nStaff engineer\n- a bullet\n").unwrap();
        assert_eq!((h.headline, h.contact.len(), h.rest), (vec!["Staff engineer"], 0, "- a bullet\n"));
        let h = split_header("# Ada\nHeadline\nLondon\n---\n").unwrap();
        assert_eq!((h.headline, h.contact, h.rest), (vec!["Headline"], vec!["London"], "---\n"));
        assert!(split_header("## Summary\n# Ada\n").is_none());
        assert!(split_header("").is_none());
    }
}
