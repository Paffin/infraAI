//! Runtime settings: a typed registry (`defaults/settings.json`) + values in PostgreSQL.
//! The admin panel renders its forms from the registry, so adding a setting never needs UI work.
//! Secrets are encrypted at rest and never returned by the API.

use crate::crypto::Keys;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool};
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingDef {
    pub key: String,
    pub group: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub default: Value,
    pub label: Value,
    #[serde(default)]
    pub help: Option<Value>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// What a text setting (`string`, `string_list`, `model`, `secret`) is, beyond "a string": see [`FORMATS`]. Checked whenever the value is saved - by an
    /// administrator, by a configuration import - so a value that cannot work is refused with its key named instead of failing later, somewhere else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// The formats of text settings and what each one accepts:
///
/// * `single_line` - one line of at most [`MAX_LINE_CHARS`] characters, no control characters (a carriage return or a line feed in a value that goes
///   into an HTTP header makes the request unbuildable). Empty is allowed.
/// * `http_url` - the address of an endpoint or a base the server calls: `http://` or `https://`, a host, no user name or password, no query, no fragment.
///   Empty only where the default is empty.
/// * `http_link` - the same for a link a person follows (a documentation page): a query and a fragment are fine.
/// * `http_origin` - `scheme://host[:port]` and nothing after it (the `Origin` check compares exactly that). It is stored as the browser writes an origin:
///   the host in lower case, no default port, no trailing slash.
/// * `line_list` - a list of `single_line` values.
/// * `url_list` - a list of `http_url` values, each stored with a trailing `/` so that a base is a directory (`.../api` cannot admit `.../api-evil/`).
pub const FORMATS: &[&str] = &["single_line", "http_url", "http_link", "http_origin", "line_list", "url_list"];

/// The longest a `single_line` value is. Part of what the format means, not a tunable.
pub const MAX_LINE_CHARS: usize = 200;
/// The longest an address is. Part of what the formats mean, not a tunable (browsers and proxies refuse longer ones anyway).
pub const MAX_URL_CHARS: usize = 2048;

impl SettingDef {
    /// The types whose values are text and so must say what kind of text (`format`), and the formats that fit each.
    pub fn takes_format(ty: &str) -> Option<&'static [&'static str]> {
        match ty {
            "string" => Some(&["single_line", "http_url", "http_link", "http_origin"]),
            "model" | "secret" => Some(&["single_line"]),
            "string_list" => Some(&["line_list", "url_list"]),
            _ => None,
        }
    }

    /// May the empty string be saved? It is a way to say "not set" for a link, an origin, a header value; an endpoint the server cannot work without has
    /// a default that is not empty, and stays that way.
    fn allows_empty(&self) -> bool {
        self.format.as_deref() == Some("single_line") || self.default.as_str() == Some("")
    }
}

/// Why a value was refused. `code` is the stable part: the interface puts it in its own words, in the language of the person (the English sentence is for the API
/// and the log, and says nothing to an administrator who reads Russian); `params` are the numbers and the names that sentence needs. Codes the interface does not
/// know yet are shown by their English `text`.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub code: &'static str,
    pub params: Value,
    text: String,
}

impl Refusal {
    pub fn new(code: &'static str, params: Value, text: impl Into<String>) -> Self {
        Refusal { code, params, text: text.into() }
    }

