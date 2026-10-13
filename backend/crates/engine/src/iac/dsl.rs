//! The template language of IaC generators.
//!
//! A template is a JSON document. Every string may contain `{{ expression }}` placeholders
//! (Jinja expression syntax, evaluated by `minijinja` in strict mode with an instruction budget –
//! no I/O, no loops that can run away). A string that consists of exactly one placeholder keeps
//! the *type* of the result (numbers stay numbers, lists stay lists); anything else is text.
//! Results that are `none` disappear from the output, as do objects/arrays that became empty
//! because all their members disappeared – that is how optional settings are expressed.
//!
//! Structure directives (all start with `$`):
//!
//! * `{"$if": expr, "then": X, "else": Y}`  – one of two values (missing branch = nothing)
//! * `{"$each": expr, "as": "x", "do": X}`   – one value per element; inside arrays the results
//!   are spliced into the parent array. `index` names the loop counter.
//! * `{"$merge": [A, B, …]}`                 – shallow merge of objects, later wins
//! * `{"$raw": X}`                           – X is copied verbatim (no placeholder evaluation)
//! * `{"$when": expr, "key": …}`             – inside an object: drop the whole object if false
//!
//! The output is a plain `serde_json::Value`; YAML/HCL are produced from it by writers, so a
//! template can never emit syntactically broken or injected documents.

use minijinja::{Environment, UndefinedBehavior, Value as MV};
use serde::Deserialize;
use serde_json::{Map, Number, Value};

use crate::config::TemplateLimits;

#[derive(Debug, Clone, PartialEq)]
pub struct DslError {
    /// Location inside the template, e.g. `objects[0].body.spec.storage.size`.
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for DslError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", if self.path.is_empty() { "$" } else { &self.path }, self.message)
    }
}

impl std::error::Error for DslError {}

type R<T> = Result<T, DslError>;

fn err(path: &str, msg: impl Into<String>) -> DslError {
    DslError { path: path.to_string(), message: msg.into() }
}

/// Convert JSON to the template engine's value type.
pub fn to_mv(v: &Value) -> MV {
    MV::deserialize(v.clone()).unwrap_or(MV::UNDEFINED)
}

