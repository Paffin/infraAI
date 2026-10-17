//! Prompt assembly: placeholders, catalog/architecture/report digests and response schemas.
//! Prompts themselves are configuration (admin-editable); this module only fills them in.

use crate::config_store::Loaded;
use engine::catalog::{Catalog, KindDef, PropDef, PropType};
use engine::evaluate::{render_message, Report, Status};
use engine::model::Architecture;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn render(template: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{}}}}}", k), v);
    }
    out
}

pub fn language_name(code: &str) -> &'static str {
    match code {
        "ru" => "Russian",
        "en" => "English",
        "de" => "German",
        "es" => "Spanish",
        "fr" => "French",
        _ => "the user's language",
    }
}

/// What a request is about: besides the basic library the model gets in full the products the design already holds and the ones the words
/// of the request point at.
#[derive(Default)]
pub struct Focus<'a> {
    /// the words of the request (a question, a wish, the repair of an answer)
    pub text: &'a str,
    /// the kinds of the nodes the design already has
    pub in_design: Vec<&'a str>,
}

/// Words of three letters or digits and more, lower case (a request in any language: the match is made on whatever the words are).
fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).map(String::from).collect()
}

/// What names a product: the parts of its id, the words of its names in both languages and of its function.
fn names_of(k: &KindDef) -> BTreeSet<String> {
    let mut out = words(&k.id.replace('_', " "));
    out.extend(["ru", "en"].iter().flat_map(|l| words(&k.name.get(l))));
    if let Some(f) = &k.function {
        out.extend(words(&f.replace('-', " ")));
    }
    out
}

