//! Front matter for Markdown files: a deliberately tiny YAML subset.
//!
//! ```text
//! ---
//! title: Senior Backend Engineer
//! tags: [rust, "payments, billing"]
//! ---
//! body...
//! ```
//!
//! A value is a single-line string or a `[a, b]` list. Keys keep their order on write, and
//! comments and blank lines inside the fence are dropped.

use std::fmt;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Doc {
    pub meta: Vec<(String, Value)>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    List(Vec<String>),
}

impl Doc {
    pub fn parse(src: &str) -> Doc {
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        let plain = || Doc { meta: vec![], body: src.to_string() };
        let Some(rest) = src.strip_prefix("---\n").or_else(|| src.strip_prefix("---\r\n")) else {
            return plain();
        };
        let mut meta = Vec::new();
        let mut consumed = 0;
        for line in rest.split_inclusive('\n') {
            consumed += line.len();
            let l = line.trim();
            if l == "---" {
                let body = rest[consumed..].trim_start_matches(['\r', '\n']).to_string();
                return Doc { meta, body };
            }
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = l.split_once(':') {
                meta.push((k.trim().to_string(), parse_value(v.trim())));
            }
        }
        plain() // no closing fence
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A non-empty string value.
    pub fn str(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Value::Str(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }

    /// A list value. A plain string is read as a comma-separated list.
    pub fn list(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::List(l)) => l.clone(),
            Some(Value::Str(s)) => s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect(),
            None => vec![],
        }
    }

    /// Set a field, or remove it when the value is empty. Newlines become spaces.
    pub fn set(&mut self, key: &str, value: Value) {
        let one_line = |s: String| s.replace(['\r', '\n'], " ").trim().to_string();
        let value = match value {
            Value::Str(s) => Value::Str(one_line(s)),
            Value::List(l) => Value::List(l.into_iter().map(one_line).filter(|x| !x.is_empty()).collect()),
        };
        let empty = match &value {
            Value::Str(s) => s.is_empty(),
            Value::List(l) => l.is_empty(),
        };
        let at = self.meta.iter().position(|(k, _)| k == key);
        match (at, empty) {
            (Some(i), true) => {
                self.meta.remove(i);
            }
            (Some(i), false) => self.meta[i].1 = value,
            (None, false) => self.meta.push((key.to_string(), value)),
            (None, true) => {}
        }
    }
}

impl fmt::Display for Doc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.meta.is_empty() {
            return f.write_str(&self.body);
        }
        f.write_str("---\n")?;
        for (k, v) in &self.meta {
            match v {
                Value::Str(s) => writeln!(f, "{k}: {}", quote(s))?,
                Value::List(l) => writeln!(f, "{k}: [{}]", l.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", "))?,
            }
        }
        write!(f, "---\n\n{}", self.body.trim_start_matches(['\r', '\n']))
    }
}

fn parse_value(v: &str) -> Value {
    match v.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
        Some(inner) => Value::List(split_list(inner).into_iter().map(|s| unquote(&s)).filter(|s| !s.is_empty()).collect()),
        None => Value::Str(unquote(v)),
    }
}

/// Split on commas that are not inside quotes.
fn split_list(s: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in s.chars() {
        let cur = out.last_mut().unwrap();
        match (quote, c) {
            (Some('"'), _) if escaped => escaped = false,
            (Some('"'), '\\') => escaped = true,
            (Some(q), _) if c == q => quote = None,
            (None, '"' | '\'') if cur.trim().is_empty() => quote = Some(c),
            (None, ',') => {
                out.push(String::new());
                continue;
            }
            _ => {}
        }
        out.last_mut().unwrap().push(c);
    }
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let mut out = String::new();
        let mut chars = s[1..s.len() - 1].chars();
        while let Some(c) = chars.next() {
            out.push(if c == '\\' { chars.next().unwrap_or('\\') } else { c });
        }
        return out;
    }
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace("''", "'");
    }
    s.to_string()
}

fn quote(s: &str) -> String {
    let risky = s.contains([',', '[', ']', '#', '"', '\'', '\\'])
        || s.contains(": ")
        || s.ends_with(':')
        || s.starts_with(['-', '{', '&', '*', '!', '|', '>', '%', '@', '`', ' '])
        || s.ends_with(' ');
    if risky {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = "---\ntitle: Backend, Rust\ntags: [a, \"b, c\", 'd''s']\nstatus: draft\n---\n\n# Hi\n";
        let d = Doc::parse(src);
        assert_eq!(d.str("title").unwrap(), "Backend, Rust");
        assert_eq!(d.list("tags"), vec!["a", "b, c", "d's"]);
        assert_eq!(d.body, "# Hi\n");
        assert_eq!(Doc::parse(&d.to_string()), d);
    }

    #[test]
    fn escapes_survive() {
        let mut d = Doc::default();
        d.set("a", Value::Str(r#"say "hi" \o/"#.into()));
        d.set("b", Value::Str("two\nlines".into()));
        d.set("c", Value::Str("+91 98765 43210".into()));
        let again = Doc::parse(&d.to_string());
        assert_eq!(again.str("a").unwrap(), r#"say "hi" \o/"#);
        assert_eq!(again.str("b").unwrap(), "two lines");
        assert_eq!(again.str("c").unwrap(), "+91 98765 43210");
    }

    #[test]
    fn empty_value_removes_key() {
        let mut d = Doc::parse("---\na: 1\nb: 2\n---\nx");
        d.set("a", Value::Str(String::new()));
        d.set("z", Value::List(vec![]));
        assert_eq!(d.meta.len(), 1);
    }

    #[test]
    fn no_or_unclosed_front_matter_is_body() {
        assert_eq!(Doc::parse("# Plain\n").body, "# Plain\n");
        assert!(Doc::parse("---\ntitle: x\n# body").meta.is_empty());
    }
}