/// Lower-case DNS-1123 label (what Kubernetes accepts as a name): Cyrillic is transliterated,
/// everything else that is not `[a-z0-9]` becomes `-`, the result is at most 63 characters.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            continue;
        }
        let tr = match c.to_lowercase().next().unwrap_or(c) {
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'д' => "d",
            'е' | 'ё' => "e",
            'ж' => "zh",
            'з' => "z",
            'и' => "i",
            'й' => "y",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ф' => "f",
            'х' => "h",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "sch",
            'ъ' | 'ь' => "",
            'ы' => "y",
            'э' => "e",
            'ю' => "yu",
            'я' => "ya",
            _ => "-",
        };
        out.push_str(tr);
    }
    let mut collapsed = String::new();
    let mut prev_dash = true;
    for c in out.chars() {
        if c == '-' {
            if !prev_dash {
                collapsed.push('-');
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }
    let mut r = collapsed.trim_matches('-').to_string();
    if r.len() > 63 {
        r.truncate(63);
        r = r.trim_end_matches('-').to_string();
    }
    r
}

pub struct Dsl {
    env: Environment<'static>,
    /// Upper bound for the number of iterations of one `$each`.
    max_items: usize,
}

impl Default for Dsl {
    fn default() -> Self {
        Self::new()
    }
}

impl Dsl {
    /// The default limits (`engine.json` → `export.templateLimits` overrides them for real exports).
    pub fn new() -> Self {
        Self::with_limits(&TemplateLimits::default())
    }

    pub fn with_limits(limits: &TemplateLimits) -> Self {
        let mut env = Environment::new();
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_fuel(Some(limits.fuel));
        env.add_filter("slug", |s: String| slug(&s));
        // a template can refuse to generate something that would be wrong
        env.add_function("fail", |msg: String| -> Result<MV, minijinja::Error> { Err(minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, msg)) });
        Dsl { env, max_items: limits.max_items }
    }

    /// Evaluate a bare expression (no braces).
    pub fn eval(&self, expr: &str, ctx: &MV) -> Result<MV, String> {
        let e = self.env.compile_expression(expr).map_err(|e| clean(&e))?;
        e.eval(ctx).map_err(|e| clean(&e))
    }

    /// Evaluate a condition; `none`/`undefined`/empty/zero are false.
    pub fn truthy(&self, expr: &str, ctx: &MV, path: &str) -> R<bool> {
        let v = self.eval(expr, ctx).map_err(|m| err(path, m))?;
        Ok(!v.is_undefined() && v.is_true())
    }

    /// Render a string in *text mode* (always yields a string; `none` is empty).
    pub fn text(&self, s: &str, ctx: &MV, path: &str) -> R<String> {
        match self.string_value(s, ctx, path)? {
            Some(Value::String(t)) => Ok(t),
            Some(other) => Ok(stringify(&other)),
            None => Ok(String::new()),
        }
    }

    /// Render a template value. `None` = the value disappears.
    pub fn render(&self, tpl: &Value, ctx: &MV) -> R<Option<Value>> {
        self.node(tpl, ctx, "")
    }

    fn node(&self, tpl: &Value, ctx: &MV, path: &str) -> R<Option<Value>> {
        match tpl {
            Value::String(s) => self.string_value(s, ctx, path),
            Value::Array(items) => {
                let mut out = Vec::new();
                for (i, it) in items.iter().enumerate() {
                    let p = format!("{path}[{i}]");
                    if let Some(each) = it.as_object().filter(|o| o.contains_key("$each")) {
                        out.extend(self.each(each, ctx, &p)?);
                    } else if let Some(v) = self.node(it, ctx, &p)? {
                        out.push(v);
                    }
                }
                if out.is_empty() && !items.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(Value::Array(out)))
                }
            }
            Value::Object(m) => self.object(m, ctx, path),
            Value::Null => Ok(None),
            other => Ok(Some(other.clone())),
        }
    }

    fn object(&self, m: &Map<String, Value>, ctx: &MV, path: &str) -> R<Option<Value>> {
        if let Some(raw) = m.get("$raw") {
            return Ok(Some(raw.clone()));
        }
        if let Some(cond) = m.get("$if") {
            let c = cond.as_str().ok_or_else(|| err(path, "$if must be an expression string"))?;
            let branch = if self.truthy(c, ctx, &format!("{path}.$if"))? { m.get("then") } else { m.get("else") };
            return match branch {
                Some(b) => self.node(b, ctx, &format!("{path}.{}", if self.truthy(c, ctx, path)? { "then" } else { "else" })),
                None => Ok(None),
            };
        }
        if m.contains_key("$each") {
            let items = self.each(m, ctx, path)?;
            return Ok(Some(Value::Array(items)));
        }
        if let Some(parts) = m.get("$merge") {
            let arr = parts.as_array().ok_or_else(|| err(path, "$merge must be an array of objects"))?;
            let mut out = Map::new();
            for (i, p) in arr.iter().enumerate() {
                match self.node(p, ctx, &format!("{path}.$merge[{i}]"))? {
                    Some(Value::Object(o)) => {
                        for (k, v) in o {
                            out.insert(k, v);
                        }
                    }
                    None => {}
                    Some(_) => return Err(err(&format!("{path}.$merge[{i}]"), "$merge entries must be objects")),
                }
            }
            return Ok(if out.is_empty() { None } else { Some(Value::Object(out)) });
        }
        if let Some(cond) = m.get("$when") {
            let c = cond.as_str().ok_or_else(|| err(path, "$when must be an expression string"))?;
            if !self.truthy(c, ctx, &format!("{path}.$when"))? {
                return Ok(None);
            }
        }
        let mut out = Map::new();
        for (k, v) in m {
            if k == "$when" {
                continue;
            }
            if k.starts_with('$') {
                return Err(err(path, format!("unknown directive '{k}'")));
            }
            let key = if k.contains("{{") { self.text(k, ctx, &format!("{path}.{k}"))? } else { k.clone() };
            if key.is_empty() {
                continue;
            }
            let p = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
            if let Some(val) = self.node(v, ctx, &p)? {
                out.insert(key, val);
            }
        }
        // an object whose members all disappeared disappears with them
        if out.is_empty() && m.keys().any(|k| k != "$when") {
            Ok(None)
        } else {
            Ok(Some(Value::Object(out)))
        }
    }

    fn each(&self, m: &Map<String, Value>, ctx: &MV, path: &str) -> R<Vec<Value>> {
        let src = m.get("$each").and_then(|v| v.as_str()).ok_or_else(|| err(path, "$each must be an expression string"))?;
        let var = m.get("as").and_then(|v| v.as_str()).unwrap_or("it");
        let idx = m.get("index").and_then(|v| v.as_str());
        let body = m.get("do").ok_or_else(|| err(path, "$each needs a 'do' template"))?;
        let list = self.eval(src, ctx).map_err(|e| err(&format!("{path}.$each"), e))?;
        let items: Vec<MV> = match list.try_iter() {
            Ok(it) => it.collect(),
            Err(_) if list.is_undefined() || list.is_none() => vec![],
            Err(_) => return Err(err(&format!("{path}.$each"), "expression is not iterable")),
        };
        if items.len() > self.max_items {
            return Err(err(&format!("{path}.$each"), format!("more than {} items", self.max_items)));
        }
        let mut out = Vec::new();
        for (i, item) in items.into_iter().enumerate() {
            let mut pairs: Vec<(String, MV)> = vec![(var.to_string(), item)];
            if let Some(ix) = idx {
                pairs.push((ix.to_string(), MV::from(i as i64)));
            }
            // loop variables are looked up first, then the enclosing context
            let scoped = minijinja::value::merge_maps([ctx.clone(), MV::from_pairs(pairs)]);
            if let Some(v) = self.node(body, &scoped, &format!("{path}.do"))? {
                match v {
                    Value::Array(a) if body.as_object().map(|o| o.contains_key("$each")).unwrap_or(false) => out.extend(a),
                    other => out.push(other),
                }
            }
        }
        Ok(out)
    }

    /// Evaluate placeholders in one string.
    fn string_value(&self, s: &str, ctx: &MV, path: &str) -> R<Option<Value>> {
        if !s.contains("{{") {
            return Ok(Some(Value::String(s.to_string())));
        }
        let segs = split_segments(s).map_err(|m| err(path, m))?;
        // exactly one placeholder and nothing else: keep the type
        if let [Seg::Expr(e)] = segs.as_slice() {
            let v = self.eval(e, ctx).map_err(|m| err(path, format!("{{{{ {e} }}}}: {m}")))?;
            if v.is_undefined() {
                return Err(err(path, format!("{{{{ {e} }}}} is undefined")));
            }
            return Ok(from_mv(&v));
        }
        let mut out = String::new();
        for seg in segs {
            match seg {
                Seg::Text(t) => out.push_str(&t),
                Seg::Expr(e) => {
                    let v = self.eval(&e, ctx).map_err(|m| err(path, format!("{{{{ {e} }}}}: {m}")))?;
                    if v.is_undefined() {
                        return Err(err(path, format!("{{{{ {e} }}}} is undefined")));
                    }
                    if let Some(j) = from_mv(&v) {
                        out.push_str(&stringify(&j));
                    }
                }
            }
        }
        Ok(Some(Value::String(out)))
    }

    /// Syntax check of one bare expression.
    pub fn check_expr(&self, expr: &str, path: &str, errors: &mut Vec<DslError>) {
        if let Err(m) = self.env.compile_expression(expr) {
            errors.push(err(path, clean(&m)));
        }
    }

    /// Syntax check of a whole template without evaluating it (used when an admin saves one).
    pub fn check(&self, tpl: &Value, path: &str, errors: &mut Vec<DslError>) {
        match tpl {
            Value::String(s) => {
                if s.contains("{{") {
                    match split_segments(s) {
                        Ok(segs) => {
                            for seg in segs {
                                if let Seg::Expr(e) = seg {
                                    if let Err(m) = self.env.compile_expression(&e) {
                                        errors.push(err(path, format!("{{{{ {e} }}}}: {}", clean(&m))));
                                    }
                                }
                            }
                        }
                        Err(m) => errors.push(err(path, m)),
                    }
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    self.check(v, &format!("{path}[{i}]"), errors);
                }
            }
            Value::Object(m) => {
                if m.contains_key("$raw") {
                    return;
                }
                for (k, v) in m {
                    let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    match k.as_str() {
                        "$if" | "$when" | "$each" => match v.as_str() {
                            Some(e) => {
                                if let Err(m) = self.env.compile_expression(e) {
                                    errors.push(err(&p, clean(&m)));
                                }
                            }
                            None => errors.push(err(&p, "must be an expression string")),
                        },
                        "as" | "index" if m.contains_key("$each") => {}
                        "then" | "else" | "do" | "$merge" => self.check(v, &p, errors),
                        other if other.starts_with('$') => errors.push(err(&p, format!("unknown directive '{other}'"))),
                        other => {
                            if other.contains("{{") {
                                self.check(&Value::String(other.to_string()), &format!("{p} (key)"), errors);
                            }
                            self.check(v, &p, errors);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn clean(e: &minijinja::Error) -> String {
    // the first line carries the cause; the rest is a source excerpt
    let s = e.to_string();
    let first = s.lines().next().unwrap_or("").to_string();
    match e.detail() {
        Some(d) if !first.contains(d) => format!("{first}: {d}"),
        _ => first,
    }
}

#[derive(Debug, PartialEq)]
enum Seg {
    Text(String),
    Expr(String),
}

fn split_segments(s: &str) -> Result<Vec<Seg>, String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("{{") {
        if i > 0 {
            out.push(Seg::Text(rest[..i].to_string()));
        }
        let after = &rest[i + 2..];
        let end = find_close(after).ok_or_else(|| "unclosed '{{'".to_string())?;
        let expr = after[..end].trim();
        if expr.is_empty() {
            return Err("empty '{{ }}'".into());
        }
        out.push(Seg::Expr(expr.to_string()));
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        out.push(Seg::Text(rest.to_string()));
    }
    Ok(out)
}

/// Position of the `}}` that closes a placeholder, ignoring braces inside quoted strings.
fn find_close(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) => {
                if c == b'\\' {
                    i += 1;
                } else if c == q {
                    quote = None;
                }
            }
            None => {
                if c == b'\'' || c == b'"' {
                    quote = Some(c);
                } else if c == b'}' && b.get(i + 1) == Some(&b'}') {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// Template-engine value → JSON (`undefined`/`none` → `None`; integral floats → integers).
fn from_mv(v: &MV) -> Option<Value> {
    if v.is_undefined() || v.is_none() {
        return None;
    }
    let j = serde_json::to_value(v).ok()?;
    Some(normalize(j))
}

fn normalize(v: Value) -> Value {
    match v {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if n.is_f64() && f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 {
                    return Value::Number(Number::from(f as i64));
                }
            }
            Value::Number(n)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

fn stringify(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => super::yaml::number(n),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> MV {
        to_mv(&json!({
            "p": {"replicas": 3, "ramGb": 32, "name": "Main DB", "cls": null, "tags": ["a", "b"], "on": true, "sc": "nvme"},
            "zones": [{"id": "a", "n": "Zone A"}, {"id": "b", "n": "Zone B"}],
            "unit": {"multi": true}
        }))
    }

    fn r(tpl: Value) -> Option<Value> {
        Dsl::new().render(&tpl, &ctx()).unwrap()
    }

    #[test]
    fn single_placeholders_keep_their_type_and_mixed_text_is_a_string() {
        assert_eq!(r(json!("{{ p.replicas }}")), Some(json!(3)));
        assert_eq!(r(json!("{{ p.ramGb ~ 'Gi' }}")), Some(json!("32Gi")));
        assert_eq!(r(json!("size-{{ p.ramGb }}Gi")), Some(json!("size-32Gi")));
        assert_eq!(r(json!("{{ p.tags }}")), Some(json!(["a", "b"])));
        assert_eq!(r(json!("{{ p.on }}")), Some(json!(true)));
        assert_eq!(r(json!("{{ p.ramGb / 4 }}")), Some(json!(8)), "integral floats become integers");
        assert_eq!(r(json!("{{ (p.ramGb / 5) }}")), Some(json!(6.4)));
        assert_eq!(r(json!("{{ p.name|slug }}")), Some(json!("main-db")));
    }

    #[test]
    fn none_disappears_with_its_key_and_emptied_containers() {
        assert_eq!(r(json!({"a": "{{ p.cls }}", "b": 1})), Some(json!({"b": 1})));
        assert_eq!(r(json!({"a": {"b": "{{ p.cls }}"}, "c": 1})), Some(json!({"c": 1})));
        assert_eq!(r(json!({"list": ["{{ p.cls }}"], "keep": []})), Some(json!({"keep": []})));
        assert_eq!(r(json!(["x", "{{ p.cls }}", "y"])), Some(json!(["x", "y"])));
    }

    #[test]
    fn directives() {
        assert_eq!(r(json!({"$if": "p.replicas > 2", "then": {"ha": true}, "else": {"ha": false}})), Some(json!({"ha": true})));
        assert_eq!(r(json!({"x": {"$if": "p.replicas > 5", "then": 1}, "y": 2})), Some(json!({"y": 2})));
        assert_eq!(r(json!({"$when": "unit.multi", "k": 1})), Some(json!({"k": 1})));
        assert_eq!(r(json!({"$when": "not unit.multi", "k": 1})), None);
        assert_eq!(r(json!({"$merge": [{"a": 1}, {"$if": "p.on", "then": {"b": 2}}, {"a": 3}]})), Some(json!({"a": 3, "b": 2})));
        assert_eq!(r(json!({"$raw": {"s": "{{ kept }}"}})), Some(json!({"s": "{{ kept }}"})));
        let each = json!({"zones": {"$each": "zones", "as": "z", "do": {"id": "{{ z.id }}", "n": "{{ loop_i }}"}, "index": "loop_i"}});
        assert_eq!(r(each), Some(json!({"zones": [{"id": "a", "n": 0}, {"id": "b", "n": 1}]})));
        // inside arrays the results are spliced
        assert_eq!(r(json!(["first", {"$each": "p.tags", "as": "t", "do": "{{ t }}-x"}, "last"])), Some(json!(["first", "a-x", "b-x", "last"])));
    }

    #[test]
    fn dynamic_keys() {
        assert_eq!(r(json!({"{{ p.sc }}-key": 1})), Some(json!({"nvme-key": 1})));
    }

    #[test]
    fn errors_carry_the_location() {
        let d = Dsl::new();
        let e = d.render(&json!({"spec": {"a": "{{ p.nope }}"}}), &ctx()).unwrap_err();
        assert_eq!(e.path, "spec.a", "{e}");
        assert!(e.message.contains("undefined"), "{e}");
        let e = d.render(&json!({"a": "{{ 1 + }}"}), &ctx()).unwrap_err();
        assert_eq!(e.path, "a");
        let e = d.render(&json!({"a": "{{ fail('no way') }}"}), &ctx()).unwrap_err();
        assert!(e.message.contains("no way"), "{e}");
        let e = d.render(&json!({"$unknown": 1}), &ctx()).unwrap_err();
        assert!(e.message.contains("unknown directive"));
        let e = d.render(&json!({"a": "{{ p.replicas "}), &ctx()).unwrap_err();
        assert!(e.message.contains("unclosed"));
    }

    #[test]
    fn braces_inside_quoted_strings_do_not_close_a_placeholder() {
        assert_eq!(r(json!("{{ '}}' ~ p.replicas }}")), Some(json!("}}3")));
        assert_eq!(r(json!("{{ '{{' }}")), Some(json!("{{")));
    }

    #[test]
    fn static_check_finds_syntax_errors_everywhere() {
        let d = Dsl::new();
        let mut errs = vec![];
        d.check(&json!({"a": {"b": ["{{ ok }}", "{{ 1 + }}"]}, "$if": "x ==", "then": 1, "{{ bad( }}": 2}), "", &mut errs);
        assert!(errs.iter().any(|e| e.path == "a.b[1]"), "{errs:?}");
        assert!(errs.iter().any(|e| e.path == "$if"), "{errs:?}");
        assert!(errs.iter().any(|e| e.path.contains("(key)")), "{errs:?}");
        let mut ok = vec![];
        d.check(&json!({"a": "{{ p.x|default(none) }}", "$when": "true"}), "", &mut ok);
        assert!(ok.is_empty(), "{ok:?}");
    }

    #[test]
    fn slug_makes_valid_dns_labels() {
        assert_eq!(slug("PG Primary (DC1)"), "pg-primary-dc1");
        assert_eq!(slug("Кластер K8s DC1"), "klaster-k8s-dc1");
        assert_eq!(slug("--x__y--"), "x-y");
        assert_eq!(slug(&"a".repeat(100)).len(), 63);
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn runaway_expressions_are_stopped() {
        let d = Dsl::new();
        let big = "{{ range(100000)|list|map('string')|join('') ~ range(100000)|list|map('string')|join('') ~ range(100000)|list|map('string')|join('') }}";
        // either fuel or size limits stop it – it must not hang or panic
        let _ = d.render(&json!({"a": big}), &ctx());
    }
}