/// The products the model gets in full, in the order it reads them: those the design holds, the basic library, then up to `extra` that the words of
/// the request name (best match first). A word that names a tenth of the library (cloud, server, managed) names nothing in particular and is not used.
pub fn detailed<'c>(cat: &'c Catalog, focus: &Focus, extra: usize) -> Vec<&'c KindDef> {
    let shown: Vec<&KindDef> = cat.kinds.iter().filter(|k| !k.hidden).collect();
    let mut out: Vec<&KindDef> = vec![];
    let take = |k: &'c KindDef, out: &mut Vec<&'c KindDef>| {
        if !out.iter().any(|x| x.id == k.id) {
            out.push(k);
        }
    };
    for id in &focus.in_design {
        if let Some(k) = shown.iter().find(|k| k.id == *id) {
            take(k, &mut out);
        }
    }
    shown.iter().filter(|k| k.core).for_each(|k| take(k, &mut out));
    if extra == 0 {
        return out;
    }
    let asked = words(focus.text);
    if asked.is_empty() {
        return out;
    }
    let names: Vec<BTreeSet<String>> = shown.iter().map(|k| names_of(k)).collect();
    let mut frequency: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &names {
        for w in n {
            *frequency.entry(w).or_default() += 1;
        }
    }
    // a word names a product when few products carry it ("qdrant"); one a twentieth of the library carries ("cloud", "server", "aws") names none in
    // particular, and counts for nothing alone. The rarer the words a product shares with the request, the better the match.
    let specific = (shown.len() / 40).max(5);
    let mut scored: Vec<(f64, usize)> = names
        .iter()
        .enumerate()
        .filter(|(i, _)| !out.iter().any(|x| x.id == shown[*i].id))
        .filter_map(|(i, n)| {
            let hits: Vec<usize> = n.iter().filter(|w| asked.contains(*w)).map(|w| frequency.get(w.as_str()).copied().unwrap_or(1)).filter(|f| *f <= specific).collect();
            // the whole id ("postgresql", "aws_alb" written as "awsalb") named at once is the surest sign
            let whole = asked.contains(&shown[i].id.replace('_', ""));
            (!hits.is_empty() || whole).then(|| (hits.iter().map(|f| 1.0 / *f as f64).sum::<f64>() + if whole { 2.0 } else { 0.0 }, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    for (_, i) in scored.into_iter().take(extra) {
        take(shown[i], &mut out);
    }
    out
}

fn prop_json(p: &PropDef) -> Value {
    let mut o = json!({"key": p.key, "type": match p.prop_type { PropType::Number => "number", PropType::String => "string", PropType::Boolean => "boolean", PropType::Enum => "enum", PropType::UnitRef => "unit_ref" }});
    if !p.options.is_empty() {
        o["options"] = json!(p.options.iter().map(|x| x.value.clone()).collect::<Vec<_>>());
    }
    if let Some(d) = &p.default {
        o["default"] = d.clone();
    }
    if !p.variants.is_empty() {
        o["variants"] = json!(p.variants);
    }
    o
}

/// Properties no product needs to be told about (a free text, the storage class).
fn skipped(key: &str) -> bool {
    matches!(key, "notes" | "storageClass")
}

/// A product in full: what is its own (sizing defaults, properties, roles, variants); the properties everybody has are said once.
fn kind_json(k: &KindDef, lang: &str, common_keys: &[&str]) -> Value {
    // a kind's own defaults for the properties everybody has (3 instances, 8 vCPU…)
    let sizing: serde_json::Map<String, Value> =
        k.props.iter().filter(|p| common_keys.contains(&p.key.as_str()) && p.default.is_some()).map(|p| (p.key.clone(), p.default.clone().unwrap_or(Value::Null))).collect();
    let own: Vec<Value> = k.props.iter().filter(|p| !skipped(&p.key) && !common_keys.contains(&p.key.as_str())).map(prop_json).collect();
    let mut o = json!({"id": k.id, "name": k.name.get(lang), "behavior": k.behavior, "traits": k.traits.join(",")});
    if let Some(f) = &k.function {
        o["function"] = json!(f);
    }
    if !k.roles.is_empty() {
        o["roles"] = json!(k.roles.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
    }
    if !sizing.is_empty() {
        o["sizing"] = Value::Object(sizing);
    }
    if !own.is_empty() {
        o["props"] = json!(own);
    }
    if !k.variants.is_empty() {
        // "id: Name [default] [traits it adds]": a line per way to deploy the product
        o["variants"] = json!(k
            .variants
            .iter()
            .map(|v| {
                let mut line = format!("{}: {}", v.id, v.name.get(lang));
                if v.default {
                    line.push_str(" [default]");
                }
                if let Some(b) = v.behavior {
                    line.push_str(&format!(" [behavior {}]", serde_json::to_value(b).ok().and_then(|x| x.as_str().map(String::from)).unwrap_or_default()));
                }
                if !v.add_traits.is_empty() {
                    line.push_str(&format!(" [+{}]", v.add_traits.join(",+")));
                }
                line
            })
            .collect::<Vec<_>>());
    }
    o
}

/// The catalog for the model, in two tiers. `index` names every product it may use, by function (the first of a function is the one to take when
/// the user names only the job); `kinds` describes in full the basic library and what the request is about (`detailed`), so that the prompt stays
/// a prompt however many products the library holds. The properties every kind has are said once.
pub fn catalog_digest(cat: &Catalog, lang: &str, focus: &Focus, extra: usize) -> String {
    let common: Vec<Value> = cat.common_props.iter().filter(|p| !skipped(&p.key)).map(prop_json).collect();
    let common_keys: Vec<&str> = cat.common_props.iter().map(|p| p.key.as_str()).collect();
    let kinds: Vec<Value> = detailed(cat, focus, extra).into_iter().map(|k| kind_json(k, lang, &common_keys)).collect();
    // by function, in the order the library declares them; the basic products come first inside a function
    let mut index = serde_json::Map::new();
    let mut listed: BTreeSet<&str> = BTreeSet::new();
    for f in cat.categories.iter().flat_map(|c| c.functions.iter()) {
        let mut ids: Vec<&KindDef> = cat.kinds_of_function(&f.id).into_iter().filter(|k| !k.hidden).collect();
        ids.sort_by_key(|k| !k.core);
        if !ids.is_empty() {
            listed.extend(ids.iter().map(|k| k.id.as_str()));
            index.insert(f.id.clone(), json!(ids.iter().map(|k| k.id.clone()).collect::<Vec<_>>()));
        }
    }
    let rest: Vec<&str> = cat.kinds.iter().filter(|k| !k.hidden && !listed.contains(k.id.as_str())).map(|k| k.id.as_str()).collect();
    if !rest.is_empty() {
        index.insert("other".into(), json!(rest));
    }
    let note = "Every kind has the commonProps; `sizing` is the kind's own default for some of them. A unit_ref property holds the `group` id of another cluster in the architecture (for example the consensus cluster used for leader election). `function` groups interchangeable products; `variants` are ways to deploy one (set props.variant). `index` lists EVERY product you may use, by function: the first of a function is the default when the user names only the job. `kinds` describes in full the basic products and those this request is about; a product that is only in `index` has the common properties and its own ones, which you leave unset (the engine fills in the defaults) unless the user gave a value.";
    serde_json::to_string(&json!({"note": note, "commonProps": common, "index": index, "kinds": kinds})).unwrap_or_default()
}

/// The full description of some products only: what a repair adds for the products the model used and was not told about in full.
pub fn kinds_digest(cat: &Catalog, lang: &str, ids: &[String]) -> String {
    let common_keys: Vec<&str> = cat.common_props.iter().map(|p| p.key.as_str()).collect();
    let kinds: Vec<Value> = ids.iter().filter_map(|id| cat.kind(id)).map(|k| kind_json(k, lang, &common_keys)).collect();
    serde_json::to_string(&kinds).unwrap_or_default()
}

/// The kinds an answer of the model names (`"kind": "…"`), each once, in the order they come: it is read off the text, so that it works on an answer that does not parse.
pub fn kinds_named(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut rest = text;
    while let Some(i) = rest.find("\"kind\"") {
        rest = &rest[i + 6..];
        let after = rest.trim_start().strip_prefix(':').map(str::trim_start);
        if let Some(value) = after.and_then(|a| a.strip_prefix('"')) {
            let id: String = value.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
            if !id.is_empty() && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

pub fn edge_types_digest(cat: &Catalog) -> String {
    let v: Vec<Value> = cat.edge_types.iter().map(|t| json!({"id": t.id, "semantics": t.semantics, "props": t.props.iter().map(|p| p.key.clone()).collect::<Vec<_>>()})).collect();
    serde_json::to_string(&v).unwrap_or_default()
}

pub fn zone_types_digest(cat: &Catalog) -> String {
    let v: Vec<Value> = cat.zone_types.iter().map(|t| json!({"id": t.id, "level": t.level, "props": t.props.iter().map(|p| p.key.clone()).collect::<Vec<_>>()})).collect();
    serde_json::to_string(&v).unwrap_or_default()
}

pub fn templates_digest(cfg: &Loaded, lang: &str) -> String {
    cfg.templates.iter().filter(|t| t.enabled).map(|t| format!("{}: {}", t.id, t.description.as_ref().map(|d| d.get(lang)).unwrap_or_default())).collect::<Vec<_>>().join(" | ")
}

pub fn rules_digest(cfg: &Loaded, lang: &str) -> String {
    cfg.rules.iter().filter(|r| r.enabled).map(|r| format!("{}: {}", r.id, r.title.get(lang))).collect::<Vec<_>>().join("; ")
}

/// The levels of `failureTolerance` the engine knows (`tolerance.levels` of the engine settings): the model may only pick one of them.
pub fn tolerance_ids(cfg: &Loaded) -> Vec<String> {
    cfg.engine.tolerance.levels.iter().map(|l| l.id.clone()).collect()
}

/// A level of `failureTolerance` the model wrote down, as the id of a configured level (the id itself or the name of the level, in any case);
/// `None` when it is none of them.
pub fn normalize_tolerance(cfg: &Loaded, value: &str) -> Option<String> {
    let v = value.trim().to_lowercase();
    cfg.engine.tolerance.levels.iter().find(|l| l.id.to_lowercase() == v || ["ru", "en"].iter().any(|lang| l.name.get(lang).to_lowercase() == v)).map(|l| l.id.clone())
}

/// The requirement fields the model may fill. The levels of `failureTolerance` are the engine's (settings `tolerance.levels`) with their own
/// explanation, so a level an administrator adds is offered to the model too and "one data center" has a level to be said with.
pub fn requirements_schema(cfg: &Loaded, lang: &str) -> String {
    let levels = cfg.engine.tolerance.levels.iter().map(|l| format!("\"{}\" = {}", l.id, l.help.get(lang))).collect::<Vec<_>>().join(" | ");
    format!(
        "rpsPeak (number, requests/s at peak), growthPercent (number), rtoSeconds (number), rpoSeconds (number, 0 = no data loss), availabilitySlo (number, e.g. 99.95), budgetMonthly (number, in the configured currency), failureTolerance (the largest failure the design must survive; one of: {levels}), siteFailures (number, how many sites may be lost at the same time, 1 unless stated), compliance (array of strings), notes (string)"
    )
}

/// Minified architecture for the model: no canvas positions, optional notes.
pub fn arch_digest(arch: &Architecture, send_notes: bool) -> String {
    let mut a = arch.clone();
    for z in a.zones.iter_mut() {
        z.ui = None;
    }
    for n in a.nodes.iter_mut() {
        n.ui = None;
        n.proposed = false;
        if !send_notes {
            n.props.remove("notes");
        }
    }
    serde_json::to_string(&a).unwrap_or_default()
}

pub fn findings_digest(report: &Report, cfg: &Loaded, lang: &str, only_open_bad: bool, limit: usize) -> String {
    let mut lines = vec![];
    for f in &report.findings {
        if f.accepted.is_some() {
            continue;
        }
        if only_open_bad && !matches!(f.status, Status::Critical | Status::Risk) {
            continue;
        }
        let Some(rule) = cfg.rules.iter().find(|r| r.id == f.rule_id) else { continue };
        lines.push(format!(
            "- [{}] {} {} (nodes: {}; fingerprint {})",
            match f.status {
                Status::Critical => "critical",
                Status::Risk => "risk",
                Status::NoData => "no_data",
                _ => "ok",
            },
            f.rule_id,
            render_message(rule, f, lang),
            f.subjects.nodes.join(", "),
            f.id
        ));
        if lines.len() >= limit {
            break;
        }
    }
    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
}

pub fn report_digest(report: &Report, cfg: &Loaded, lang: &str) -> String {
    let scen: Vec<Value> = report
        .scenarios
        .iter()
        .map(|s| {
            json!({"failed": s.failed, "state": s.state, "recovery": s.recovery_kind, "recoverySeconds": s.recovery_seconds, "rpoSeconds": s.rpo_seconds, "capacityRatio": s.capacity_ratio, "splitBrain": s.split_brain, "dataLost": s.data_lost})
        })
        .collect();
    json!({
        "score": report.score.value, "confidence": report.score.coverage, "counts": report.counts,
        "scoring": {"formula": "score = 100 - sum(penalties); a rule's penalty depends on its status", "penaltyCritical": cfg.engine.scoring.critical, "penaltyRisk": cfg.engine.scoring.risk, "penaltyNoData": cfg.engine.scoring.no_data, "noDataPenaltyCap": cfg.engine.scoring.no_data_cap, "breakdown": report.score.breakdown.iter().filter(|b| b.penalty > 0.0).map(|b| json!({"rule": b.rule_id, "status": b.status, "penalty": b.penalty})).collect::<Vec<_>>()},
        "objectives": report.objectives.iter().map(|o| json!({"id": o.id, "status": o.status, "required": o.required, "actual": o.actual, "assumed": o.assumed})).collect::<Vec<_>>(),
        "cost": {"total": report.cost.total, "currency": report.cost.currency, "incomplete": report.cost.incomplete},
        "scenarios": scen,
        "findings": findings_digest(report, cfg, lang, false, cfg.settings.num("llm.digestFindings").max(1.0) as usize),
    })
    .to_string()
}

fn obj_array(item: Value) -> Value {
    json!({"type": "array", "items": item})
}

pub fn generate_schema(cat: &Catalog, tolerance: &[String]) -> Value {
    let kinds: Vec<&str> = cat.kinds.iter().map(|k| k.id.as_str()).collect();
    let edges: Vec<&str> = cat.edge_types.iter().map(|k| k.id.as_str()).collect();
    let zones: Vec<&str> = cat.zone_types.iter().map(|k| k.id.as_str()).collect();
    json!({
        "type": "object",
        "properties": {
            "requirements": {"type": "object", "description": "Requirements inferred from the user's text; numbers only", "properties": {
                "rpsPeak": {"type": "number", "description": "peak requests per second"},
                "growthPercent": {"type": "number", "description": "expected load growth, percent"},
                "rtoSeconds": {"type": "number", "description": "recovery time objective in seconds"},
                "rpoSeconds": {"type": "number", "description": "recovery point objective in seconds; 0 = no committed data may be lost"},
                "availabilitySlo": {"type": "number", "description": "availability target in percent, e.g. 99.95"},
                "budgetMonthly": {"type": "number", "description": "monthly budget as a plain number"},
                "failureTolerance": if tolerance.is_empty() { json!({"type": "string"}) } else { json!({"type": "string", "enum": tolerance, "description": "the largest failure the design must survive"}) },
                "siteFailures": {"type": "number", "description": "how many sites may be lost at the same time"},
                "compliance": {"type": "array", "items": {"type": "string"}},
                "notes": {"type": "string"}
            }},
            "architecture": {
                "type": "object",
                "properties": {
                    "zones": obj_array(json!({"type": "object", "properties": {
                        "id": {"type": "string"}, "name": {"type": "string"}, "type": {"type": "string", "enum": zones},
                        "parentId": {"type": ["string", "null"]}, "role": {"type": ["string", "null"]}, "props": {"type": "object", "description": "zone props such as geo, tier"}
                    }, "required": ["id", "name", "type"]})),
                    "nodes": obj_array(json!({"type": "object", "properties": {
                        "id": {"type": "string"}, "kind": {"type": "string", "enum": kinds}, "name": {"type": "string"},
                        "zoneId": {"type": ["string", "null"]}, "group": {"type": ["string", "null"]}, "role": {"type": ["string", "null"]},
                        "props": {"type": "object", "description": "REQUIRED content: sizing (replicas, vcpu, ramGb, diskGb) and the kind-specific props from the catalog (failover, dcs, connectionPooler, encryptionAtRest, immutable, retentionDays, ...)"}
                    }, "required": ["id", "kind", "name", "props"]})),
                    "edges": obj_array(json!({"type": "object", "properties": {
                        "id": {"type": "string"}, "source": {"type": "string"}, "target": {"type": "string"},
                        "type": {"type": "string", "enum": edges}, "label": {"type": ["string", "null"]}, "props": {"type": "object"}
                    }, "required": ["id", "source", "target", "type"]})),
                },
                "required": ["zones", "nodes", "edges"]
            },
            "rationale": {"type": "string"},
            "assumptions": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["architecture", "rationale"]
    })
}

/// Fields of the patch operations (`engine::ops::Op`). Every field must be declared: constrained
/// decoding only emits declared properties, so an item that lists just `op` yields empty operations.
pub const OP_FIELDS: &[&str] = &["op", "id", "zone", "node", "edge", "set", "cascade"];

pub fn patch_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "rationale": {"type": "string"},
            "ops": {"type": "array", "items": {"type": "object", "properties": {
                "op": {"type": "string", "enum": [
                    "add_zone", "update_zone", "remove_zone", "add_node", "update_node", "remove_node", "add_edge", "update_edge", "remove_edge", "set_requirements"
                ]},
                "id": {"type": "string", "description": "id of the zone/node/edge for update_* and remove_*"},
                "zone": {"type": "object", "description": "the whole zone for add_zone"},
                "node": {"type": "object", "description": "the whole node for add_node (id, kind, name, zoneId, group, role, props)"},
                "edge": {"type": "object", "description": "the whole edge for add_edge (id, source, target, type, label, props)"},
                "set": {"type": "object", "description": "fields to change for update_* and set_requirements: name, kind, zoneId, group, role, type, label, source, target, props.<key>, or a requirement key; null clears"},
                "cascade": {"type": "boolean", "description": "remove_zone: also remove what the zone contains"}
            }, "required": ["op"]}},
            "assumptions": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["rationale", "ops"]
    })
}

/// Normalise what a model produced into something the engine's types accept: keep only known
/// property keys, coerce numeric/boolean strings, drop non-scalar values and numbers outside the range the catalog gives the property (dropped,
/// never clamped: a clamped number is a plausible figure the model did not write, and the property falls back to the catalog's default, which is
/// what "no value" means). Returns a list of human-readable notes about what was dropped.
pub fn sanitize(arch: &mut Architecture, cat: &Catalog, numbers: &engine::config::LimitSettings) -> Vec<String> {
    use engine::model::{as_bool, as_f64, as_string, prop_problem};
    let mut notes = vec![];
    let coerce = |defs: &[&engine::catalog::PropDef], bounds: &dyn Fn(&str) -> (Option<f64>, Option<f64>), props: &mut engine::model::Props, owner: &str, notes: &mut Vec<String>| {
        let keys: Vec<String> = props.keys().cloned().collect();
        for k in keys {
            let v = props.get(&k).cloned().unwrap_or(Value::Null);
            if v.is_object() || v.is_array() {
                props.remove(&k);
                notes.push(format!("{owner}: non-scalar prop '{k}' dropped"));
                continue;
            }
            match defs.iter().find(|d| d.key == k) {
                None => {
                    props.remove(&k);
                    notes.push(format!("{owner}: unknown prop '{k}' dropped"));
                }
                Some(d) => match d.prop_type {
                    PropType::Number => match as_f64(&v) {
                        Some(n) if !v.is_boolean() => match prop_problem(d, bounds(&k), numbers, &v) {
                            Some((_, why)) => {
                                props.remove(&k);
                                notes.push(format!("{owner}: prop '{k}' dropped, {why}"));
                            }
                            // keep integers integral (`3`, not `3.0`)
                            None => {
                                props.insert(k, if n.fract() == 0.0 && n.abs() < 1e15 { json!(n as i64) } else { json!(n) });
                            }
                        },
                        _ => {
                            props.remove(&k);
                            notes.push(format!("{owner}: prop '{k}' is not a number, dropped"));
                        }
                    },
                    PropType::Boolean => match as_bool(&v) {
                        Some(b) => {
                            props.insert(k, json!(b));
                        }
                        None => {
                            props.remove(&k);
                        }
                    },
                    _ => {}
                },
            }
        }
    };
    for z in arch.zones.iter_mut() {
        let defs: Vec<&engine::catalog::PropDef> = cat.zone_type(&z.zone_type).map(|t| t.props.iter().collect()).unwrap_or_default();
        coerce(&defs, &|k| defs.iter().find(|d| d.key == k).map_or((None, None), |d| (d.min, d.max)), &mut z.props, &z.id.clone(), &mut notes);
        z.ui = None;
    }
    for n in arch.nodes.iter_mut() {
        // the kind's own definition of a property wins over the common one, the same as everywhere else (`Catalog::prop_def`)
        let kind = cat.kind(&n.kind);
        let mut defs: Vec<&engine::catalog::PropDef> = kind.map(|k| k.props.iter().collect()).unwrap_or_default();
        defs.extend(cat.common_props.iter().filter(|c| !defs.iter().any(|d| d.key == c.key)).collect::<Vec<_>>());
        // The way a product is deployed is not one of the kind's properties but a choice among its variants (`props.variant`):
        // it is kept when the kind has that variant, whatever the case the model wrote it in.
        let variant = n.props.remove("variant");
        coerce(&defs, &|k| cat.prop_bounds(&n.kind, k), &mut n.props, &n.id.clone(), &mut notes);
        if let Some(wanted) = variant.as_ref().and_then(as_string).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            let known = kind.map(|k| k.variants.as_slice()).unwrap_or_default();
            match known.iter().find(|x| x.id == wanted).or_else(|| known.iter().find(|x| x.id.eq_ignore_ascii_case(&wanted))) {
                Some(x) => {
                    n.props.insert("variant".into(), json!(x.id));
                }
                None => notes.push(format!("{}: '{wanted}' is not a variant of {}, dropped", n.id, n.kind)),
            }
        }
        n.ui = None;
        n.proposed = true;
    }
    for e in arch.edges.iter_mut() {
        let defs: Vec<&engine::catalog::PropDef> = cat.edge_type(&e.edge_type).map(|t| t.props.iter().collect()).unwrap_or_default();
        coerce(&defs, &|k| defs.iter().find(|d| d.key == k).map_or((None, None), |d| (d.min, d.max)), &mut e.props, &e.id.clone(), &mut notes);
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::ops::Op;

    #[test]
    fn patch_schema_declares_every_field_of_every_operation() {
        let samples = [
            json!({"op": "add_zone", "zone": {"id": "z", "name": "Z", "type": "dc", "parentId": null, "role": null, "props": {}}}),
            json!({"op": "update_zone", "id": "z", "set": {"name": "Y"}}),
            json!({"op": "remove_zone", "id": "z", "cascade": true}),
            json!({"op": "add_node", "node": {"id": "n", "kind": "redis", "name": "R", "zoneId": null, "group": null, "role": null, "props": {}}}),
            json!({"op": "update_node", "id": "n", "set": {"props.replicas": 3}}),
            json!({"op": "remove_node", "id": "n"}),
            json!({"op": "add_edge", "edge": {"id": "e", "source": "a", "target": "b", "type": "traffic", "label": null, "props": {}}}),
            json!({"op": "update_edge", "id": "e", "set": {"type": "sync"}}),
            json!({"op": "remove_edge", "id": "e"}),
            json!({"op": "set_requirements", "set": {"rpsPeak": 10}}),
        ];
        let schema = patch_schema();
        let declared = schema["properties"]["ops"]["items"]["properties"].as_object().unwrap();
        let ops_enum: Vec<&str> = schema["properties"]["ops"]["items"]["properties"]["op"]["enum"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        for s in samples {
            let op: Op = serde_json::from_value(s.clone()).unwrap_or_else(|e| panic!("sample {s} is not a valid op: {e}"));
            let back = serde_json::to_value(&op).unwrap();
            for key in back.as_object().unwrap().keys() {
                assert!(declared.contains_key(key), "operation field '{key}' is missing in the patch schema");
                assert!(OP_FIELDS.contains(&key.as_str()), "'{key}' missing in OP_FIELDS");
            }
            assert!(ops_enum.contains(&back["op"].as_str().unwrap()), "op {} missing in the enum", back["op"]);
        }
    }
}

#[cfg(test)]
mod sanitize_tests {
    use super::*;

    fn arch(nodes: Value) -> Architecture {
        Architecture { nodes: serde_json::from_value(nodes).unwrap(), ..Default::default() }
    }

    #[test]
    fn the_variant_a_model_names_survives_when_the_kind_has_it_and_is_noted_when_it_does_not() {
        let cat = engine::defaults::catalog();
        let mut a = arch(json!([
            {"id": "db", "kind": "postgresql", "name": "DB", "props": {"variant": "rds", "replicas": 2}},
            {"id": "db2", "kind": "postgresql", "name": "DB", "props": {"variant": " RDS "}},
            {"id": "ghost", "kind": "postgresql", "name": "DB", "props": {"variant": "exadata"}},
            {"id": "cache", "kind": "redis", "name": "C", "props": {"variant": ""}},
            {"id": "lb", "kind": "l7", "name": "LB", "props": {"variant": 7}}
        ]));
        let notes = sanitize(&mut a, &cat, &engine::config::LimitSettings::default());
        let variant = |id: &str| a.node(id).unwrap().props.get("variant").cloned();
        assert_eq!(variant("db"), Some(json!("rds")));
        assert_eq!(variant("db2"), Some(json!("rds")), "case and padding are forgiven, the id kept is the catalog's");
        assert_eq!(variant("ghost"), None, "a variant the kind does not have is not invented for it");
        assert_eq!(variant("cache"), None);
        assert_eq!(variant("lb"), None);
        assert_eq!(a.node("db").unwrap().props["replicas"], json!(2), "the other properties are handled as before");
        assert!(notes.iter().any(|n| n.starts_with("ghost:") && n.contains("exadata")), "{notes:?}");
        assert!(!notes.iter().any(|n| n.contains("unknown prop 'variant'")), "{notes:?}");
    }
}

#[cfg(test)]
mod catalog_digest_tests {
    use super::*;

    fn parsed(cat: &Catalog, focus: &Focus, extra: usize) -> (usize, Value) {
        let d = catalog_digest(cat, "en", focus, extra);
        (d.len(), serde_json::from_str(&d).unwrap())
    }

    fn ids_of(v: &Value) -> Vec<String> {
        v["kinds"].as_array().unwrap().iter().map(|k| k["id"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_digest_of_the_whole_catalog_stays_a_prompt_and_names_every_product_and_variant() {
        let cat = engine::defaults::catalog();
        let shown = cat.kinds.iter().filter(|k| !k.hidden).count();
        assert!(shown >= 500, "the library is meant to be broad: {shown} products");
        let (len, v) = parsed(&cat, &Focus::default(), 0);
        // the index has every product, once, grouped by function
        let index = v["index"].as_object().unwrap();
        let listed: Vec<&str> = index.values().flat_map(|x| x.as_array().unwrap()).map(|x| x.as_str().unwrap()).collect();
        assert_eq!(listed.len(), shown);
        assert_eq!(listed.iter().collect::<BTreeSet<_>>().len(), shown, "a product is listed once");
        // the first of a function is a basic product when the function has one: it is the one taken when the user names only the job
        for (f, ids) in index {
            let first = cat.kind(ids[0].as_str().unwrap()).unwrap();
            if cat.kinds_of_function(f).iter().any(|k| k.core) {
                assert!(first.core, "the first product of {f} is {}, which is not a basic one", first.id);
            }
        }
        // the basic library is described in full, with its variants
        let core = cat.kinds.iter().filter(|k| !k.hidden && k.core).count();
        let kinds = v["kinds"].as_array().unwrap();
        assert_eq!(kinds.len(), core);
        let pg = kinds.iter().find(|k| k["id"] == "postgresql").unwrap();
        assert_eq!(pg["function"], "rdbms");
        assert!(pg["variants"].as_array().unwrap().iter().any(|x| x.as_str().map(|s| s.starts_with("rds: ") && s.contains("[+managed]")).unwrap_or(false)), "{:?}", pg["variants"]);
        assert!(v["commonProps"].as_array().unwrap().iter().any(|p| p["key"] == "replicas"));
        // sizing defaults of a kind are kept, the common properties are not repeated per kind
        assert!(pg["sizing"]["vcpu"].is_number());
        assert!(pg.get("props").and_then(|p| p.as_array()).map(|p| !p.iter().any(|x| x["key"] == "vcpu")).unwrap_or(true));
        // and the whole of it stays a prompt: it is sent with every AI request, so it must not grow with the library
        assert!(len < 80_000, "the catalog digest is {len} bytes");
    }

    #[test]
    fn what_the_request_is_about_is_described_in_full_and_the_rest_is_not() {
        let cat = engine::defaults::catalog();
        let core = cat.kinds.iter().filter(|k| !k.hidden && k.core).count();
        let none = ids_of(&parsed(&cat, &Focus::default(), 40).1);
        assert_eq!(none.len(), core, "a request that names nothing gets the basic library");

        // the words of the request name the products: by id, by name, in any case and with punctuation around
        let focus = Focus { text: "Нужен Qdrant для поиска по эмбеддингам, перед ним Amazon-ALB; а кэш — Garnet.", in_design: vec![] };
        let ids = ids_of(&parsed(&cat, &focus, 40).1);
        for want in ["qdrant", "aws_alb", "garnet"] {
            if cat.kind(want).is_some() {
                assert!(ids.contains(&want.to_string()), "{want} is named by the request: {ids:?}");
            }
        }
        assert!(ids.len() <= core + 40);
        // the products the design already has are always there, whatever the budget is
        let design = Focus { text: "", in_design: vec!["tomcat", "no_such_kind"] };
        assert!(ids_of(&parsed(&cat, &design, 0).1).contains(&"tomcat".to_string()));
        // a word that a twentieth of the library carries names nothing in particular
        let generic = Focus { text: "cloud server data network", in_design: vec![] };
        assert_eq!(ids_of(&parsed(&cat, &generic, 40).1).len(), core, "words that name a twentieth of the library name no product");
        // the same request gives the same digest
        assert_eq!(catalog_digest(&cat, "en", &focus, 40), catalog_digest(&cat, "en", &focus, 40));
        // and a hidden product is never offered
        let mut hidden = cat.clone();
        if let Some(k) = hidden.kinds.iter_mut().find(|k| k.id == "qdrant") {
            k.hidden = true;
        }
        assert!(!ids_of(&parsed(&hidden, &focus, 40).1).contains(&"qdrant".to_string()));
    }

    #[test]
    fn the_kinds_an_answer_names_are_read_off_its_text_even_when_it_does_not_parse() {
        let answer = r#"{"architecture": {"nodes": [{"id": "a", "kind": "postgresql", "name": "DB"}, {"id":"b","kind" : "aws_alb"}, {"id": "c", "kind":"postgresql"}, {"kind": 5}, {"id": "d", "kind": "kafka", "props": {"#;
        assert_eq!(kinds_named(answer), vec!["postgresql", "aws_alb", "kafka"]);
        assert!(kinds_named("no kinds here").is_empty());
        let cat = engine::defaults::catalog();
        let d: Value = serde_json::from_str(&kinds_digest(&cat, "en", &["aws_alb".into(), "no_such_kind".into(), "kafka".into()])).unwrap();
        assert_eq!(d.as_array().unwrap().iter().map(|k| k["id"].as_str().unwrap()).collect::<Vec<_>>(), vec!["aws_alb", "kafka"]);
    }
}
