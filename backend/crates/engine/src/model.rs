//! The Infrastructure Digital Twin: the single source of truth.
//!
//! Canvas, AI chat, checks, simulation, documentation and IaC export are all *views* of this
//! one typed graph. Every change – manual or proposed by an LLM – goes through the same
//! validation (`validate`) and the same rule engine.

use crate::catalog::{PropDef, PropType};
use crate::config::LimitSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub type Props = BTreeMap<String, Value>;

pub const SCHEMA_VERSION: u32 = 1;

fn one() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Architecture {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default)]
    pub meta: Meta,
    #[serde(default)]
    pub requirements: Requirements,
    #[serde(default)]
    pub zones: Vec<Zone>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Business / engineering requirements the architecture is verified against.
/// Every field is optional: a rule that needs a missing requirement reports `no_data`
/// instead of silently assuming something.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Requirements {
    /// Peak requests per second the entry point must sustain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rps_peak: Option<f64>,
    /// Expected load growth the capacity must cover, percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub growth_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rto_seconds: Option<f64>,
    /// 0 means "no committed transaction may be lost".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpo_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_slo: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_monthly: Option<f64>,
    /// Largest failure the service must survive: a level of `tolerance.levels` of the engine
    /// settings (`instance`, `rack`, `site` (default), `geo`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_tolerance: Option<String>,
    /// How many sites may be lost **at the same time** (1 = the default, one site). With five or more
    /// sites a design is normally meant to survive two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_failures: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compliance: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ui {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

/// Failure-domain hierarchy: region ⊃ DC / AZ / cloud ⊃ rack. What a zone *means* for failure
/// and partition analysis is defined by its zone type (`level`) in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub zone_type: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    /// `active` | `standby` | `witness` | free text; informational + used by some rules.
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub props: Props,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<Ui>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    /// Catalog kind id.
    pub kind: String,
    pub name: String,
    /// `None` = global / external (users, anycast DNS): not subject to zone failures.
    #[serde(default)]
    pub zone_id: Option<String>,
    /// Nodes sharing a `group` form one logical service (cluster) – members are alternatives.
    #[serde(default)]
    pub group: Option<String>,
    /// Role inside the group (`primary`, `replica`, `witness`, …) – kind-specific.
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub props: Props,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<Ui>,
    /// Set on nodes created by an unapplied AI proposal preview.
    #[serde(default, skip_serializing_if = "is_false")]
    pub proposed: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    /// Catalog edge type id (`traffic`, `sync`, `async`, `quorum`, `backup`, `control`, …).
    #[serde(rename = "type")]
    pub edge_type: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub props: Props,
}

// ---------------------------------------------------------------------------------------------
// Props helpers (lenient: LLMs and forms may send numbers as strings)
// ---------------------------------------------------------------------------------------------

pub fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        // "inf" and "NaN" parse as numbers in Rust and are not numbers anywhere in an architecture
        Value::String(s) => s.trim().replace(',', ".").parse::<f64>().ok().filter(|f| f.is_finite()),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

pub fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" | "yes" | "да" | "1" | "on" => Some(true),
            "false" | "no" | "нет" | "0" | "off" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_f64().map(|f| f != 0.0),
        _ => None,
    }
}

pub fn as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub trait PropsExt {
    fn num(&self, key: &str) -> Option<f64>;
    fn flag(&self, key: &str) -> Option<bool>;
    fn text(&self, key: &str) -> Option<String>;
    fn has(&self, key: &str) -> bool;
}

impl PropsExt for Props {
    fn num(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(as_f64)
    }
    fn flag(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(as_bool)
    }
    fn text(&self, key: &str) -> Option<String> {
        self.get(key).and_then(as_string).filter(|s| !s.is_empty())
    }
    fn has(&self, key: &str) -> bool {
        !matches!(self.get(key), None | Some(Value::Null))
    }
}

