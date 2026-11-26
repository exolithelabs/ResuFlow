//! Deterministic resume checks: missing contact, ATS-hostile structure, weak bullets, leftovers,
//! length, and keyword coverage against the target posting. No AI involved: the app and agents
//! both use these as a fast, objective first pass.

use crate::doc::Doc;
use crate::render::{page_fill, plain_text, split_header, strip_components};
use crate::workspace::{count_words, Result, Workspace};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Debug, Clone)]
pub struct Finding {
    /// "error" | "warn" | "info"
    pub level: &'static str,
    pub message: String,
    /// 1-based line in resume.mdx, when the finding points at one line.
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
    /// Present when the resume has a target posting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Keywords>,
}

/// Which of the posting's key terms the resume mentions.
#[derive(Serialize, Debug, PartialEq)]
pub struct Keywords {
    /// Percentage of terms found, 0–100.
    pub coverage: u32,
    pub found: Vec<String>,
    pub missing: Vec<String>,
}

const WEAK_OPENERS: [&str; 10] = [
    "responsible for", "worked on", "helped", "assisted", "involved in", "tasked with",
    "participated in", "duties included", "in charge of", "was part of",
];

const PLACEHOLDERS: [&str; 7] = [
    "Your Name", "you@example.com", "What you do, in one line", "Achievement with a measurable result",
    "Role — Company", "Two or three lines aimed at this role", "Degree — Institution",
];

impl Workspace {
    pub fn check(&self, slug: &str) -> Result<Report> {
        let content = self.read_resume(slug)?.content;
        let posting = self.read_posting(slug)?.content;
        Ok(check_doc(&content, Some(&posting).filter(|p| !p.trim().is_empty()).map(|p| p.as_str())))
    }
}

pub fn check_doc(content: &str, posting: Option<&str>) -> Report {
    let resume = Doc::parse(content);
    let mut f: Vec<Finding> = Vec::new();
    let mut add = |level, message: String, line| f.push(Finding { level, message, line });

    let header = split_header(&resume.body);
    if header.as_ref().is_none_or(|h| plain_text(h.name).is_empty()) {
        add("error", "No name: start the resume with a \"# Your Name\" line.".into(), None);
    }
    let reachable = header.as_ref().is_some_and(|h| {
        h.contact.iter().any(|l| l.contains('@') || l.bytes().filter(u8::is_ascii_digit).count() >= 7)
    });
    if !reachable {
        add("error", "No email or phone: add a contact line under your name (London · you@example.com · +44 …).".into(), None);
    }

    // The body starts after the front matter; findings keep real file line numbers.
    let body_start = content.lines().count().saturating_sub(resume.body.lines().count());
    let mut bullets = 0usize;
    let mut numbered = 0usize;
    let mut headings: Vec<String> = Vec::new();
    let mut openers: HashMap<String, Vec<usize>> = HashMap::new();
    let mut in_code = false;

    for (i, raw) in resume.body.lines().enumerate() {
        let line = body_start + i + 1;
        let plain = strip_components(raw); // judge the words, not the <Text> styling around them
        let t = plain.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if let Some(h) = t.strip_prefix("## ") {
            headings.push(h.trim().to_lowercase());
        }
        if t.starts_with('|') {
            add("warn", "Table: many ATS parsers scramble tables. Use headings and bullets.".into(), Some(line));
        }
        if t.contains("![") {
            add("warn", "Image: ATS parsers skip images, and they take space.".into(), Some(line));
        }
        for p in PLACEHOLDERS {
            if t.contains(p) {
                add("warn", format!("Template text left in: \"{p}\"."), Some(line));
            }
        }

        let Some(b) = bullet_text(t) else { continue };
        if b.split_once(':').is_some_and(|(label, _)| label.split_whitespace().count() <= 3) {
            continue; // "Languages: Rust, Go" lists skills; it is not an achievement
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
        if lower.starts_with("i ") || lower.contains(" i ") || lower.starts_with("my ") || lower.contains(" my ") {
            add("info", "First person: resumes usually drop \"I\" and \"my\".".into(), Some(line));
        }
        if let Some(first) = lower.split(|c: char| !c.is_alphanumeric()).next().filter(|w| w.len() > 2) {
            openers.entry(first.to_string()).or_default().push(line);
        }
    }

    let mut repeated: Vec<_> = openers.into_iter().filter(|(_, lines)| lines.len() >= 3).collect();
    repeated.sort_by_key(|(_, lines)| lines[0]);
    for (verb, lines) in repeated {
        add("info", format!("{} bullets start with \"{verb}\": vary the verbs.", lines.len()), Some(lines[0]));
    }

    if !headings.iter().any(|h| h.contains("experience") || h.contains("employment") || h.contains("work")) {
        add("warn", "No Experience section (## Experience): ATS parsers look for it.".into(), None);
    }
    if !headings.iter().any(|h| h.contains("skill")) {
        add("info", "No Skills section: a plain keyword list helps ATS matching.".into(), None);
    }
    if bullets >= 4 && numbered * 2 < bullets {
        add("warn", format!("Only {numbered} of {bullets} bullets have a number. Quantify impact where you can."), None);
    }
    let words = count_words(&resume.body);
    let fill = page_fill(&resume);
    let est_pages = (fill * 10.0).ceil() / 10.0;
    if words < 180 {
        add("info", format!("Short ({words} words). Most strong one-page resumes run 350–600."), None);
    }
    if fill > 1.0 {
        add("warn", format!("Runs past one A4 page (~{est_pages:.1} pages). Cut it to one page."), None);
    }

    let keywords = posting.map(|p| coverage(&key_terms(p), &strip_components(content)));
    if let Some(k) = &keywords {
        if !k.missing.is_empty() && k.coverage < 60 {
            add("warn", format!("Covers {}% of the posting's key terms. See which are missing.", k.coverage), None);
        }
    }

    f.sort_by_key(|x| match x.level {
        "error" => 0,
        "warn" => 1,
        _ => 2,
    });
    Report { words, bullets, bullets_with_numbers: numbered, est_pages, findings: f, keywords }
}

fn bullet_text(t: &str) -> Option<&str> {
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")).or_else(|| {
        let (n, rest) = t.split_once(". ")?;
        (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then_some(rest)
    })
}

