//! The Infrastructure Digital Twin: the single source of truth.
//!
//! Canvas, AI chat, checks, simulation, documentation and IaC export are all *views* of this
//! one typed graph. Every change – manual or proposed by an LLM – goes through the same
//! validation (`validate`) and the same rule engine.

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
    /// Largest failure the service must survive: `instance`, `site` (default) or `geo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_tolerance: Option<String>,
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
        Value::String(s) => s.trim().replace(',', ".").parse::<f64>().ok(),
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
        Issue {
            level: IssueLevel::Error,
            code: code.into(),
            message,
            subject: subject.map(|s| s.to_string()),
        }
    }
    fn warn(code: &str, subject: Option<&str>, message: String) -> Self {
        Issue {
            level: IssueLevel::Warning,
            code: code.into(),
            message,
            subject: subject.map(|s| s.to_string()),
        }
    }
}

pub struct ValidationLimits {
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_zones: usize,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_nodes: 500,
            max_edges: 2000,
            max_zones: 100,
        }
    }
}

/// Validate referential integrity and catalog conformity. Used for imports, manual saves and –
/// crucially – for everything an LLM produces (errors are fed back to the model for repair).
pub fn validate(
    arch: &Architecture,
    cat: &crate::catalog::Catalog,
    limits: &ValidationLimits,
) -> Vec<Issue> {
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
        if cat.zone_type(&z.zone_type).is_none() {
            out.push(Issue::err(
                "unknown_zone_type",
                Some(&z.id),
                format!("zone '{}' has unknown type '{}'", z.id, z.zone_type),
            ));
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
        match cat.kind(&n.kind) {
            None => out.push(Issue::err(
                "unknown_kind",
                Some(&n.id),
                format!("node '{}' has unknown kind '{}' (allowed: {})", n.id, n.kind, cat.kinds.iter().map(|k| k.id.as_str()).collect::<Vec<_>>().join(", ")),
            )),
            Some(k) => {
                if let Some(role) = &n.role {
                    if !k.roles.is_empty() && !k.roles.iter().any(|r| &r.id == role) {
                        out.push(Issue::warn(
                            "unknown_role",
                            Some(&n.id),
                            format!("node '{}' role '{}' is not defined for kind '{}'", n.id, role, n.kind),
                        ));
                    }
                }
            }
        }
        if let Some(z) = &n.zone_id {
            if !zone_ids.contains_key(z.as_str()) {
                out.push(Issue::err("dangling_zone", Some(&n.id), format!("node '{}' zone '{}' does not exist", n.id, z)));
            }
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
        if cat.edge_type(&e.edge_type).is_none() {
            out.push(Issue::err(
                "unknown_edge_type",
                Some(&e.id),
                format!("edge '{}' has unknown type '{}' (allowed: {})", e.id, e.edge_type, cat.edge_types.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")),
            ));
        }
    }
    out
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.level == IssueLevel::Error)
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

/// Make an id unique within `taken` by appending `-2`, `-3`, …
pub fn unique_id(base: &str, taken: &HashSet<String>) -> String {
    let clean: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':') { c } else { '-' })
        .collect();
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
        self.zones
            .iter()
            .map(|z| z.id.clone())
            .chain(self.nodes.iter().map(|n| n.id.clone()))
            .chain(self.edges.iter().map(|e| e.id.clone()))
            .collect()
    }
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn zone(&self, id: &str) -> Option<&Zone> {
        self.zones.iter().find(|z| z.id == id)
    }
}
