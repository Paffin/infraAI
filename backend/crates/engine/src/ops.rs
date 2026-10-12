//! Patch operations and structural diff.
//!
//! The LLM never edits the architecture directly. It proposes a list of [`Op`]s which are
//! applied atomically to a *copy*, validated, evaluated by the rule engine and shown to the
//! user as a diff with before/after scores. Deterministic fixes produced by the engine use the
//! very same operations.

use crate::catalog::Catalog;
use crate::model::{unique_id, validate, Architecture, Edge, Issue, IssueLevel, Node, Props, Requirements, ValidationLimits, Zone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    AddZone {
        zone: Zone,
    },
    /// `set` keys: `name`, `type`, `parentId`, `role`, `props.<key>`; `null` clears.
    UpdateZone {
        id: String,
        set: Map<String, Value>,
    },
    RemoveZone {
        id: String,
        #[serde(default)]
        cascade: bool,
    },
    AddNode {
        node: Node,
    },
    /// `set` keys: `name`, `kind`, `zoneId`, `group`, `role`, `ui`, `props.<key>`; `null` clears.
    UpdateNode {
        id: String,
        set: Map<String, Value>,
    },
    RemoveNode {
        id: String,
    },
    AddEdge {
        edge: Edge,
    },
    /// `set` keys: `type`, `label`, `source`, `target`, `props.<key>`; `null` clears.
    UpdateEdge {
        id: String,
        set: Map<String, Value>,
    },
    RemoveEdge {
        id: String,
    },
    /// Merge into requirements (camelCase keys). `null` clears.
    SetRequirements {
        set: Map<String, Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub architecture: Architecture,
    /// Edges removed implicitly because a node/zone they touch was removed.
    pub cascaded_edges: Vec<String>,
    /// Ids that had to be renamed because they collided (`old -> new`).
    pub renamed: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyError {
    pub issues: Vec<Issue>,
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.issues.iter().map(|i| i.message.clone()).collect::<Vec<_>>().join("; "))
    }
}
impl std::error::Error for ApplyError {}

fn op_err(idx: usize, msg: String) -> Issue {
    Issue { level: IssueLevel::Error, code: "op_failed".into(), message: format!("op #{}: {}", idx, msg), subject: None }
}

fn set_props(props: &mut Props, key: &str, value: &Value) {
    if value.is_null() {
        props.remove(key);
    } else {
        props.insert(key.to_string(), value.clone());
    }
}

fn opt_str(v: &Value) -> Result<Option<String>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(if s.is_empty() { None } else { Some(s.clone()) }),
        other => Err(format!("expected string or null, got {}", other)),
    }
}

fn req_str(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) if !s.is_empty() => Ok(s.clone()),
        other => Err(format!("expected non-empty string, got {}", other)),
    }
}