// ---------------------------------------------------------------------------------------------
// Structural validation
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IssueLevel {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub level: IssueLevel,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

impl Issue {
    fn err(code: &str, subject: Option<&str>, message: String) -> Self {
        Issue { level: IssueLevel::Error, code: code.into(), message, subject: subject.map(|s| s.to_string()) }
    }
    fn warn(code: &str, subject: Option<&str>, message: String) -> Self {
        Issue { level: IssueLevel::Warning, code: code.into(), message, subject: subject.map(|s| s.to_string()) }
    }
}

pub struct ValidationLimits {
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_zones: usize,
    /// Ceilings of the numbers (`limits` of the engine settings).
    pub numbers: LimitSettings,
    /// A number outside its range is an error (the default: whatever is saved, imported or applied must be in range) or, when a design
    /// that is already stored is only being looked at, a warning: an old project still opens, and says what is wrong with it.
    pub lenient_ranges: bool,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self { max_nodes: 500, max_edges: 2000, max_zones: 100, numbers: LimitSettings::default(), lenient_ranges: false }
    }
}

impl ValidationLimits {
    /// The same limits for a design that is only looked at: a number out of range is reported as a warning, not an error.
    pub fn lenient(mut self) -> Self {
        self.lenient_ranges = true;
        self
    }
    /// The same limits with the numbers' ceilings of `numbers`.
    pub fn with_numbers(mut self, numbers: LimitSettings) -> Self {
        self.numbers = numbers;
        self
    }
}

/// A number the way a person reads it: `100000`, not `100000.0`, and not `1e12` for a million million.
fn show_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// A value for a message: short, whatever it is.
fn show_value(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 40 {
        format!("{}…", s.chars().take(40).collect::<String>())
    } else {
        s
    }
}

/// What is wrong with `v` as the value of the prop `def`, if anything: its code (`prop_type`, `prop_out_of_range`) and what to tell. `bounds` is the
/// range the catalog gives the prop (see `Catalog::prop_bounds`), narrowed by the ceilings of `numbers`.
/// A prop that is not set (`null`, or an empty string, which the editor never sends but a form may) has no value to be wrong.
pub fn prop_problem(def: &PropDef, bounds: (Option<f64>, Option<f64>), numbers: &LimitSettings, v: &Value) -> Option<(&'static str, String)> {
    let (min, max) = bounds;
    let ceiling = ceiling_of(&def.key, numbers);
    match def.prop_type {
        PropType::Number => {
            if matches!(v, Value::Null) || matches!(v, Value::String(s) if s.trim().is_empty()) {
                return None;
            }
            // numbers sent as text are numbers (forms and models do it); a flag, a list or a word is not
            let n = if matches!(v, Value::Bool(_)) { None } else { as_f64(v) };
            let Some(n) = n else {
                return Some(("prop_type", format!("{} is not a number", show_value(v))));
            };
            let hi = max.map_or(ceiling, |m| m.min(ceiling));
            let lo = min.unwrap_or(-ceiling);
            if n < lo || n > hi {
                let range = match min {
                    Some(lo) => format!("{} to {}", show_num(lo), show_num(hi)),
                    None => format!("up to {}", show_num(hi)),
                };
                return Some(("prop_out_of_range", format!("{} is out of range (allowed: {})", show_num(n), range)));
            }
            None
        }
        PropType::Boolean => {
            if matches!(v, Value::Null) || as_bool(v).is_some() {
                None
            } else {
                Some(("prop_type", format!("{} is not true or false", show_value(v))))
            }
        }
        _ => None,
    }
}

/// The largest the prop `key` may be for anything: the instance counts have a ceiling of their own.
fn ceiling_of(key: &str, numbers: &LimitSettings) -> f64 {
    match key {
        "replicas" | "failoverReplicas" => numbers.max_replicas as f64,
        _ => numbers.max_number,
    }
}

