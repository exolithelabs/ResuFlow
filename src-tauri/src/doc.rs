//! Minimal front matter for Markdown files.
//!
//! Supported syntax (a deliberately small YAML subset):
//!
//! ```text
//! ---
//! title: Senior Backend Engineer
//! tags: [rust, backend]
//! ---
//! body...
//! ```
//!
//! Values are strings or `[a, b]` lists. Key order is preserved on write.

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
        let Some(rest) = src
            .strip_prefix("---\n")
            .or_else(|| src.strip_prefix("---\r\n"))
        else {
            return Doc { meta: vec![], body: src.to_string() };
        };
        let mut meta = Vec::new();
        let mut offset = 0;
        for line in rest.split_inclusive('\n') {
            offset += line.len();
            let l = line.trim_end_matches(['\r', '\n']);
            if l.trim() == "---" {
                let body = rest[offset..].trim_start_matches(['\r', '\n']).to_string();
                return Doc { meta, body };
            }
            if l.trim().is_empty() || l.trim_start().starts_with('#') {
                continue;
            }
            if let Some((k, v)) = l.split_once(':') {
                meta.push((k.trim().to_string(), parse_value(v.trim())));
            }
        }
        // No closing fence: treat everything as body.
        Doc { meta: vec![], body: src.to_string() }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn str(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Value::Str(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::List(l)) => l.clone(),
            Some(Value::Str(s)) if !s.is_empty() => {
                s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
            }
            _ => vec![],
        }
    }

    pub fn set(&mut self, key: &str, value: Value) {
        let empty = matches!(&value, Value::Str(s) if s.is_empty())
            || matches!(&value, Value::List(l) if l.is_empty());
        if let Some(i) = self.meta.iter().position(|(k, _)| k == key) {
            if empty {
                self.meta.remove(i);
            } else {
                self.meta[i].1 = value;
            }
        } else if !empty {
            self.meta.push((key.to_string(), value));
        }
    }

    pub fn to_string(&self) -> String {
        if self.meta.is_empty() {
            return self.body.clone();
        }
        let mut out = String::from("---\n");
        for (k, v) in &self.meta {
            out.push_str(k);
            out.push_str(": ");
            match v {
                Value::Str(s) => out.push_str(&quote_if_needed(s)),
                Value::List(l) => {
                    out.push('[');
                    out.push_str(&l.iter().map(|s| quote_if_needed(s)).collect::<Vec<_>>().join(", "));
                    out.push(']');
                }
            }
            out.push('\n');
        }
        out.push_str("---\n\n");
        out.push_str(self.body.trim_start_matches(['\r', '\n']));
        out
    }
}

fn parse_value(v: &str) -> Value {
    if let Some(inner) = v.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
        return Value::List(
            inner.split(',').map(|s| unquote(s.trim())).filter(|s| !s.is_empty()).collect(),
        );
    }
    Value::Str(unquote(v))
}

fn unquote(s: &str) -> String {
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return s[1..s.len() - 1].replace("\\\"", "\"");
        }
    }
    s.to_string()
}

fn quote_if_needed(s: &str) -> String {
    let risky = s.contains([',', '[', ']', '#', '"', '\''])
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.contains(": ");
    if risky {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = "---\ntitle: Backend, Rust\ntags: [a, b]\nstatus: draft\n---\n\n# Hi\n";
        let d = Doc::parse(src);
        assert_eq!(d.str("title").unwrap(), "Backend, Rust");
        assert_eq!(d.list("tags"), vec!["a", "b"]);
        assert_eq!(d.body, "# Hi\n");
        let again = Doc::parse(&d.to_string());
        assert_eq!(again, d);
    }

    #[test]
    fn no_front_matter() {
        let d = Doc::parse("# Plain\n");
        assert!(d.meta.is_empty());
        assert_eq!(d.body, "# Plain\n");
    }

    #[test]
    fn unclosed_fence_is_body() {
        let d = Doc::parse("---\ntitle: x\n# body");
        assert!(d.meta.is_empty());
    }
}