// ---- keyword coverage --------------------------------------------------------

const MAX_TERMS: usize = 30;

/// Common English and job-ad words that say nothing about the role.
const STOPWORDS: &str = "a about above across after again against all also am an and any are as at be because been \
before being below between both but by can could did do does doing down during each either else etc ever every few for \
from further get gets getting had has have having he her here hers him his how however i if in into is it its itself \
just less like make makes many may me might more most much must my need needs no nor not now of off often on once one \
only or other our ours out over own per plus rather same she should since so some such than that the their theirs them \
then there these they this those through to too under until up upon us use used using very via want was we well were \
what when where whether which while who whom whose why will with within without would yet you your yours \
ability able across additional apply applicants benefits best candidate candidates closely company competitive \
culture day degree environment equal etc excellent experience experiences familiarity familiar folks following \
good great help helping highly ideal ideally including job join key knowledge least level looking new nice opportunity \
opportunities plus position preferred proven qualifications related required requirements responsibilities role \
skills strong team teams understanding work working world year years based bonus build building company's \
include includes including know learn love lot make offer part people products related seeking solid successful \
take things together way ways we're we'll you'll you're";

fn is_stop(w: &str) -> bool {
    STOPWORDS.split_whitespace().any(|s| s == w)
}

/// Tokens of a text: lowercase words, keeping characters that matter in tech terms (c++, c#, node.js, ci/cd).
fn tokens(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut sentence_start = true;
    for raw in text.split(|c: char| c.is_whitespace() || ",;:()[]{}\"!?*_`|<>".contains(c)) {
        let ends_sentence = raw.ends_with('.');
        let w = raw.trim_matches(|c: char| ".-/'’".contains(c));
        if w.is_empty() {
            sentence_start |= ends_sentence;
            continue;
        }
        // A capital letter mid-sentence suggests a name: a tool, framework, certification.
        let proper = !sentence_start && w.chars().next().is_some_and(char::is_uppercase);
        out.push((w.to_lowercase(), proper));
        sentence_start = ends_sentence;
    }
    out
}

fn is_term(w: &str) -> bool {
    w.chars().any(char::is_alphabetic) && !is_stop(w) && (w.chars().count() > 1 || w == "c" || w == "r")
}