/// Check every prop of an entity that the catalog describes. Props the catalog does not describe are left alone (a model may carry notes of its
/// own, and a product may be older than a prop). `bounds` says the range of a key: the catalog's, field by field.
fn check_props(out: &mut Vec<Issue>, what: &str, id: &str, props: &Props, defs: &[&PropDef], bounds: &dyn Fn(&str) -> (Option<f64>, Option<f64>), limits: &ValidationLimits) {
    for (key, v) in props {
        let Some(def) = defs.iter().find(|d| &d.key == key) else { continue };
        if let Some((code, why)) = prop_problem(def, bounds(key), &limits.numbers, v) {
            let msg = format!("{what} '{id}' prop '{key}': {why}");
            if code == "prop_out_of_range" && limits.lenient_ranges {
                out.push(Issue::warn(code, Some(id), msg));
            } else {
                out.push(Issue::err(code, Some(id), msg));
            }
        }
    }
}

/// The numeric targets of the requirements. The domains are mathematical (an availability above 0 and at most 100 percent, a time or a sum that is
/// not negative); only the ceilings are settings.
fn check_requirements(out: &mut Vec<Issue>, r: &Requirements, limits: &ValidationLimits) {
    let n = &limits.numbers;
    // (field, value, lowest allowed, whether the lowest itself is allowed, highest allowed)
    let fields: [(&str, Option<f64>, f64, bool, f64); 6] = [
        ("availabilitySlo", r.availability_slo, 0.0, false, 100.0),
        ("rtoSeconds", r.rto_seconds, 0.0, true, n.max_number),
        ("rpoSeconds", r.rpo_seconds, 0.0, true, n.max_number),
        ("budgetMonthly", r.budget_monthly, 0.0, true, n.max_number),
        ("rpsPeak", r.rps_peak, 0.0, true, n.max_rps_peak),
        ("growthPercent", r.growth_percent, -100.0, false, n.max_growth_percent),
    ];
    for (field, value, lo, lo_ok, hi) in fields {
        let Some(v) = value else { continue };
        let ok = v.is_finite() && (v > lo || (lo_ok && v == lo)) && v <= hi;
        if !ok {
            let allowed = format!("{}{} to {}", if lo_ok { "" } else { "above " }, show_num(lo), show_num(hi));
            let msg = format!("requirement '{field}': {} is out of range (allowed: {allowed})", if v.is_finite() { show_num(v) } else { "a number that is not finite".to_string() });
            let subject = format!("requirements.{field}");
            if limits.lenient_ranges {
                out.push(Issue::warn("requirement_out_of_range", Some(&subject), msg));
            } else {
                out.push(Issue::err("requirement_out_of_range", Some(&subject), msg));
            }
        }
    }
}

