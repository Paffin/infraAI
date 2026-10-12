//! Localised text and `{placeholder}` interpolation.
//!
//! Every user-facing string that comes from configuration (rule titles, catalog names, …) is an
//! [`LText`]: either a plain string or a `{lang: text}` map. Nothing in the engine hardcodes
//! prose; the engine only emits codes and variables, texts are rendered from config.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum LText {
    Plain(String),
    Map(BTreeMap<String, String>),
}

impl Default for LText {
    fn default() -> Self {
        LText::Plain(String::new())
    }
}

impl LText {
    pub fn plain(s: impl Into<String>) -> Self {
        LText::Plain(s.into())
    }

    /// Resolve for `lang`, falling back to `en`, then `ru`, then any available value.
    pub fn get(&self, lang: &str) -> String {
        match self {
            LText::Plain(s) => s.clone(),
            LText::Map(m) => m
                .get(lang)
                .or_else(|| m.get("en"))
                .or_else(|| m.get("ru"))
                .or_else(|| m.values().next())
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// Resolve and substitute `{var}` placeholders.
    pub fn render(&self, lang: &str, vars: &BTreeMap<String, Value>) -> String {
        interpolate(&self.get(lang), vars)
    }
}

/// Replace `{name}` with the value of `vars[name]`. Unknown placeholders are left untouched so
/// that a typo in a configured template is visible instead of silently swallowed.
pub fn interpolate(tpl: &str, vars: &BTreeMap<String, Value>) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(v) => out.push_str(&display_value(v)),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn display_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    return format!("{}", f as i64);
                }
                return format!("{:.2}", f)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string();
            }
            n.to_string()
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "—".to_string(),
        Value::Array(a) => a.iter().map(display_value).collect::<Vec<_>>().join(", "),
        Value::Object(_) => v.to_string(),
    }
}

/// Human-readable duration used in messages (`35 с`, `4 мин`, `2 ч`) – language neutral digits +
/// short units supplied by caller.
pub fn fmt_seconds(s: f64) -> String {
    if s < 120.0 {
        format!("{}s", s.round() as i64)
    } else if s < 7200.0 {
        format!("{}m", (s / 60.0).round() as i64)
    } else {
        format!("{}h", (s / 3600.0 * 10.0).round() / 10.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn interpolates_and_keeps_unknown() {
        let mut vars = BTreeMap::new();
        vars.insert("node".into(), json!("PG"));
        vars.insert("n".into(), json!(3.0));
        assert_eq!(interpolate("{node} x{n} {missing}", &vars), "PG x3 {missing}");
    }

    #[test]
    fn ltext_fallbacks() {
        let t: LText = serde_json::from_value(json!({"ru": "привет"})).unwrap();
        assert_eq!(t.get("en"), "привет");
        let t: LText = serde_json::from_value(json!("plain")).unwrap();
        assert_eq!(t.get("ru"), "plain");
    }
}
