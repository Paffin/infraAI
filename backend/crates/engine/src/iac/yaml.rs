//! Minimal, correct YAML writer for JSON-shaped data.
//!
//! IaC files are produced from structured values – never by pasting strings into text – so a
//! node named `a: b # c` or containing a newline can neither break the document nor inject
//! extra keys. Strings are written plain only when that is unambiguous; everything else is a
//! JSON-quoted scalar (which is valid YAML).

use serde_json::Value;

/// Serialise one document (no leading `---`). Object key order is preserved.
pub fn to_yaml(v: &Value) -> String {
    let mut out = String::new();
    match v {
        Value::Object(m) if !m.is_empty() => write_map(m, 0, &mut out),
        Value::Array(a) if !a.is_empty() => write_seq(a, 0, &mut out),
        other => {
            out.push_str(&scalar(other));
            out.push('\n');
        }
    }
    out
}

/// Several documents joined with `---`.
pub fn to_yaml_stream(docs: &[Value]) -> String {
    docs.iter().map(to_yaml).collect::<Vec<_>>().join("---\n")
}

fn pad(n: usize) -> String {
    "  ".repeat(n)
}

fn write_map(m: &serde_json::Map<String, Value>, indent: usize, out: &mut String) {
    for (k, v) in m {
        out.push_str(&pad(indent));
        out.push_str(&key(k));
        out.push(':');
        write_value_after_key(v, indent, out);
    }
}

fn write_value_after_key(v: &Value, indent: usize, out: &mut String) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            out.push('\n');
            write_map(m, indent + 1, out);
        }
        Value::Array(a) if !a.is_empty() => {
            out.push('\n');
            // sequences are indented under their key, which keeps `kubectl`/yamllint conventions
            write_seq(a, indent + 1, out);
        }
        Value::String(s) if s.contains('\n') && block_ok(s) => {
            out.push(' ');
            write_block(s, indent + 1, out);
        }
        other => {
            out.push(' ');
            out.push_str(&scalar(other));
            out.push('\n');
        }
    }
}

fn write_seq(a: &[Value], indent: usize, out: &mut String) {
    for v in a {
        out.push_str(&pad(indent));
        out.push('-');
        match v {
            Value::Object(m) if !m.is_empty() => {
                // first key shares the dash line
                let mut first = true;
                for (k, val) in m {
                    if first {
                        out.push(' ');
                        first = false;
                    } else {
                        out.push_str(&pad(indent + 1));
                    }
                    out.push_str(&key(k));
                    out.push(':');
                    write_value_after_key(val, indent + 1, out);
                }
            }
            Value::Array(inner) if !inner.is_empty() => {
                out.push('\n');
                write_seq(inner, indent + 1, out);
            }
            Value::String(s) if s.contains('\n') && block_ok(s) => {
                out.push(' ');
                write_block(s, indent + 1, out);
            }
            other => {
                out.push(' ');
                out.push_str(&scalar(other));
                out.push('\n');
            }
        }
    }
}

/// Literal block scalars are used for multi-line text when every line can be written verbatim.
fn block_ok(s: &str) -> bool {
    !s.is_empty() && !s.starts_with(' ') && !s.starts_with('\n') && !s.contains('\r') && !s.contains('\t') && s.lines().all(|l| !l.ends_with(' ')) && !s.chars().any(|c| c.is_control() && c != '\n')
}

fn write_block(s: &str, indent: usize, out: &mut String) {
    let keep = if s.ends_with('\n') {
        if s.ends_with("\n\n") {
            "|+"
        } else {
            "|"
        }
    } else {
        "|-"
    };
    out.push_str(keep);
    out.push('\n');
    let body = s.strip_suffix('\n').unwrap_or(s);
    for line in body.split('\n') {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&pad(indent));
            out.push_str(line);
            out.push('\n');
        }
    }
}

fn key(k: &str) -> String {
    if plain_ok(k) {
        k.to_string()
    } else {
        quote(k)
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number(n),
        Value::String(s) => {
            if plain_ok(s) {
                s.clone()
            } else {
                quote(s)
            }
        }
        Value::Array(_) => "[]".into(),
        Value::Object(_) => "{}".into(),
    }
}