/// Validate referential integrity and catalog conformity. Used for imports, manual saves and –
/// crucially – for everything an LLM produces (errors are fed back to the model for repair).
pub fn validate(arch: &Architecture, cat: &crate::catalog::Catalog, limits: &ValidationLimits) -> Vec<Issue> {
    let mut out = Vec::new();

    if arch.nodes.len() > limits.max_nodes {
        out.push(Issue::err("limit_nodes", None, format!("too many nodes: {} > {}", arch.nodes.len(), limits.max_nodes)));
    }
    if arch.edges.len() > limits.max_edges {
        out.push(Issue::err("limit_edges", None, format!("too many edges: {} > {}", arch.edges.len(), limits.max_edges)));
    }
    if arch.zones.len() > limits.max_zones {
        out.push(Issue::err("limit_zones", None, format!("too many zones: {} > {}", arch.zones.len(), limits.max_zones)));
    }

    let mut ids: HashSet<&str> = HashSet::new();
    let mut zone_ids: HashMap<&str, &Zone> = HashMap::new();
    for z in &arch.zones {
        if !valid_id(&z.id) {
            out.push(Issue::err("bad_id", Some(&z.id), format!("invalid zone id '{}'", z.id)));
        }
        if !ids.insert(&z.id) {
            out.push(Issue::err("duplicate_id", Some(&z.id), format!("duplicate id '{}'", z.id)));
        }
        zone_ids.insert(&z.id, z);
        match cat.zone_type(&z.zone_type) {
            None => out.push(Issue::err("unknown_zone_type", Some(&z.id), format!("zone '{}' has unknown type '{}'", z.id, z.zone_type))),
            Some(t) => {
                let defs: Vec<&PropDef> = t.props.iter().collect();
                check_props(&mut out, "zone", &z.id, &z.props, &defs, &|k| defs.iter().find(|d| d.key == k).map_or((None, None), |d| (d.min, d.max)), limits);
            }
        }
    }
    for z in &arch.zones {
        if let Some(p) = &z.parent_id {
            if !zone_ids.contains_key(p.as_str()) {
                out.push(Issue::err("dangling_parent", Some(&z.id), format!("zone '{}' parent '{}' does not exist", z.id, p)));
            } else if p == &z.id {
                out.push(Issue::err("zone_cycle", Some(&z.id), format!("zone '{}' is its own parent", z.id)));
            }
        }
    }
    // zone cycles
    for z in &arch.zones {
        let mut seen = HashSet::new();
        let mut cur = Some(z.id.as_str());
        while let Some(c) = cur {
            if !seen.insert(c) {
                out.push(Issue::err("zone_cycle", Some(&z.id), format!("zone hierarchy cycle through '{}'", c)));
                break;
            }
            cur = zone_ids.get(c).and_then(|zz| zz.parent_id.as_deref());
        }
    }

    let mut node_ids: HashSet<&str> = HashSet::new();
    for n in &arch.nodes {
        if !valid_id(&n.id) {
            out.push(Issue::err("bad_id", Some(&n.id), format!("invalid node id '{}'", n.id)));
        }
        if !ids.insert(&n.id) {
            out.push(Issue::err("duplicate_id", Some(&n.id), format!("duplicate id '{}'", n.id)));
        }
        node_ids.insert(&n.id);
        if let Some(v) = n.props.get("variant").and_then(as_string).filter(|v| !v.is_empty()) {
            if let Some(k) = cat.kind(&n.kind) {
                if !k.variants.iter().any(|x| x.id == v) {
                    out.push(Issue::warn(
                        "unknown_variant",
                        Some(&n.id),
                        format!("node '{}' variant '{}' is not defined for kind '{}' (known: {})", n.id, v, n.kind, k.variants.iter().map(|x| x.id.as_str()).collect::<Vec<_>>().join(", ")),
                    ));
                }
            }
        }
        match cat.kind(&n.kind) {
            None => out.push(Issue::err(
                "unknown_kind",
                Some(&n.id),
                format!("node '{}' has unknown kind '{}' (allowed: {})", n.id, n.kind, cat.kinds.iter().map(|k| k.id.as_str()).collect::<Vec<_>>().join(", ")),
            )),
            Some(k) => {
                if let Some(role) = &n.role {
                    let variant = cat.variant(&n.kind, n.props.get("variant").and_then(as_string).as_deref());
                    let roles = cat.roles_of(k, variant);
                    if !roles.is_empty() && !roles.iter().any(|r| &r.id == role) {
                        out.push(Issue::warn("unknown_role", Some(&n.id), format!("node '{}' role '{}' is not defined for kind '{}'", n.id, role, n.kind)));
                    }
                }
            }
        }
        if let Some(z) = &n.zone_id {
            if !zone_ids.contains_key(z.as_str()) {
                out.push(Issue::err("dangling_zone", Some(&n.id), format!("node '{}' zone '{}' does not exist", n.id, z)));
            }
        }
        if let Some(k) = cat.kind(&n.kind) {
            // the kind's own definition of a prop wins, the common one fills in what it leaves out
            let defs: Vec<&PropDef> = k.props.iter().chain(cat.common_props.iter().filter(|c| !k.props.iter().any(|p| p.key == c.key))).collect();
            check_props(&mut out, "node", &n.id, &n.props, &defs, &|key| cat.prop_bounds(&n.kind, key), limits);
        }
    }

    // references to other units (e.g. the DCS of a database) must resolve
    let unit_keys: HashSet<&str> = arch.nodes.iter().map(|n| n.group.as_deref().filter(|g| !g.is_empty()).unwrap_or(n.id.as_str())).collect();
    for n in &arch.nodes {
        for def in cat.kind(&n.kind).map(|k| k.props.iter()).into_iter().flatten() {
            if def.prop_type == crate::catalog::PropType::UnitRef {
                if let Some(v) = n.props.get(&def.key).and_then(as_string).filter(|s| !s.is_empty()) {
                    if !unit_keys.contains(v.as_str()) {
                        let mut keys: Vec<&str> = unit_keys.iter().copied().collect();
                        keys.sort();
                        out.push(Issue::err(
                            "unknown_unit_ref",
                            Some(&n.id),
                            format!("node '{}' prop '{}' references unknown group '{}' (use the `group` id of a cluster; existing: {})", n.id, def.key, v, keys.join(", ")),
                        ));
                    }
                }
            }
        }
    }

    for e in &arch.edges {
        if !valid_id(&e.id) {
            out.push(Issue::err("bad_id", Some(&e.id), format!("invalid edge id '{}'", e.id)));
        }
        if !ids.insert(&e.id) {
            out.push(Issue::err("duplicate_id", Some(&e.id), format!("duplicate id '{}'", e.id)));
        }
        if !node_ids.contains(e.source.as_str()) {
            out.push(Issue::err("dangling_edge", Some(&e.id), format!("edge '{}' source '{}' does not exist", e.id, e.source)));
        }
        if !node_ids.contains(e.target.as_str()) {
            out.push(Issue::err("dangling_edge", Some(&e.id), format!("edge '{}' target '{}' does not exist", e.id, e.target)));
        }
        if e.source == e.target {
            out.push(Issue::err("self_loop", Some(&e.id), format!("edge '{}' connects a node to itself", e.id)));
        }
        match cat.edge_type(&e.edge_type) {
            None => out.push(Issue::err(
                "unknown_edge_type",
                Some(&e.id),
                format!("edge '{}' has unknown type '{}' (allowed: {})", e.id, e.edge_type, cat.edge_types.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")),
            )),
            Some(t) => {
                let defs: Vec<&PropDef> = t.props.iter().collect();
                check_props(&mut out, "edge", &e.id, &e.props, &defs, &|k| defs.iter().find(|d| d.key == k).map_or((None, None), |d| (d.min, d.max)), limits);
            }
        }
    }
    check_requirements(&mut out, &arch.requirements, limits);
    out
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.level == IssueLevel::Error)
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

/// Make an id unique within `taken` by appending `-2`, `-3`, …
pub fn unique_id(base: &str, taken: &HashSet<String>) -> String {
    let clean: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':') { c } else { '-' }).collect();
    let clean = if clean.is_empty() { "x".to_string() } else { clean };
    if !taken.contains(&clean) {
        return clean;
    }
    let mut i = 2;
    loop {
        let c = format!("{}-{}", clean, i);
        if !taken.contains(&c) {
            return c;
        }
        i += 1;
    }
}

impl Architecture {
    pub fn all_ids(&self) -> HashSet<String> {
        self.zones.iter().map(|z| z.id.clone()).chain(self.nodes.iter().map(|n| n.id.clone())).chain(self.edges.iter().map(|e| e.id.clone())).collect()
    }
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn zone(&self, id: &str) -> Option<&Zone> {
        self.zones.iter().find(|z| z.id == id)
    }
}