    /// The same refusal for one entry of a list: the sentence names the entry, and the parameters carry it (`value`, at most 60 characters).
    fn of_entry(mut self, entry: &str) -> Self {
        let shown: String = entry.chars().take(60).collect();
        self.text = format!("'{shown}' {}", self.text);
        match &mut self.params {
            Value::Object(m) => {
                m.insert("value".into(), json!(shown));
            }
            other => *other = json!({"value": shown}),
        }
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

/// A refusal nobody gave a code of its own: `invalid`, with its sentence.
impl From<&str> for Refusal {
    fn from(text: &str) -> Self {
        Refusal::new("invalid", Value::Null, text)
    }
}

impl From<String> for Refusal {
    fn from(text: String) -> Self {
        Refusal::new("invalid", Value::Null, text)
    }
}

fn check_line(s: &str) -> Result<String, Refusal> {
    if s.chars().any(char::is_control) {
        return Err(Refusal::new("not_single_line", Value::Null, "must be a single line, without line breaks or other control characters"));
    }
    if s.chars().count() > MAX_LINE_CHARS {
        return Err(Refusal::new("line_too_long", json!({"max": MAX_LINE_CHARS}), format!("must be at most {MAX_LINE_CHARS} characters")));
    }
    Ok(s.to_string())
}

/// An address that is http or https, has a host and no credentials. Returns it parsed; the caller adds what its format forbids.
fn parse_http(s: &str) -> Result<reqwest::Url, Refusal> {
    let lower = s.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(Refusal::new("not_http", Value::Null, "must be an address that starts with http:// or https://"));
    }
    // the URL parser silently drops tabs and line breaks and reads a backslash as a slash: what is stored must be what it will make of it
    if s.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\') {
        return Err(Refusal::new("address_chars", Value::Null, "must not contain spaces, backslashes or control characters"));
    }
    if s.chars().count() > MAX_URL_CHARS {
        return Err(Refusal::new("address_too_long", json!({"max": MAX_URL_CHARS}), format!("must be at most {MAX_URL_CHARS} characters")));
    }
    let u = reqwest::Url::parse(s).map_err(|e| Refusal::new("bad_address", Value::Null, format!("is not a valid address ({e})")))?;
    if u.host_str().is_none_or(str::is_empty) {
        return Err(Refusal::new("no_host", Value::Null, "must name a host"));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(Refusal::new("has_credentials", Value::Null, "must not contain a user name or a password"));
    }
    Ok(u)
}

fn check_format(format: &str, s: &str) -> Result<String, Refusal> {
    match format {
        "single_line" | "line_list" => check_line(s),
        "http_link" => parse_http(s).map(|_| s.to_string()),
        "http_url" | "url_list" => {
            let u = parse_http(s)?;
            if u.query().is_some() || u.fragment().is_some() {
                return Err(Refusal::new("has_query", Value::Null, "must not contain a query (?) or a fragment (#)"));
            }
            if format == "url_list" && !s.ends_with('/') {
                return Ok(format!("{s}/"));
            }
            Ok(s.to_string())
        }
        "http_origin" => {
            let u = parse_http(s)?;
            // what follows the host, other than slashes, is a path: the parser would read `/.` as the root, the `Origin` check would not
            let after_scheme = s.split_once("://").map_or(s, |(_, rest)| rest);
            if u.query().is_some() || u.fragment().is_some() || after_scheme.trim_end_matches('/').contains('/') {
                return Err(Refusal::new("not_origin", Value::Null, "must be an origin, scheme://host[:port], without a path, a query or a fragment"));
            }
            Ok(u.origin().ascii_serialization())
        }
        other => Err(format!("unknown format {other}").into()),
    }
}

pub const SETTINGS_JSON: &str = include_str!("../defaults/settings.json");

pub fn registry() -> &'static Vec<SettingDef> {
    static R: OnceLock<Vec<SettingDef>> = OnceLock::new();
    R.get_or_init(|| serde_json::from_str(SETTINGS_JSON).expect("embedded settings.json is valid"))
}

pub fn def(key: &str) -> Option<&'static SettingDef> {
    registry().iter().find(|d| d.key == key)
}