/// The terms a posting cares about most: repeated two-word phrases, then names and repeated words.
pub fn key_terms(posting: &str) -> Vec<String> {
    let toks = tokens(posting);
    let mut uni: HashMap<&str, (usize, bool, usize)> = HashMap::new(); // count, proper, first index
    let mut bi: HashMap<String, (usize, usize)> = HashMap::new();
    for (i, (w, proper)) in toks.iter().enumerate() {
        if !is_term(w) {
            continue;
        }
        let e = uni.entry(w).or_insert((0, false, i));
        e.0 += 1;
        e.1 |= *proper;
        if let Some((next, _)) = toks.get(i + 1).filter(|(n, _)| is_term(n)) {
            let e = bi.entry(format!("{w} {next}")).or_insert((0, i));
            e.0 += 1;
        }
    }

    let mut phrases: Vec<(String, usize, usize)> = bi.into_iter().filter(|(_, (n, _))| *n >= 2).map(|(k, (n, i))| (k, n, i)).collect();
    phrases.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    phrases.truncate(MAX_TERMS / 3);

    let in_phrase = |w: &str| phrases.iter().any(|(p, _, _)| p.split(' ').any(|x| x == w));
    let techy = |w: &str| w.chars().any(|c| c.is_ascii_digit() || "+#./".contains(c));
    let mut words: Vec<(&str, usize, usize)> = uni
        .into_iter()
        .filter(|(w, (n, proper, _))| !in_phrase(w) && (*n >= 2 || *proper || techy(w)))
        .map(|(w, (n, proper, i))| (w, n + if proper || techy(w) { 2 } else { 0 }, i))
        .collect();
    words.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));

    let mut out: Vec<String> = phrases.into_iter().map(|(p, _, _)| p).collect();
    out.extend(words.into_iter().map(|(w, _, _)| w.to_string()));
    out.truncate(MAX_TERMS);
    out
}

/// Normalise a word for matching: "APIs" finds "api", "services" finds "service".
fn stem(w: &str) -> &str {
    if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") { &w[..w.len() - 1] } else { w }
}

pub fn coverage(terms: &[String], resume: &str) -> Keywords {
    let words: Vec<String> = tokens(resume).into_iter().map(|(w, _)| stem(&w).to_string()).collect();
    let has = |term: &str| {
        let parts: Vec<&str> = term.split(' ').map(stem).collect();
        words.windows(parts.len()).any(|win| win.iter().zip(&parts).all(|(a, b)| a == b))
    };
    let (found, missing): (Vec<String>, Vec<String>) = terms.iter().cloned().partition(|t| has(t));
    let coverage = if terms.is_empty() { 100 } else { (found.len() * 100 / terms.len()) as u32 };
    Keywords { coverage, found, missing }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_common_problems() {
        let src = "---\ntitle: T\n---\n\n# A\na@b.c\n\n## Experience\n\n- Responsible for the build\n- Cut costs 30%\n| a | b |\n\n```\n- Responsible for code\n```\n";
        let r = check_doc(src, None);
        assert_eq!(r.bullets, 2);
        assert_eq!(r.bullets_with_numbers, 1);
        let weak: Vec<_> = r.findings.iter().filter(|f| f.message.starts_with("Weak")).collect();
        assert_eq!(weak.len(), 1);
        assert_eq!(weak[0].line, Some(10));
        assert!(r.findings.iter().any(|f| f.message.starts_with("Table")));
        assert!(!r.findings.iter().any(|f| f.level == "error"));
        assert!(r.keywords.is_none());
    }

    #[test]
    fn repeated_openers() {
        let r = check_doc("# A\na@b.c\n## Experience\n- Led a 1\n- Led b 2\n- Led c 3\n- Skills: x\n", None);
        let f = r.findings.iter().find(|f| f.message.contains("\"led\"")).unwrap();
        assert_eq!(f.line, Some(4));
    }

    #[test]
    fn header_checks() {
        let errors = |src: &str| check_doc(src, None).findings.into_iter().filter(|f| f.level == "error").count();
        assert_eq!(errors("## Experience\n- Cut costs 30%\n"), 2);
        assert_eq!(errors("# Ada\nEngineer\nLondon\n"), 1);
        assert_eq!(errors("# Ada\nEngineer\nLondon · +44 20 7946 0000\n"), 0);
        assert_eq!(errors("# <Text color=\"red\"></Text>\nLondon · a@b.c\n"), 1); // a styled but empty name
    }

    #[test]
    fn posting_terms_and_coverage() {
        let posting = "We are hiring a Backend Engineer. You will build payment systems in Rust. \
            Experience with Kafka and distributed systems is required. Payment systems at scale; \
            distributed systems on AWS. Bonus: C++ or Node.js.";
        let terms = key_terms(posting);
        for t in ["payment systems", "distributed systems", "rust", "kafka", "aws", "c++", "node.js"] {
            assert!(terms.contains(&t.to_string()), "{t} missing from {terms:?}");
        }
        assert!(!terms.iter().any(|t| t == "experience" || t == "required" || t == "the"));

        let k = coverage(&terms, "Built payment system in Rust on AWS with Kafka. Wrote Node.js tooling.");
        assert!(k.found.contains(&"payment systems".to_string()));
        assert!(k.missing.contains(&"distributed systems".to_string()));
        assert!(k.coverage > 0 && k.coverage < 100);
        assert_eq!(coverage(&[], "x").coverage, 100);
    }
}