/// Integral floats are written as integers (`3.0` → `3`): Kubernetes rejects `replicas: 3.0`.
pub fn number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    match n.as_f64() {
        Some(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 => format!("{}", f as i64),
        Some(f) => format!("{}", f),
        None => n.to_string(),
    }
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Can `s` be written without quotes and still be read back as the same string?
fn plain_ok(s: &str) -> bool {
    if s.is_empty() || s.len() > 200 {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !(first.is_ascii_alphanumeric() || first == '_' || first == '/' || first == '.') {
        return false;
    }
    // characters that are always safe inside a plain scalar; single inner spaces are allowed
    let mut prev_space = false;
    for c in s.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-' | '@' | '=' | '+' | ',' | '(' | ')' | ':') || (c == ' ' && !prev_space);
        if !ok {
            return false;
        }
        prev_space = c == ' ';
    }
    if s.ends_with(' ') || s.ends_with(':') || s.contains(": ") || s.contains(":,") {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "y" | "n" | "~" | "nan" | ".nan" | ".inf" | "-.inf" | "+.inf") {
        return false;
    }
    // numbers, hex/octal/binary, 1e3, 1_000, version-like floats and dates would change type
    if s.parse::<f64>().is_ok() || looks_numeric(s) || looks_like_date(s) {
        return false;
    }
    true
}

fn looks_numeric(s: &str) -> bool {
    let t = s.trim_start_matches(['+', '-']);
    if t.is_empty() {
        return false;
    }
    if t.len() > 2 && (t.starts_with("0x") || t.starts_with("0o") || t.starts_with("0b")) {
        return true;
    }
    let mut seen_digit = false;
    for c in t.chars() {
        match c {
            '0'..='9' => seen_digit = true,
            '.' | '_' | 'e' | 'E' | '+' | '-' | ':' => {}
            _ => return false,
        }
    }
    seen_digit
}

fn looks_like_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 8 && b[..4].iter().all(|c| c.is_ascii_digit()) && b[4] == b'-' && s[5..].chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scalars_are_quoted_only_when_needed() {
        let y = to_yaml(
            &json!({"name": "pg-main", "size": "500Gi", "num": 3, "f": 1.5, "i": 3.0, "t": true, "z": null, "v": "16", "e": "", "b": "true", "c": "a: b", "h": "# x", "d": "2024-01-02", "s": "two words"}),
        );
        assert!(y.contains("name: pg-main\n"), "{y}");
        assert!(y.contains("size: 500Gi\n"));
        assert!(y.contains("num: 3\n") && y.contains("f: 1.5\n") && y.contains("i: 3\n"));
        assert!(y.contains("t: true\n") && y.contains("z: null\n"));
        assert!(y.contains("v: \"16\"\n"), "numeric-looking strings stay strings: {y}");
        assert!(y.contains("e: \"\"\n") && y.contains("b: \"true\"\n"));
        assert!(y.contains("c: \"a: b\"\n") && y.contains("h: \"# x\"\n"));
        assert!(y.contains("d: \"2024-01-02\"\n"));
        assert!(y.contains("s: two words\n"));
        // YAML 1.1 readers treat bare y/n/on/off as booleans – keys are quoted too
        assert!(to_yaml(&json!({"n": 1, "on": 2})).contains("\"n\": 1"));
    }

    #[test]
    fn nesting_sequences_and_empty_containers() {
        let y = to_yaml(&json!({"a": {"b": [1, {"c": 2, "d": [3, 4]}], "e": {}, "f": []}, "g": [[1, 2], []]}));
        let expect = "a:\n  b:\n    - 1\n    - c: 2\n      d:\n        - 3\n        - 4\n  e: {}\n  f: []\ng:\n  -\n    - 1\n    - 2\n  - []\n";
        assert_eq!(y, expect);
    }

    #[test]
    fn hostile_strings_cannot_break_out() {
        let y = to_yaml(&json!({"k": "x\ninjected: true", "q": "a\"b", "bs": "back\\slash", "colon:key": "v", "- dash": 1}));
        assert!(y.contains("k: |-\n  x\n  injected: true\n"), "{y}");
        assert!(y.contains("q: a\"b\n") || y.contains("q: \"a\\\"b\"\n"), "{y}");
        assert!(y.contains("\"colon:key\": v") || y.contains("colon:key: v"));
        assert!(y.contains("\"- dash\": 1"));
    }

    #[test]
    fn documents_are_joined() {
        let y = to_yaml_stream(&[json!({"a": 1}), json!({"b": 2})]);
        assert_eq!(y, "a: 1\n---\nb: 2\n");
    }
}