/// Validate and normalise a value for a setting. Returns the value to store, or the [`Refusal`] (a code for the interface, and a sentence in English).
pub fn validate(def: &SettingDef, v: &Value) -> Result<Value, Refusal> {
    let expected = |what: &'static str, text: &'static str| Refusal::new("wrong_type", json!({"expected": what}), text);
    match def.ty.as_str() {
        "string" | "model" | "secret" => match v {
            Value::String(s) => {
                let s = s.trim();
                match def.format.as_deref() {
                    Some(_) if s.is_empty() && def.allows_empty() => Ok(Value::String(String::new())),
                    Some(_) if s.is_empty() => Err(Refusal::new("empty", Value::Null, "must not be empty")),
                    Some(f) => check_format(f, s).map(Value::String),
                    None => Ok(Value::String(s.to_string())),
                }
            }
            _ => Err(expected("string", "expected a string")),
        },
        "enum" => {
            let s = v.as_str().ok_or_else(|| expected("string", "expected a string"))?;
            let options = def.options.clone().unwrap_or_default();
            if options.iter().any(|x| x == s) {
                Ok(json!(s))
            } else {
                Err(Refusal::new("not_an_option", json!({"options": options.join(", ")}), format!("must be one of: {}", options.join(", "))))
            }
        }
        "number" => {
            let n = match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse::<f64>().ok(),
                _ => None,
            }
            .ok_or_else(|| expected("number", "expected a number"))?;
            if let Some(min) = def.min {
                if n < min {
                    return Err(Refusal::new("below_min", json!({"min": min}), format!("must be ≥ {min}")));
                }
            }
            if let Some(max) = def.max {
                if n > max {
                    return Err(Refusal::new("above_max", json!({"max": max}), format!("must be ≤ {max}")));
                }
            }
            Ok(json!(n))
        }
        "boolean" => v.as_bool().map(|b| json!(b)).ok_or_else(|| expected("boolean", "expected true or false")),
        "string_list" => {
            let a = v.as_array().ok_or_else(|| expected("list", "expected a list of strings"))?;
            let mut out = vec![];
            for x in a {
                let s = x.as_str().ok_or_else(|| expected("list", "expected a list of strings"))?.trim();
                if s.is_empty() {
                    continue;
                }
                match def.format.as_deref() {
                    Some(f) => out.push(json!(check_format(f, s).map_err(|why| why.of_entry(s))?)),
                    None => out.push(json!(s)),
                }
            }
            Ok(Value::Array(out))
        }
        "json" => {
            if v.is_object() || v.is_array() {
                Ok(v.clone())
            } else {
                Err(expected("object", "expected a JSON object"))
            }
        }
        other => Err(format!("unknown setting type {other}").into()),
    }
}

#[derive(Clone, Default)]
pub struct Settings {
    values: HashMap<String, Value>,
    /// keys whose value comes from the database (vs. default / environment)
    from_db: HashMap<String, bool>,
}

impl Settings {
    /// The shipped defaults of every setting – no database involved.
    pub fn defaults() -> Self {
        Settings { values: registry().iter().map(|d| (d.key.clone(), d.default.clone())).collect(), from_db: HashMap::new() }
    }

    /// The same settings with `key` set to `value` (validated like an administrator's change).
    pub fn with(mut self, key: &str, value: Value) -> Result<Self, String> {
        let d = def(key).ok_or_else(|| format!("unknown setting '{key}'"))?;
        let v = validate(d, &value).map_err(|e| format!("{key}: {e}"))?;
        self.values.insert(key.to_string(), v);
        self.from_db.insert(key.to_string(), true);
        Ok(self)
    }

    pub async fn load(pool: &PgPool, keys: &Keys) -> Result<Self, sqlx::Error> {
        let rows: Vec<(String, Value, bool)> = sqlx::query_as("select key, value, encrypted from settings").fetch_all(pool).await?;
        let mut values: HashMap<String, Value> = registry().iter().map(|d| (d.key.clone(), d.default.clone())).collect();
        let mut from_db = HashMap::new();
        for (k, v, enc) in rows {
            if def(&k).is_none() {
                continue; // removed setting
            }
            let v = if enc {
                match v.as_str().and_then(|s| keys.decrypt(s)) {
                    Some(plain) => Value::String(plain),
                    None => {
                        tracing::warn!(key = %k, "cannot decrypt secret setting (APP_SECRET changed?)");
                        continue;
                    }
                }
            } else {
                v
            };
            values.insert(k.clone(), v);
            from_db.insert(k, true);
        }
        Ok(Settings { values, from_db })
    }

