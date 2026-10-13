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
            LText::Map(m) => m.get(lang).or_else(|| m.get("en")).or_else(|| m.get("ru")).or_else(|| m.values().next()).cloned().unwrap_or_default(),
        }
    }

    /// Resolve and substitute `{var}` / `{var|dur}` placeholders.
    pub fn render(&self, lang: &str, vars: &BTreeMap<String, Value>) -> String {
        interpolate_l(&self.get(lang), vars, lang)
    }
}

/// Replace `{name}` with the value of `vars[name]`. Unknown placeholders are left untouched so
/// that a typo in a configured template is visible instead of silently swallowed.
pub fn interpolate(tpl: &str, vars: &BTreeMap<String, Value>) -> String {
    interpolate_l(tpl, vars, "en")
}

/// Like [`interpolate`], with formats: `{seconds|dur}` renders a duration (`35 s`, `15 min`,
/// `2 h`, in the given language), `{ratio|pct}` a ratio as a percentage and
/// `{n|plural:площадка,площадки,площадок}` the word form that agrees with the number (three forms for
/// Russian-like languages, two – singular and plural – for the others).
pub fn interpolate_l(tpl: &str, vars: &BTreeMap<String, Value>, lang: &str) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let spec = &after[..end];
                let (key, format) = spec.split_once('|').map(|(k, f)| (k, Some(f))).unwrap_or((spec, None));
                match vars.get(key) {
                    Some(v) => out.push_str(&format_value(v, format, lang)),
                    None => {
                        out.push('{');
                        out.push_str(spec);
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

fn format_value(v: &Value, format: Option<&str>, lang: &str) -> String {
    match (format, v.as_f64()) {
        (Some("dur"), Some(s)) => format_duration(s, lang),
        (Some("pct"), Some(r)) => format!("{}%", (r * 100.0).round() as i64),
        (Some(f), Some(n)) if f.starts_with("plural:") => plural_form(n, &f["plural:".len()..], lang),
        _ => display_value(v),
    }
}

/// The word form for `n` out of `forms` (comma separated). Russian, Ukrainian and Belarusian use the one / few / many
/// rule; every other language picks the first form for exactly one and the last for the rest.
pub fn plural_form(n: f64, forms: &str, lang: &str) -> String {
    let list: Vec<&str> = forms.split(',').map(str::trim).collect();
    let pick = |i: usize| list.get(i).or_else(|| list.last()).copied().unwrap_or("").to_string();
    let n = n.abs().round() as i64;
    if matches!(lang, "ru" | "uk" | "be") && list.len() >= 3 {
        let (m10, m100) = (n % 10, n % 100);
        return if m10 == 1 && m100 != 11 {
            pick(0)
        } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
            pick(1)
        } else {
            pick(2)
        };
    }
    if n == 1 {
        pick(0)
    } else {
        pick(list.len().saturating_sub(1))
    }
}

/// `35 с` / `15 мин` / `2 ч` / `3 дн` (or `s` / `min` / `h` / `d`): the unit is picked so that the
/// number stays short.
pub fn format_duration(s: f64, lang: &str) -> String {
    let (sec, min, hour, day) = if lang == "ru" { ("с", "мин", "ч", "дн") } else { ("s", "min", "h", "d") };
    let short = |x: f64| {
        let r = (x * 10.0).round() / 10.0;
        if r.fract() == 0.0 {
            format!("{}", r as i64)
        } else {
            format!("{r}")
        }
    };
    let s = s.max(0.0);
    if s < 120.0 {
        format!("{} {sec}", short(s))
    } else if s < 7200.0 {
        format!("{} {min}", short(s / 60.0))
    } else if s < 172_800.0 {
        format!("{} {hour}", short(s / 3600.0))
    } else {
        format!("{} {day}", short(s / 86_400.0))
    }
}

pub fn display_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    return format!("{}", f as i64);
                }
                return format!("{:.2}", f).trim_end_matches('0').trim_end_matches('.').to_string();
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
    fn formats_durations_and_percentages() {
        let mut vars = BTreeMap::new();
        vars.insert("a".into(), json!(35.0));
        vars.insert("b".into(), json!(900.0));
        vars.insert("c".into(), json!(7200.0));
        vars.insert("d".into(), json!(259_200.0));
        vars.insert("r".into(), json!(0.96));
        assert_eq!(interpolate_l("{a|dur} {b|dur} {c|dur} {d|dur} {r|pct}", &vars, "ru"), "35 с 15 мин 2 ч 3 дн 96%");
        assert_eq!(interpolate_l("{a|dur} {b|dur}", &vars, "en"), "35 s 15 min");
        assert_eq!(interpolate_l("{zzz|dur}", &vars, "en"), "{zzz|dur}");
    }

    #[test]
    fn plural_forms_follow_the_language() {
        let mut vars = BTreeMap::new();
        for (k, n) in [("a", 1), ("b", 2), ("c", 5), ("d", 11), ("e", 21), ("f", 22), ("g", 112)] {
            vars.insert(k.to_string(), json!(n));
        }
        let ru = |k: &str| interpolate_l(&format!("{{{k}}} {{{k}|plural:площадка,площадки,площадок}}"), &vars, "ru");
        assert_eq!([ru("a"), ru("b"), ru("c"), ru("d"), ru("e"), ru("f"), ru("g")], ["1 площадка", "2 площадки", "5 площадок", "11 площадок", "21 площадка", "22 площадки", "112 площадок"]);
        let en = |k: &str| interpolate_l(&format!("{{{k}}} {{{k}|plural:site,sites}}"), &vars, "en");
        assert_eq!([en("a"), en("b"), en("d")], ["1 site", "2 sites", "11 sites"]);
    }

    #[test]
    fn ltext_fallbacks() {
        let t: LText = serde_json::from_value(json!({"ru": "привет"})).unwrap();
        assert_eq!(t.get("en"), "привет");
        let t: LText = serde_json::from_value(json!("plain")).unwrap();
        assert_eq!(t.get("ru"), "plain");
    }
}