/// Apply operations atomically. Either all succeed and the result validates, or nothing changes.
pub fn apply_ops(base: &Architecture, ops: &[Op], cat: &Catalog, limits: &ValidationLimits) -> Result<ApplyOutcome, ApplyError> {
    let mut a = base.clone();
    let mut issues: Vec<Issue> = Vec::new();
    let mut cascaded: Vec<String> = Vec::new();
    let mut renamed: BTreeMap<String, String> = BTreeMap::new();

    for (i, op) in ops.iter().enumerate() {
        let r: Result<(), String> = (|| {
            match op {
                Op::AddZone { zone } => {
                    let mut z = zone.clone();
                    let taken = a.all_ids();
                    if taken.contains(&z.id) {
                        let nid = unique_id(&z.id, &taken);
                        renamed.insert(z.id.clone(), nid.clone());
                        z.id = nid;
                    }
                    a.zones.push(z);
                }
                Op::UpdateZone { id, set } => {
                    let id = renamed.get(id).cloned().unwrap_or_else(|| id.clone());
                    let z = a.zones.iter_mut().find(|z| z.id == id).ok_or(format!("zone '{}' not found", id))?;
                    for (k, v) in set {
                        match k.as_str() {
                            "name" => z.name = req_str(v)?,
                            "type" => z.zone_type = req_str(v)?,
                            "parentId" => z.parent_id = opt_str(v)?,
                            "role" => z.role = opt_str(v)?,
                            k if k.starts_with("props.") => set_props(&mut z.props, &k[6..], v),
                            other => return Err(format!("unknown zone field '{}'", other)),
                        }
                    }
                }
                Op::RemoveZone { id, cascade } => {
                    let id = renamed.get(id).cloned().unwrap_or_else(|| id.clone());
                    if !a.zones.iter().any(|z| z.id == id) {
                        return Err(format!("zone '{}' not found", id));
                    }
                    let mut doomed: HashSet<String> = HashSet::new();
                    doomed.insert(id.clone());
                    loop {
                        let before = doomed.len();
                        for z in &a.zones {
                            if let Some(p) = &z.parent_id {
                                if doomed.contains(p) {
                                    doomed.insert(z.id.clone());
                                }
                            }
                        }
                        if doomed.len() == before {
                            break;
                        }
                    }
                    let has_children = a.zones.iter().any(|z| doomed.contains(&z.id) && z.id != id) || a.nodes.iter().any(|n| n.zone_id.as_ref().map(|z| doomed.contains(z)).unwrap_or(false));
                    if has_children && !cascade {
                        return Err(format!("zone '{}' is not empty (use cascade or remove its contents first)", id));
                    }
                    let doomed_nodes: HashSet<String> = a.nodes.iter().filter(|n| n.zone_id.as_ref().map(|z| doomed.contains(z)).unwrap_or(false)).map(|n| n.id.clone()).collect();
                    a.zones.retain(|z| !doomed.contains(&z.id));
                    a.nodes.retain(|n| !doomed_nodes.contains(&n.id));
                    let (keep, drop): (Vec<_>, Vec<_>) = a.edges.drain(..).partition(|e| !doomed_nodes.contains(&e.source) && !doomed_nodes.contains(&e.target));
                    cascaded.extend(drop.into_iter().map(|e| e.id));
                    a.edges = keep;
                }
                Op::AddNode { node } => {
                    let mut n = node.clone();
                    let taken = a.all_ids();
                    if taken.contains(&n.id) {
                        let nid = unique_id(&n.id, &taken);
                        renamed.insert(n.id.clone(), nid.clone());
                        n.id = nid;
                    }
                    if let Some(z) = &n.zone_id {
                        if let Some(r) = renamed.get(z) {
                            n.zone_id = Some(r.clone());
                        }
                    }
                    a.nodes.push(n);
                }
                Op::UpdateNode { id, set } => {
                    let id = renamed.get(id).cloned().unwrap_or_else(|| id.clone());
                    let n = a.nodes.iter_mut().find(|n| n.id == id).ok_or(format!("node '{}' not found", id))?;
                    for (k, v) in set {
                        match k.as_str() {
                            "name" => n.name = req_str(v)?,
                            "kind" => n.kind = req_str(v)?,
                            "zoneId" => n.zone_id = opt_str(v)?.map(|z| renamed.get(&z).cloned().unwrap_or(z)),
                            "group" => n.group = opt_str(v)?,
                            "role" => n.role = opt_str(v)?,
                            "ui" => n.ui = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?,
                            k if k.starts_with("props.") => set_props(&mut n.props, &k[6..], v),
                            other => return Err(format!("unknown node field '{}'", other)),
                        }
                    }
                }
                Op::RemoveNode { id } => {
                    let id = renamed.get(id).cloned().unwrap_or_else(|| id.clone());
                    if !a.nodes.iter().any(|n| n.id == id) {
                        return Err(format!("node '{}' not found", id));
                    }
                    a.nodes.retain(|n| n.id != id);
                    let (keep, drop): (Vec<_>, Vec<_>) = a.edges.drain(..).partition(|e| e.source != id && e.target != id);
                    cascaded.extend(drop.into_iter().map(|e| e.id));
                    a.edges = keep;
                }
                Op::AddEdge { edge } => {
                    let mut e = edge.clone();
                    let taken = a.all_ids();
                    if taken.contains(&e.id) {
                        let nid = unique_id(&e.id, &taken);
                        renamed.insert(e.id.clone(), nid.clone());
                        e.id = nid;
                    }
                    if let Some(r) = renamed.get(&e.source) {
                        e.source = r.clone();
                    }
                    if let Some(r) = renamed.get(&e.target) {
                        e.target = r.clone();
                    }
                    a.edges.push(e);
                }
                Op::UpdateEdge { id, set } => {
                    let id = renamed.get(id).cloned().unwrap_or_else(|| id.clone());
                    let e = a.edges.iter_mut().find(|e| e.id == id).ok_or(format!("edge '{}' not found", id))?;
                    for (k, v) in set {
                        match k.as_str() {
                            "type" => e.edge_type = req_str(v)?,
                            "label" => e.label = opt_str(v)?,
                            "source" => e.source = req_str(v)?,
                            "target" => e.target = req_str(v)?,
                            k if k.starts_with("props.") => set_props(&mut e.props, &k[6..], v),
                            other => return Err(format!("unknown edge field '{}'", other)),
                        }
                    }
                }
                Op::RemoveEdge { id } => {
                    let id = renamed.get(id).cloned().unwrap_or_else(|| id.clone());
                    if !a.edges.iter().any(|e| e.id == id) {
                        return Err(format!("edge '{}' not found", id));
                    }
                    a.edges.retain(|e| e.id != id);
                }
                Op::SetRequirements { set } => {
                    let mut cur = serde_json::to_value(&a.requirements).map_err(|e| e.to_string())?;
                    let obj = cur.as_object_mut().ok_or("requirements is not an object")?;
                    for (k, v) in set {
                        if v.is_null() {
                            obj.remove(k);
                        } else {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                    a.requirements = serde_json::from_value::<Requirements>(cur).map_err(|e| format!("invalid requirements: {}", e))?;
                }
            }
            Ok(())
        })();
        if let Err(m) = r {
            issues.push(op_err(i, m));
        }
    }

    if !issues.is_empty() {
        return Err(ApplyError { issues });
    }
    let v = validate(&a, cat, limits);
    let errors: Vec<Issue> = v.into_iter().filter(|i| i.level == IssueLevel::Error).collect();
    if !errors.is_empty() {
        return Err(ApplyError { issues: errors });
    }
    Ok(ApplyOutcome { architecture: a, cascaded_edges: cascaded, renamed })
}

// ---------------------------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub path: String,
    pub from: Value,
    pub to: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntityRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntityChange {
    pub id: String,
    pub name: String,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntityDiff {
    pub added: Vec<EntityRef>,
    pub removed: Vec<EntityRef>,
    pub changed: Vec<EntityChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArchDiff {
    pub requirements: Vec<Change>,
    pub zones: EntityDiff,
    pub nodes: EntityDiff,
    pub edges: EntityDiff,
}

impl ArchDiff {
    pub fn is_empty(&self) -> bool {
        self.requirements.is_empty() && self.zones.is_empty() && self.nodes.is_empty() && self.edges.is_empty()
    }
    pub fn counts(&self) -> Value {
        json!({
            "added": self.zones.added.len() + self.nodes.added.len() + self.edges.added.len(),
            "removed": self.zones.removed.len() + self.nodes.removed.len() + self.edges.removed.len(),
            "changed": self.zones.changed.len() + self.nodes.changed.len() + self.edges.changed.len() + (!self.requirements.is_empty()) as usize,
        })
    }
}

impl EntityDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

fn flatten(prefix: &str, v: &Value, out: &mut BTreeMap<String, Value>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, vv) in m {
                let p = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
                flatten(&p, vv, out);
            }
        }
        _ => {
            out.insert(prefix.to_string(), v.clone());
        }
    }
}

fn changes_between(a: &Value, b: &Value, ignore_ui: bool) -> Vec<Change> {
    let mut fa = BTreeMap::new();
    let mut fb = BTreeMap::new();
    flatten("", a, &mut fa);
    flatten("", b, &mut fb);
    let mut keys: Vec<&String> = fa.keys().chain(fb.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut out = Vec::new();
    for k in keys {
        if ignore_ui && (k == "ui" || k.starts_with("ui.") || k == "proposed") {
            continue;
        }
        let va = fa.get(k).cloned().unwrap_or(Value::Null);
        let vb = fb.get(k).cloned().unwrap_or(Value::Null);
        if va != vb {
            out.push(Change { path: k.clone(), from: va, to: vb });
        }
    }
    out
}

fn diff_entities<T: Serialize>(old: &[T], new: &[T], id_of: impl Fn(&T) -> &str, name_of: impl Fn(&T) -> String, ignore_ui: bool) -> EntityDiff {
    let mut d = EntityDiff::default();
    let omap: BTreeMap<&str, &T> = old.iter().map(|x| (id_of(x), x)).collect();
    let nmap: BTreeMap<&str, &T> = new.iter().map(|x| (id_of(x), x)).collect();
    for x in new {
        if !omap.contains_key(id_of(x)) {
            d.added.push(EntityRef { id: id_of(x).into(), name: name_of(x) });
        }
    }
    for x in old {
        match nmap.get(id_of(x)) {
            None => d.removed.push(EntityRef { id: id_of(x).into(), name: name_of(x) }),
            Some(y) => {
                let ch = changes_between(&serde_json::to_value(x).unwrap_or(Value::Null), &serde_json::to_value(y).unwrap_or(Value::Null), ignore_ui);
                if !ch.is_empty() {
                    d.changed.push(EntityChange { id: id_of(x).into(), name: name_of(y), changes: ch });
                }
            }
        }
    }
    d
}

/// Structural diff between two architectures. Canvas positions (`ui`) are ignored unless
/// `include_ui` is set – moving a card is not an architectural change.
pub fn diff(old: &Architecture, new: &Architecture, include_ui: bool) -> ArchDiff {
    ArchDiff {
        requirements: changes_between(&serde_json::to_value(&old.requirements).unwrap_or(Value::Null), &serde_json::to_value(&new.requirements).unwrap_or(Value::Null), false),
        zones: diff_entities(&old.zones, &new.zones, |z| z.id.as_str(), |z| z.name.clone(), !include_ui),
        nodes: diff_entities(&old.nodes, &new.nodes, |n| n.id.as_str(), |n| n.name.clone(), !include_ui),
        edges: diff_entities(&old.edges, &new.edges, |e| e.id.as_str(), |e| e.label.clone().unwrap_or_else(|| format!("{} → {}", e.source, e.target)), !include_ui),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults;

    fn base() -> Architecture {
        defaults::templates().into_iter().next().expect("at least one template").architecture
    }

    #[test]
    fn apply_is_atomic_and_validates() {
        let cat = defaults::catalog();
        let a = base();
        let n_before = a.nodes.len();
        let bad = vec![Op::RemoveNode { id: a.nodes[0].id.clone() }, Op::RemoveNode { id: "does-not-exist".into() }];
        assert!(apply_ops(&a, &bad, &cat, &ValidationLimits::default()).is_err());
        assert_eq!(a.nodes.len(), n_before);
    }

    #[test]
    fn remove_node_cascades_edges_and_diff_sees_it() {
        let cat = defaults::catalog();
        let a = base();
        let victim = a.edges[0].source.clone();
        let out = apply_ops(&a, &[Op::RemoveNode { id: victim.clone() }], &cat, &ValidationLimits::default()).unwrap();
        assert!(!out.cascaded_edges.is_empty());
        let d = diff(&a, &out.architecture, false);
        assert!(d.nodes.removed.iter().any(|n| n.id == victim));
        assert!(!d.edges.removed.is_empty());
    }

    #[test]
    fn colliding_ids_are_renamed() {
        let cat = defaults::catalog();
        let a = base();
        let mut n = a.nodes[0].clone();
        n.name = "copy".into();
        let out = apply_ops(&a, &[Op::AddNode { node: n.clone() }], &cat, &ValidationLimits::default()).unwrap();
        assert_eq!(out.architecture.nodes.len(), a.nodes.len() + 1);
        assert!(out.renamed.contains_key(&n.id));
    }

    #[test]
    fn update_node_props_and_clear() {
        let cat = defaults::catalog();
        let a = base();
        let id = a.nodes[1].id.clone();
        let mut set = Map::new();
        set.insert("props.replicas".into(), json!(7));
        let out = apply_ops(&a, &[Op::UpdateNode { id: id.clone(), set }], &cat, &ValidationLimits::default()).unwrap();
        assert_eq!(out.architecture.node(&id).unwrap().props["replicas"], json!(7));
        let mut set = Map::new();
        set.insert("props.replicas".into(), Value::Null);
        let out2 = apply_ops(&out.architecture, &[Op::UpdateNode { id: id.clone(), set }], &cat, &ValidationLimits::default()).unwrap();
        assert!(!out2.architecture.node(&id).unwrap().props.contains_key("replicas"));
    }
}