    pub fn get(&self, key: &str) -> Value {
        self.values.get(key).cloned().or_else(|| def(key).map(|d| d.default.clone())).unwrap_or(Value::Null)
    }
    pub fn str(&self, key: &str) -> String {
        self.get(key).as_str().unwrap_or("").to_string()
    }
    pub fn num(&self, key: &str) -> f64 {
        self.get(key).as_f64().unwrap_or(0.0)
    }
    pub fn boolean(&self, key: &str) -> bool {
        self.get(key).as_bool().unwrap_or(false)
    }
    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key).as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
    }
    pub fn is_from_db(&self, key: &str) -> bool {
        self.from_db.get(key).copied().unwrap_or(false)
    }

    /// Settings as shown to administrators: secrets are masked.
    pub fn for_admin(&self, env_secret_present: &dyn Fn(&str) -> bool) -> Vec<Value> {
        registry()
            .iter()
            .map(|d| {
                let mut o = serde_json::to_value(d).unwrap_or(Value::Null);
                if d.ty == "secret" {
                    let set_db = self.is_from_db(&d.key) && !self.str(&d.key).is_empty();
                    o["value"] = Value::Null;
                    o["secretSet"] = json!(set_db);
                    o["secretSource"] = json!(if set_db {
                        "database"
                    } else if env_secret_present(&d.key) {
                        "environment"
                    } else {
                        "none"
                    });
                } else {
                    o["value"] = self.get(&d.key);
                    o["modified"] = json!(self.is_from_db(&d.key));
                }
                o
            })
            .collect()
    }
}

/// Integral floats are stored as integers (`12.0` → `12`) so that a value equal to its default is recognised.
fn canonical(v: Value) -> Value {
    match v.as_f64() {
        Some(f) if v.is_f64() && f.fract() == 0.0 && f.abs() < 9e15 => json!(f as i64),
        _ => v,
    }
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Why a change to the settings was not stored.
#[derive(Debug)]
pub enum SaveError {
    /// The registry refuses the value: unknown key, wrong type, out of range. Boxed: a `Refusal` is large and the `Err` of every caller would carry it.
    Invalid { key: String, reason: Box<Refusal> },
    /// PostgreSQL refused the value (a string with U+0000 cannot be kept in `jsonb`) or could not be reached.
    Store { key: String, source: sqlx::Error },
}

fn store(key: &str) -> impl FnOnce(sqlx::Error) -> SaveError + '_ {
    move |source| SaveError::Store { key: key.to_string(), source }
}

/// Persist changes in a transaction of their own. Secrets are encrypted; an empty secret removes the stored value.
pub async fn save(pool: &PgPool, keys: &Keys, changes: &serde_json::Map<String, Value>, actor: &str) -> Result<Vec<String>, SaveError> {
    let mut tx = pool.begin().await.map_err(store(""))?;
    let changed = save_in(&mut tx, keys, changes, actor).await?;
    tx.commit().await.map_err(store(""))?;
    Ok(changed)
}

