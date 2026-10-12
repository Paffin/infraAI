//! Prompt assembly: placeholders, catalog/architecture/report digests and response schemas.
//! Prompts themselves are configuration (admin-editable); this module only fills them in.

use crate::config_store::Loaded;
use engine::catalog::{Catalog, PropType};
use engine::evaluate::{render_message, Report, Status};
use engine::model::Architecture;
use serde_json::{json, Value};
use std::collections::BTreeMap;

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

pub fn catalog_digest(cat: &Catalog, lang: &str) -> String {
    let kinds: Vec<Value> = cat
        .kinds
        .iter()
        .filter(|k| !k.hidden)
        .map(|k| {
            let props: Vec<Value> = cat
                .common_props
                .iter()
                .chain(k.props.iter())
                .filter(|p| p.key != "notes" && p.key != "storageClass")
                .map(|p| {
                    let mut o = json!({"key": p.key, "type": match p.prop_type { PropType::Number => "number", PropType::String => "string", PropType::Boolean => "boolean", PropType::Enum => "enum", PropType::UnitRef => "unit_ref" }});
                    if !p.options.is_empty() {
                        o["options"] = json!(p.options.iter().map(|x| x.value.clone()).collect::<Vec<_>>());
                    }
                    if let Some(d) = &p.default {
                        o["default"] = d.clone();
                    }
                    if p.prop_type == PropType::UnitRef {
                        o["refersTo"] = json!("the `group` id of another cluster in the architecture (for example the consensus cluster used for leader election)");
                    }
                    o
                })
                .collect();
            json!({
                "id": k.id, "name": k.name.get(lang), "category": k.category, "behavior": k.behavior,
                "traits": k.traits, "roles": k.roles.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), "props": props,
            })
        })
        .collect();
    serde_json::to_string(&kinds).unwrap_or_default()
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

pub fn requirements_schema() -> &'static str {
    "rpsPeak (number, requests/s at peak), growthPercent (number), rtoSeconds (number), rpoSeconds (number, 0 = no data loss), availabilitySlo (number, e.g. 99.95), budgetMonthly (number, in the configured currency), failureTolerance (\"instance\" | \"site\" | \"geo\"), compliance (array of strings), notes (string)"
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
        "objectives": report.objectives.iter().map(|o| json!({"id": o.id, "status": o.status, "required": o.required, "actual": o.actual, "assumed": o.assumed})).collect::<Vec<_>>(),
        "cost": {"total": report.cost.total, "currency": report.cost.currency, "incomplete": report.cost.incomplete},
        "scenarios": scen,
        "findings": findings_digest(report, cfg, lang, false, 40),
    })
    .to_string()
}

fn obj_array(item: Value) -> Value {
    json!({"type": "array", "items": item})
}

pub fn generate_schema(cat: &Catalog) -> Value {
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
                "failureTolerance": {"type": "string", "enum": ["instance", "site", "geo"]},
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

pub fn patch_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "rationale": {"type": "string"},
            "ops": {"type": "array", "items": {"type": "object", "properties": {"op": {"type": "string", "enum": [
                "add_zone", "update_zone", "remove_zone", "add_node", "update_node", "remove_node", "add_edge", "update_edge", "remove_edge", "set_requirements"
            ]}}, "required": ["op"], "additionalProperties": true}},
            "assumptions": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["rationale", "ops"]
    })
}

/// Normalise what a model produced into something the engine's types accept: keep only known
/// property keys, coerce numeric/boolean strings, drop non-scalar values. Returns a list of
/// human-readable notes about what was dropped.
pub fn sanitize(arch: &mut Architecture, cat: &Catalog) -> Vec<String> {
    use engine::model::{as_bool, as_f64};
    let mut notes = vec![];
    let coerce = |defs: &[&engine::catalog::PropDef], props: &mut engine::model::Props, owner: &str, notes: &mut Vec<String>| {
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
                        Some(n) if !v.is_boolean() => {
                            // keep integers integral (`3`, not `3.0`)
                            props.insert(k, if n.fract() == 0.0 && n.abs() < 1e15 { json!(n as i64) } else { json!(n) });
                        }
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
        coerce(&defs, &mut z.props, &z.id.clone(), &mut notes);
        z.ui = None;
    }
    for n in arch.nodes.iter_mut() {
        let mut defs: Vec<&engine::catalog::PropDef> = cat.common_props.iter().collect();
        if let Some(k) = cat.kind(&n.kind) {
            defs.extend(k.props.iter());
        }
        coerce(&defs, &mut n.props, &n.id.clone(), &mut notes);
        n.ui = None;
        n.proposed = true;
    }
    for e in arch.edges.iter_mut() {
        let defs: Vec<&engine::catalog::PropDef> = cat.edge_type(&e.edge_type).map(|t| t.props.iter().collect()).unwrap_or_default();
        coerce(&defs, &mut e.props, &e.id.clone(), &mut notes);
    }
    notes
}