/// Persist changes on the caller's connection, so that they can be part of a larger transaction (a configuration import writes items,
/// settings and the revision in one). All or nothing is the caller's: nothing is committed here, and after an error the transaction must be dropped.
pub async fn save_in(conn: &mut PgConnection, keys: &Keys, changes: &serde_json::Map<String, Value>, actor: &str) -> Result<Vec<String>, SaveError> {
    let mut changed = vec![];
    for (k, v) in changes {
        let d = def(k).ok_or_else(|| SaveError::Invalid { key: k.clone(), reason: Box::new(Refusal::new("unknown_setting", Value::Null, "unknown setting")) })?;
        let norm = canonical(validate(d, v).map_err(|reason| SaveError::Invalid { key: k.clone(), reason: Box::new(reason) })?);
        if d.ty == "secret" {
            let s = norm.as_str().unwrap_or("");
            if s.is_empty() {
                sqlx::query("delete from settings where key = $1").bind(k).execute(&mut *conn).await.map_err(store(k))?;
            } else {
                sqlx::query("insert into settings (key, value, encrypted, updated_by) values ($1, $2, true, $3) on conflict (key) do update set value = $2, encrypted = true, updated_by = $3, updated_at = now()")
                    .bind(k)
                    .bind(json!(keys.encrypt(s)))
                    .bind(actor)
                    .execute(&mut *conn)
                    .await
                    .map_err(store(k))?;
            }
        } else if same_value(&norm, &d.default) {
            sqlx::query("delete from settings where key = $1").bind(k).execute(&mut *conn).await.map_err(store(k))?;
        } else {
            sqlx::query("insert into settings (key, value, encrypted, updated_by) values ($1, $2, false, $3) on conflict (key) do update set value = $2, encrypted = false, updated_by = $3, updated_at = now()")
                .bind(k)
                .bind(&norm)
                .bind(actor)
                .execute(&mut *conn)
                .await
                .map_err(store(k))?;
        }
        changed.push(k.clone());
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_valid_and_defaults_validate() {
        let mut keys = std::collections::HashSet::new();
        for d in registry() {
            assert!(keys.insert(d.key.clone()), "duplicate key {}", d.key);
            if d.ty != "secret" {
                validate(d, &d.default).unwrap_or_else(|e| panic!("default of {} invalid: {e}", d.key));
            }
        }
    }

    fn refuses(key: &str, v: Value) -> Refusal {
        match validate(def(key).unwrap(), &v) {
            Err(e) => e,
            Ok(stored) => panic!("{key} accepted {v} and would store {stored}"),
        }
    }

    fn accepts(key: &str, v: Value) -> Value {
        validate(def(key).unwrap(), &v).unwrap_or_else(|e| panic!("{key} refused {v}: {e}"))
    }

    /// The ratchet: a text setting says what kind of text it is. A new `string` setting without a `format` fails here, so that "any string will do"
    /// is a decision someone made (`single_line` at the least) and not the default.
    #[test]
    fn every_text_setting_declares_a_format() {
        let mut used = std::collections::HashSet::new();
        for d in registry() {
            match SettingDef::takes_format(&d.ty) {
                Some(fits) => {
                    let f = d.format.as_deref().unwrap_or_else(|| panic!("{} is a text setting and declares no format (settings.rs, FORMATS)", d.key));
                    assert!(fits.contains(&f), "{}: the format {f} does not fit the type {}", d.key, d.ty);
                    assert!(FORMATS.contains(&f), "{}: {f} is not a known format", d.key);
                    used.insert(f);
                }
                None => assert!(d.format.is_none(), "{} is not text and cannot have a format", d.key),
            }
        }
        for f in FORMATS {
            assert!(used.contains(f), "no setting uses the format {f}: a format nothing needs is dead code");
        }
        for (key, f) in [
            ("llm.baseUrl", "http_url"),
            ("app.publicUrl", "http_origin"),
            ("ui.helpUrl", "http_link"),
            ("versions.sourceBases", "url_list"),
            ("llm.appReferer", "single_line"),
            ("llm.appTitle", "single_line"),
            ("app.name", "single_line"),
        ] {
            assert_eq!(def(key).unwrap().format.as_deref(), Some(f), "{key}");
        }
    }

    #[test]
    fn http_url_refuses_what_cannot_be_called_and_accepts_what_can() {
        for bad in [
            "file:///etc",
            "javascript:alert(1)",
            "not a url",
            "http://169.254.169.254@evil",
            "https://user:secret@openrouter.ai/api/v1",
            "https://openrouter.ai/api/v1?key=1",
            "https://openrouter.ai/api/v1#top",
            "http:openrouter.ai",
            "ftp://openrouter.ai/",
            "http://open router.ai/",
            "https://openrouter.ai\\@evil.example/",
            "http://",
            "https://openrouter.ai:99999/",
            "//openrouter.ai/api",
            "",
        ] {
            refuses("llm.baseUrl", json!(bad));
        }
        for good in ["https://openrouter.ai/api/v1", "http://127.0.0.1:9", "http://localhost:8080/v1/", "http://[::1]:9/x", "  https://openrouter.ai/api/v1  "] {
            accepts("llm.baseUrl", json!(good));
        }
        assert_eq!(accepts("llm.baseUrl", json!("  https://gateway.internal/v1  ")), json!("https://gateway.internal/v1"), "trimmed, otherwise as written");
        refuses("llm.baseUrl", json!(5));
    }

    #[test]
    fn http_link_takes_a_query_and_a_fragment_but_not_a_scheme_that_runs_code() {
        for bad in ["javascript:alert(document.domain)", "data:text/html,<script>1</script>", "//docs.example.com/guide", "https://docs.example.com/a\nb", "mailto:x@y.z"] {
            refuses("ui.helpUrl", json!(bad));
        }
        assert_eq!(accepts("ui.helpUrl", json!("")), json!(""), "no link is a way to have none");
        accepts("ui.helpUrl", json!("https://docs.example.com/guide?lang=ru#start"));
    }

    /// `csrf_guard` compares the host of the `Origin` header with what follows `://` in this setting: a path in it makes every browser write fail.
    #[test]
    fn http_origin_refuses_a_path_and_stores_the_form_a_browser_sends() {
        for bad in ["https://x.com/app", "https://x.com/app/", "https://x.com/.", "https://x.com?x=1", "https://x.com/#a", "x.com", "https://user@x.com"] {
            refuses("app.publicUrl", json!(bad));
        }
        assert_eq!(accepts("app.publicUrl", json!("")), json!(""), "empty: the Origin must match the Host");
        assert_eq!(accepts("app.publicUrl", json!("https://x.com")), json!("https://x.com"));
        assert_eq!(accepts("app.publicUrl", json!("https://x.com/")), json!("https://x.com"), "the slash is not part of an origin");
        assert_eq!(accepts("app.publicUrl", json!("HTTPS://Architect.Example.COM:443")), json!("https://architect.example.com"), "lower case, no default port: the Origin header has neither");
        assert_eq!(accepts("app.publicUrl", json!("http://localhost:8080")), json!("http://localhost:8080"));
    }

    #[test]
    fn url_list_checks_every_entry_and_makes_each_base_a_directory() {
        let e = refuses("versions.sourceBases", json!(["https://endoflife.date/api/", "ftp://mirror.internal/"]));
        assert!(e.text().contains("ftp://mirror.internal/"), "the entry that is wrong is named: {e}");
        assert_eq!((e.code, e.params["value"].as_str()), ("not_http", Some("ftp://mirror.internal/")), "and it is a parameter too, for the interface to name it in its own words");
        refuses("versions.sourceBases", json!(["https://endoflife.date/api/?x=1"]));
        refuses("versions.sourceBases", json!(["file:///etc/"]));
        refuses("versions.sourceBases", json!([7]));
        assert_eq!(accepts("versions.sourceBases", json!(["http://127.0.0.1:9/api", " https://endoflife.date/api/ ", ""])), json!(["http://127.0.0.1:9/api/", "https://endoflife.date/api/"]));
        assert_eq!(accepts("versions.sourceBases", json!([])), json!([]), "no base: nothing is fetched");
    }

    #[test]
    fn single_line_refuses_line_breaks_and_other_control_characters() {
        for bad in ["a\r\nX-Evil: 1", "a\nb", "a\tb", "x\u{0}y", "a\u{7f}b", &"x".repeat(MAX_LINE_CHARS + 1)] {
            for key in ["llm.appReferer", "llm.appTitle", "app.name"] {
                refuses(key, json!(bad));
            }
        }
        for good in ["", "Infra Architect", "Инфра Архитектор", &"x".repeat(MAX_LINE_CHARS)] {
            for key in ["llm.appReferer", "llm.appTitle", "app.name"] {
                accepts(key, json!(good));
            }
        }
        refuses("llm.apiKey", json!("sk-or-v1-abc\r\nX-Evil: 1"));
        refuses("llm.model.planner", json!("anthropic/claude\nx"));
        refuses("llm.model.fallbacks", json!(["ok/model", "bad\nmodel"]));
        accepts("llm.model.fallbacks", json!(["openai/gpt-5", " anthropic/claude-sonnet-4 "]));
    }

    #[test]
    fn number_bounds_and_enums_are_enforced() {
        let d = def("auth.sessionTtlHours").unwrap();
        assert!(validate(d, &json!(0)).is_err());
        assert!(validate(d, &json!("8")).is_ok());
        let e = def("llm.structuredOutput").unwrap();
        assert!(validate(e, &json!("nope")).is_err());
        assert!(validate(e, &json!("json_object")).is_ok());
    }
}
