//! From the architecture model to a deployment plan.
//!
//! The plan is the single intermediate form behind every IaC format: which Kubernetes cluster
//! of the architecture runs which logical service, which operators have to exist first, which
//! objects and Helm releases result, and – just as important – which components were *not*
//! generated and why. Nothing is faked: a component without a generator is reported, not stubbed.

use super::dsl::{slug, to_mv, Dsl, DslError};
use crate::catalog::Semantics;
use crate::config::{EngineConfig, IacGranularity, IacTarget, IacTemplate, Operator};
use crate::graph::Index;
use crate::model::Architecture;
use crate::text::interpolate_l;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

// -------------------------------------------------------------------------------------------
// Report
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub clusters: Vec<ClusterInfo>,
    pub generated: Vec<Generated>,
    pub skipped: Vec<Skipped>,
    pub notes: Vec<Note>,
    pub prerequisites: Vec<Prerequisite>,
    pub secrets: Vec<SecretNeed>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClusterInfo {
    pub nodes: Vec<String>,
    pub name: String,
    pub slug: String,
    pub zones: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Generated {
    pub unit: String,
    pub name: String,
    pub kind: String,
    pub cluster: String,
    pub nodes: Vec<String>,
    /// Human-readable list of produced objects/releases.
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub unit: String,
    pub name: String,
    pub kind: String,
    pub nodes: Vec<String>,
    /// `no_template`, `no_generator`, `hosting`, `no_cluster`, `needs_input`, `template_error`, `disabled`, `no_workload`.
    pub code: String,
    pub message: String,
    /// Properties to fill in (for `needs_input`).
    pub props: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// `info` or `warn`.
    pub level: String,
    pub unit: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prerequisite {
    pub cluster: String,
    pub id: String,
    pub title: String,
    pub release: String,
    pub namespace: String,
    pub chart: String,
    pub repo: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SecretNeed {
    pub cluster: String,
    pub namespace: String,
    pub name: String,
    pub keys: Vec<String>,
    pub unit: String,
    pub hint: Option<String>,
}

// -------------------------------------------------------------------------------------------
// Plan
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ClusterPlan {
    /// Ids of the nodes that make up the cluster (usually one; several = one cluster stretched over their zones).
    pub nodes: Vec<String>,
    pub name: String,
    pub slug: String,
    pub zones: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PlannedObject {
    pub cluster: usize,
    pub unit: String,
    /// Operators (ids from `export.k8s.operators`) the object needs.
    pub requires: Vec<String>,
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub body: Value,
    /// Unique (per cluster) file name, e.g. `pg-main-cluster.yaml`.
    pub file: String,
}

#[derive(Debug, Clone)]
pub struct PlannedRelease {
    pub cluster: usize,
    pub unit: Option<String>,
    /// Operator ids a workload chart needs (resolved to `needs` once operators are planned).
    pub requires: Vec<String>,
    pub name: String,
    pub namespace: String,
    pub chart: String,
    pub repo: String,
    pub version: Option<String>,
    pub values: Value,
    /// Prerequisite (operator/controller) rather than a workload.
    pub operator: bool,
    /// Id in `export.k8s.operators` for prerequisites.
    pub op_id: Option<String>,
    /// Names of releases of the same cluster that must be installed first.
    pub needs: Vec<String>,
}

impl PlannedRelease {
    pub fn key(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub arch_name: String,
    pub arch_slug: String,
    pub clusters: Vec<ClusterPlan>,
    pub objects: Vec<PlannedObject>,
    pub releases: Vec<PlannedRelease>,
    /// (cluster, namespace) pairs that have to exist.
    pub namespaces: Vec<(usize, String)>,
    pub report: Report,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.releases.is_empty()
    }
}

/// Texts of the report come from configuration (`export.texts`), never from code.
struct Texts<'a> {
    cfg: &'a EngineConfig,
    lang: &'a str,
}

impl Texts<'_> {
    fn get(&self, key: &str, vars: &[(&str, Value)]) -> String {
        let map: BTreeMap<String, Value> = vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        interpolate_l(&self.cfg.settings.export.text(self.lang, key), &map, self.lang)
    }
}

fn unique(taken: &mut HashSet<String>, base: &str) -> String {
    let base = if base.is_empty() { "x".to_string() } else { base.to_string() };
    let mut s = base.clone();
    let mut i = 2;
    while !taken.insert(s.clone()) {
        s = format!("{base}-{i}");
        i += 1;
    }
    s
}

// -------------------------------------------------------------------------------------------
// Context for templates
// -------------------------------------------------------------------------------------------

/// Every property declared by the kind (and the common ones) is present – `null` when unset – so
/// templates can test optional properties without tripping over "undefined".
fn props_json(ix: &Index<'_>, ni: usize) -> Value {
    let mut m = Map::new();
    let kind = &ix.arch.nodes[ni].kind;
    if let Some(k) = ix.kind[ni] {
        for p in &k.props {
            m.insert(p.key.clone(), Value::Null);
        }
    }
    for p in &ix.cat.common_props {
        m.entry(p.key.clone()).or_insert(Value::Null);
    }
    let _ = kind;
    for (k, v) in &ix.eff[ni].map {
        m.insert(k.clone(), v.clone());
    }
    Value::Object(m)
}

fn member_json(ix: &Index<'_>, ni: usize) -> Value {
    let n = &ix.arch.nodes[ni];
    let zone = n.zone_id.as_deref().and_then(|z| ix.zone_ix.get(z)).map(|&zi| ix.zone_name(zi).to_string());
    json!({
        "id": n.id,
        "name": n.name,
        "slug": slug(&n.id),
        "kind": n.kind,
        "zone": n.zone_id,
        "zoneName": zone,
        "role": n.role,
        "replicas": ix.replicas(ni),
        "props": props_json(ix, ni),
    })
}

/// A cluster for `ni`: the cluster it is part of, or the nearest one by zone (innermost zone first).
/// The flag tells that several clusters competed for the same zone.
fn resolve_cluster(ix: &Index<'_>, clusters: &[Vec<usize>], ni: usize) -> Option<(usize, bool)> {
    if let Some(p) = clusters.iter().position(|c| c.contains(&ni)) {
        return Some((p, false));
    }
    for &z in &ix.zone_chain[ni] {
        let hits: Vec<usize> = clusters.iter().enumerate().filter(|(_, c)| c.iter().any(|&m| ix.zone_chain[m].contains(&z))).map(|(p, _)| p).collect();
        if !hits.is_empty() {
            return Some((hits[0], hits.len() > 1));
        }
    }
    if clusters.len() == 1 {
        return Some((0, false));
    }
    None
}

fn pick_lead(ix: &Index<'_>, members: &[usize]) -> usize {
    members.iter().copied().find(|&m| ix.arch.nodes[m].role.as_deref() == Some("primary")).or_else(|| members.iter().copied().find(|&m| !ix.is_witness(m))).unwrap_or(members[0])
}

#[derive(Clone)]
struct Site {
    cluster: usize,
    members: Vec<usize>,
    instances: u32,
    primary: bool,
}

// -------------------------------------------------------------------------------------------
// Building
// -------------------------------------------------------------------------------------------

pub fn build_plan(arch: &Architecture, cfg: &EngineConfig, lang: &str) -> Plan {
    let ix = Index::build(arch, &cfg.catalog, &cfg.settings);
    let ex = &cfg.settings.export;
    let k8s = &ex.k8s;
    let dsl = Dsl::new();
    let tx = Texts { cfg, lang };
    let arch_name = arch.meta.name.clone().unwrap_or_else(|| "architecture".into());
    let mut plan = Plan {
        arch_slug: {
            let s = slug(&arch_name);
            if s.is_empty() {
                "architecture".into()
            } else {
                s
            }
        },
        arch_name,
        ..Default::default()
    };

    // clusters of the architecture: nodes of a cluster kind that share a `clusterName` form one
    // cluster (stretched over their zones); a node without a name is a cluster of its own
    let mut cluster_members: Vec<Vec<usize>> = vec![];
    let mut cluster_keys: Vec<String> = vec![];
    for ni in (0..arch.nodes.len()).filter(|&ni| ix.traits[ni].contains(&k8s.cluster_trait)) {
        let key = ix.eff[ni].text("clusterName").map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| arch.nodes[ni].id.clone());
        match cluster_keys.iter().position(|k| *k == key) {
            Some(p) => cluster_members[p].push(ni),
            None => {
                cluster_keys.push(key);
                cluster_members.push(vec![ni]);
            }
        }
    }
    let mut taken = HashSet::new();
    for (ci, members) in cluster_members.iter().enumerate() {
        let named = ix.eff[members[0]].text("clusterName").map(|n| !n.trim().is_empty()).unwrap_or(false);
        let name = if named { cluster_keys[ci].clone() } else { arch.nodes[members[0]].name.clone() };
        let mut zones: Vec<String> = vec![];
        for &m in members {
            if let Some(z) = &arch.nodes[m].zone_id {
                if !zones.contains(z) {
                    zones.push(z.clone());
                }
            }
        }
        plan.clusters.push(ClusterPlan { nodes: members.iter().map(|&m| arch.nodes[m].id.clone()).collect(), name, slug: unique(&mut taken, &slug(&cluster_keys[ci])), zones });
    }
    plan.report.clusters = plan.clusters.iter().map(|c| ClusterInfo { nodes: c.nodes.clone(), name: c.name.clone(), slug: c.slug.clone(), zones: c.zones.clone() }).collect();
    if cluster_members.is_empty() {
        plan.report.notes.push(Note { level: "warn".into(), unit: None, message: tx.get("iac.note.no_clusters", &[("trait", json!(k8s.cluster_trait))]) });
    }

    // architecture-wide facts for templates
    let mut has = Map::new();
    for t in &ix.traits {
        for tr in t {
            has.insert(tr.clone(), json!(true));
        }
    }
    let labels_tpl: Vec<(String, String)> = k8s.labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    let mut operators_needed: Vec<BTreeSet<String>> = vec![BTreeSet::new(); plan.clusters.len()];
    let mut file_names: Vec<HashSet<String>> = vec![HashSet::new(); plan.clusters.len()];
    let mut ns_used: BTreeSet<(usize, String)> = BTreeSet::new();
    let mut release_names: Vec<HashSet<String>> = vec![HashSet::new(); plan.clusters.len()];

    for (u, unit) in ix.units.iter().enumerate() {
        let kind_name = ix.kind[unit.members[0]].map(|k| k.name.get(lang)).unwrap_or_else(|| unit.kind.clone());
        let node_ids: Vec<String> = unit.members.iter().map(|&m| arch.nodes[m].id.clone()).collect();
        let skip = |plan: &mut Plan, code: &str, message: String, props: Vec<String>, nodes: Vec<String>| {
            // the same reason for several sites is reported once, with all affected nodes
            if let Some(prev) = plan.report.skipped.iter_mut().find(|s| s.unit == unit.key && s.code == code && s.message == message) {
                for n in nodes {
                    if !prev.nodes.contains(&n) {
                        prev.nodes.push(n);
                    }
                }
                return;
            }
            plan.report.skipped.push(Skipped { unit: unit.key.clone(), name: unit.name.clone(), kind: unit.kind.clone(), nodes, code: code.into(), message, props });
        };
        let tpl: Option<&IacTemplate> = cfg.iac.iter().find(|t| t.id == unit.kind);
        let Some(tpl) = tpl else {
            skip(&mut plan, "no_template", tx.get("iac.skip.no_template", &[("kind", json!(kind_name))]), vec![], node_ids);
            continue;
        };
        if !tpl.enabled {
            skip(&mut plan, "disabled", tx.get("iac.skip.disabled", &[("kind", json!(kind_name))]), vec![], node_ids);
            continue;
        }
        if tpl.target == IacTarget::None {
            let reason = tpl.reason.as_ref().map(|r| r.get(lang)).unwrap_or_else(|| tx.get("iac.skip.no_generator", &[("kind", json!(kind_name))]));
            skip(&mut plan, "no_generator", reason, vec![], node_ids);
            continue;
        }

        // placement: which members run on which cluster
        let mut placed: Vec<(usize, usize)> = vec![]; // (member, cluster position)
        let mut unplaced: BTreeMap<&'static str, Vec<usize>> = BTreeMap::new();
        for &m in &unit.members {
            let hosting = ix.eff[m].text("hosting").unwrap_or_else(|| "auto".into());
            if hosting != "auto" && hosting != "kubernetes" {
                unplaced.entry("hosting").or_default().push(m);
                continue;
            }
            match resolve_cluster(&ix, &cluster_members, m) {
                Some((c, ambiguous)) => {
                    if ambiguous {
                        let zone = arch.nodes[m].zone_id.clone().unwrap_or_default();
                        plan.report.notes.push(Note {
                            level: "info".into(),
                            unit: Some(unit.key.clone()),
                            message: tx.get("iac.note.ambiguous_cluster", &[("node", json!(arch.nodes[m].name)), ("zone", json!(zone)), ("cluster", json!(plan.clusters[c].name))]),
                        });
                    }
                    placed.push((m, c));
                }
                None => unplaced.entry("no_cluster").or_default().push(m),
            }
        }
        for (code, ms) in &unplaced {
            let mut names: Vec<String> = ms.iter().map(|&m| arch.nodes[m].name.clone()).collect();
            names.dedup();
            let names: Vec<String> = {
                let mut seen = HashSet::new();
                names.into_iter().filter(|n| seen.insert(n.clone())).collect()
            };
            let hosting = ms.first().and_then(|&m| ix.eff[m].text("hosting")).unwrap_or_default();
            let msg = tx.get(&format!("iac.skip.{code}"), &[("kind", json!(kind_name)), ("nodes", json!(names.join(", "))), ("hosting", json!(hosting)), ("trait", json!(k8s.cluster_trait))]);
            skip(&mut plan, code, msg, vec![], ms.iter().map(|&m| arch.nodes[m].id.clone()).collect());
        }
        if placed.is_empty() {
            continue;
        }

        // workloads: the whole unit per cluster, or each node on its own
        let groups: Vec<(String, String, Vec<(usize, usize)>)> = match tpl.granularity {
            IacGranularity::Unit => vec![(unit.key.clone(), unit.name.clone(), placed.clone())],
            IacGranularity::Node => placed.iter().map(|&(m, c)| (arch.nodes[m].id.clone(), arch.nodes[m].name.clone(), vec![(m, c)])).collect(),
        };
        for (key, display, members) in groups {
            // sites = placed members grouped by cluster; the one with the primary (or first) member leads
            let mut by_cluster: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for &(m, c) in &members {
                by_cluster.entry(c).or_default().push(m);
            }
            let mut sites: Vec<Site> = by_cluster
                .into_iter()
                .map(|(c, ms)| {
                    let instances: u32 = ms.iter().filter(|&&m| !ix.is_witness(m)).map(|&m| ix.replicas(m)).sum();
                    let primary = ms.iter().any(|&m| arch.nodes[m].role.as_deref() == Some("primary"));
                    Site { cluster: c, members: ms, instances, primary }
                })
                .collect();
            if !sites.iter().any(|s| s.primary) {
                if let Some(first) = sites.iter_mut().find(|s| s.instances > 0) {
                    first.primary = true;
                }
            } else {
                // only one primary site
                let mut seen = false;
                for s in sites.iter_mut() {
                    if s.primary && seen {
                        s.primary = false;
                    }
                    seen |= s.primary;
                }
            }
            sites.sort_by_key(|s| !s.primary);
            let live: Vec<Site> = sites.iter().filter(|s| s.instances > 0).cloned().collect();
            if live.is_empty() {
                skip(&mut plan, "no_workload", tx.get("iac.skip.no_workload", &[("kind", json!(kind_name))]), vec![], members.iter().map(|&(m, _)| arch.nodes[m].id.clone()).collect());
                continue;
            }

            let category = ix.kind[members[0].0].map(|k| k.category.clone()).unwrap_or_default();
            let ns_default = k8s.namespaces.get(&category).cloned().unwrap_or_else(|| k8s.default_namespace.clone());
            // short enough that generated suffixes (`-pooler-rw`, `-backup`) stay within the 63-character limit
            let wl_slug = {
                let mut s = slug(&key);
                s.truncate(40);
                let s = s.trim_end_matches('-').to_string();
                if s.is_empty() {
                    "workload".to_string()
                } else {
                    s
                }
            };

            let site_json: Vec<Value> = live
                .iter()
                .map(|s| {
                    let zones: BTreeSet<String> = s.members.iter().filter_map(|&m| arch.nodes[m].zone_id.clone()).collect();
                    json!({
                        "cluster": plan.clusters[s.cluster].slug,
                        "clusterName": plan.clusters[s.cluster].name,
                        "role": if s.primary { "primary" } else { "replica" },
                        "instances": s.instances,
                        "zones": zones,
                        "members": s.members.iter().map(|&m| arch.nodes[m].id.clone()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let all_members: Vec<usize> = live.iter().flat_map(|s| s.members.iter().copied()).collect();
            let member_cluster: HashMap<usize, usize> = live.iter().flat_map(|s| s.members.iter().map(move |&m| (m, s.cluster))).collect();

            // replication between members of this workload
            let mut repl = vec![];
            let (mut sync_local, mut sync_cross, mut async_local, mut async_cross, mut sync_block) = (false, false, false, false, false);
            for (ei, ends) in ix.edge_ends.iter().enumerate() {
                let Some((s, t)) = ends else { continue };
                if ix.sem(ei) != Some(Semantics::Replication) || !member_cluster.contains_key(s) || !member_cluster.contains_key(t) {
                    continue;
                }
                let mode = ix.edge_eff[ei].text("mode").unwrap_or_else(|| arch.edges[ei].edge_type.clone());
                let cross = member_cluster[s] != member_cluster[t];
                match (mode.as_str(), cross) {
                    ("sync", false) => {
                        sync_local = true;
                        sync_block |= ix.edge_eff[ei].text("onReplicaLoss").as_deref() == Some("block");
                    }
                    ("sync", true) => sync_cross = true,
                    (_, false) => async_local = true,
                    (_, true) => async_cross = true,
                }
                repl.push(json!({"from": arch.nodes[*s].id, "to": arch.nodes[*t].id, "type": arch.edges[ei].edge_type, "mode": mode, "crossCluster": cross,
                    "lagSeconds": ix.edge_eff[ei].num("lagSeconds"), "latencyMs": ix.edge_eff[ei].num("latencyMs"), "onReplicaLoss": ix.edge_eff[ei].text("onReplicaLoss")}));
            }

            // backup targets
            let mut backups = vec![];
            let mut seen_targets = HashSet::new();
            for &m in &all_members {
                for &ei in ix.out_edges[m].iter().chain(ix.in_edges[m].iter()) {
                    if ix.sem(ei) != Some(Semantics::Backup) {
                        continue;
                    }
                    let Some((s, t)) = ix.edge_ends[ei] else { continue };
                    let other = if s == m { t } else { s };
                    let target = if ix.traits[other].contains("backup-target") || !ix.traits[m].contains("backup-target") { other } else { m };
                    if target == m || !seen_targets.insert(target) {
                        continue;
                    }
                    let mut v = member_json(&ix, target);
                    v["edge"] = json!({"type": arch.edges[ei].edge_type, "intervalSeconds": ix.edge_eff[ei].num("intervalSeconds"), "props": ix.edge_eff[ei].map});
                    backups.push(v);
                }
            }

            for (si, site) in live.iter().enumerate() {
                let cluster = &plan.clusters[site.cluster];
                let lead = pick_lead(&ix, &site.members);
                let cluster_lead = cluster_members[site.cluster][0];
                let primary_site = live.iter().find(|s| s.primary).unwrap_or(&live[0]);
                let ns = ns_default.clone();
                let zones: Vec<Value> = {
                    let mut seen = BTreeSet::new();
                    site.members.iter().filter_map(|&m| arch.nodes[m].zone_id.clone()).filter(|z| seen.insert(z.clone())).map(|z| json!(z)).collect()
                };
                let base = json!({
                    "arch": {"name": plan.arch_name, "slug": plan.arch_slug, "description": arch.meta.description},
                    "req": serde_json::to_value(&arch.requirements).unwrap_or(Value::Null),
                    "k8s": serde_json::to_value(k8s).map(|mut v| { if let Some(o) = v.as_object_mut() { o.remove("operators"); } v }).unwrap_or(Value::Null),
                    "lang": lang,
                    "cluster": {"nodes": cluster.nodes, "name": cluster.name, "slug": cluster.slug, "zones": cluster.zones, "props": props_json(&ix, cluster_lead)},
                    "unit": {
                        "key": key, "name": display, "slug": wl_slug, "kind": unit.kind, "behavior": format!("{:?}", unit.behavior).to_lowercase(),
                        "traits": unit.traits, "multiSite": live.len() > 1, "sites": site_json,
                        "members": all_members.iter().map(|&m| member_json(&ix, m)).collect::<Vec<_>>(),
                    },
                    "site": {
                        "index": si, "cluster": cluster.slug, "role": if site.primary { "primary" } else { "replica" }, "instances": site.instances,
                        "zones": zones, "members": site.members.iter().map(|&m| member_json(&ix, m)).collect::<Vec<_>>(),
                    },
                    "primary": {"cluster": plan.clusters[primary_site.cluster].slug, "namespace": ns, "name": wl_slug},
                    "p": props_json(&ix, lead),
                    "ns": ns,
                    "name": wl_slug,
                    "replication": {"edges": repl, "syncLocal": sync_local, "syncCross": sync_cross, "asyncLocal": async_local, "asyncCross": async_cross, "syncBlock": sync_block},
                    "backup": {"enabled": !backups.is_empty(), "targets": backups, "target": backups.first().cloned()},
                    "has": has,
                });
                let mut ctx_json = base;
                // labels may use placeholders of the context itself
                let ctx0 = to_mv(&ctx_json);
                let mut labels = Map::new();
                for (k, v) in &labels_tpl {
                    match dsl.text(v, &ctx0, &format!("export.k8s.labels.{k}")) {
                        Ok(s) => {
                            labels.insert(k.clone(), json!(s));
                        }
                        Err(e) => plan.report.notes.push(Note { level: "warn".into(), unit: None, message: e.to_string() }),
                    }
                }
                labels.insert("infra-architect/node".into(), json!(slug(&arch.nodes[lead].id)));
                ctx_json["labels"] = Value::Object(labels);

                // named expressions first: refusals and conditions may use them
                let ctx_json = match apply_vars(&dsl, tpl, ctx_json) {
                    Ok(c) => c,
                    Err(e) => {
                        let msg = tx.get("iac.skip.template_error", &[("kind", json!(kind_name)), ("error", json!(e.to_string()))]);
                        skip(&mut plan, "template_error", msg, vec![], site.members.iter().map(|&m| arch.nodes[m].id.clone()).collect());
                        continue;
                    }
                };

                // situations the template cannot express faithfully
                match refusal(&dsl, tpl, &ctx_json, lang) {
                    Ok(Some(reason)) => {
                        skip(&mut plan, "unsupported", reason, vec![], site.members.iter().map(|&m| arch.nodes[m].id.clone()).collect());
                        continue;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let msg = tx.get("iac.skip.template_error", &[("kind", json!(kind_name)), ("error", json!(e.to_string()))]);
                        skip(&mut plan, "template_error", msg, vec![], site.members.iter().map(|&m| arch.nodes[m].id.clone()).collect());
                        continue;
                    }
                }

                // required inputs
                let missing: Vec<String> = tpl.needs.iter().filter(|n| ix.eff[lead].map.get(&n.prop).map(|v| v.is_null() || v.as_str() == Some("")).unwrap_or(true)).map(|n| n.prop.clone()).collect();
                if !missing.is_empty() {
                    let hints: Vec<String> = tpl.needs.iter().filter(|n| missing.contains(&n.prop)).filter_map(|n| n.hint.as_ref().map(|h| h.get(lang))).collect();
                    let msg = tx.get("iac.skip.needs_input", &[("kind", json!(kind_name)), ("props", json!(missing.join(", "))), ("hint", json!(hints.join(" ")))]);
                    skip(&mut plan, "needs_input", msg, missing, site.members.iter().map(|&m| arch.nodes[m].id.clone()).collect());
                    continue;
                }

                match render_site(&dsl, tpl, &ctx_json, lang) {
                    Err(e) => {
                        let msg = tx.get("iac.skip.template_error", &[("kind", json!(kind_name)), ("error", json!(e.to_string()))]);
                        skip(&mut plan, "template_error", msg, vec![], site.members.iter().map(|&m| arch.nodes[m].id.clone()).collect());
                    }
                    Ok(r) => {
                        let mut items = vec![];
                        for o in r.objects {
                            let (api_version, kind, name, namespace) = identity(&o.body);
                            let base_name = format!("{}-{}", wl_slug, slug(&o.id));
                            let file = format!("{}.yaml", unique(&mut file_names[site.cluster], &base_name));
                            items.push(format!("{} {} {}", api_version, kind, name));
                            if let Some(ns) = &namespace {
                                ns_used.insert((site.cluster, ns.clone()));
                            }
                            plan.objects.push(PlannedObject { cluster: site.cluster, unit: unit.key.clone(), requires: r.requires.clone(), api_version, kind, name, namespace, body: o.body, file });
                        }
                        for rel in r.releases {
                            items.push(format!("helm {}/{} {}", rel.repo, rel.chart, rel.version.as_deref().unwrap_or("")).trim().to_string());
                            let name = unique(&mut release_names[site.cluster], &slug(&rel.name));
                            ns_used.insert((site.cluster, rel.namespace.clone()));
                            plan.releases.push(PlannedRelease {
                                cluster: site.cluster,
                                unit: Some(unit.key.clone()),
                                requires: r.requires.clone(),
                                name,
                                namespace: rel.namespace,
                                chart: rel.chart,
                                repo: rel.repo,
                                version: rel.version,
                                values: rel.values,
                                operator: false,
                                op_id: None,
                                needs: vec![],
                            });
                        }
                        for op in r.requires {
                            operators_needed[site.cluster].insert(op);
                        }
                        for s in r.secrets {
                            plan.report.secrets.push(SecretNeed {
                                cluster: cluster.slug.clone(),
                                namespace: s.namespace.unwrap_or_else(|| ns.clone()),
                                name: s.name,
                                keys: s.keys,
                                unit: unit.key.clone(),
                                hint: s.hint,
                            });
                        }
                        for n in r.notes {
                            plan.report.notes.push(Note { level: n.0, unit: Some(unit.key.clone()), message: n.1 });
                        }
                        plan.report.generated.push(Generated {
                            unit: unit.key.clone(),
                            name: display.clone(),
                            kind: unit.kind.clone(),
                            cluster: cluster.slug.clone(),
                            nodes: site.members.iter().map(|&m| arch.nodes[m].id.clone()).collect(),
                            items,
                        });
                    }
                }
            }
            let _ = u;
        }
    }

    // operators: pinned releases, ordered by `after`
    let mut operator_releases: Vec<PlannedRelease> = vec![];
    for (c, needed) in operators_needed.iter().enumerate() {
        let order = operator_order(&k8s.operators, needed, &mut plan.report.notes, &tx);
        let cluster_slug = plan.clusters[c].slug.clone();
        for id in &order {
            let Some(op) = k8s.operators.get(id) else { continue };
            let release = if op.release.is_empty() { slug(id) } else { op.release.clone() };
            let ns = if op.namespace.is_empty() { release.clone() } else { op.namespace.clone() };
            ns_used.insert((c, ns.clone()));
            let needs: Vec<String> = op
                .after
                .iter()
                .filter(|a| order.contains(a))
                .map(|a| k8s.operators.get(a).map(|o| if o.release.is_empty() { slug(a) } else { o.release.clone() }).unwrap_or_else(|| slug(a)))
                .collect();
            plan.report.prerequisites.push(Prerequisite {
                cluster: cluster_slug.clone(),
                id: id.clone(),
                title: op.title.get(lang),
                release: release.clone(),
                namespace: ns.clone(),
                chart: op.chart.clone(),
                repo: op.repo.clone(),
                version: op.version.clone(),
            });
            operator_releases.push(PlannedRelease {
                cluster: c,
                unit: None,
                requires: vec![],
                name: release,
                namespace: ns,
                chart: op.chart.clone(),
                repo: op.repo.clone(),
                version: if op.version.is_empty() { None } else { Some(op.version.clone()) },
                values: op.values.clone(),
                operator: true,
                op_id: Some(id.clone()),
                needs,
            });
        }
    }
    // prerequisites first, workload charts after; a chart waits only for the operators it requires
    let release_of = |id: &str| -> String {
        let op = k8s.operators.get(id);
        match op {
            Some(o) if !o.release.is_empty() => o.release.clone(),
            _ => slug(id),
        }
    };
    for r in plan.releases.iter_mut() {
        r.needs = r.requires.iter().filter(|id| k8s.operators.contains_key(id.as_str())).map(|id| release_of(id)).collect();
    }
    operator_releases.append(&mut plan.releases);
    plan.releases = operator_releases;

    if k8s.create_namespaces {
        plan.namespaces = ns_used.into_iter().filter(|(_, ns)| !ns.is_empty()).collect();
    }
    // the same remark produced for several sites of one service is shown once
    let mut seen = HashSet::new();
    plan.report.notes.retain(|n| seen.insert((n.level.clone(), n.unit.clone(), n.message.clone())));
    plan
}

fn operator_order(ops: &BTreeMap<String, Operator>, needed: &BTreeSet<String>, notes: &mut Vec<Note>, tx: &Texts<'_>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    fn visit(id: &str, ops: &BTreeMap<String, Operator>, out: &mut Vec<String>, stack: &mut Vec<String>, notes: &mut Vec<Note>, tx: &Texts<'_>) {
        if out.iter().any(|x| x == id) || stack.iter().any(|x| x == id) {
            return;
        }
        let Some(op) = ops.get(id) else {
            notes.push(Note { level: "warn".into(), unit: None, message: tx.get("iac.note.unknown_operator", &[("id", json!(id))]) });
            return;
        };
        stack.push(id.to_string());
        for a in &op.after {
            visit(a, ops, out, stack, notes, tx);
        }
        stack.pop();
        out.push(id.to_string());
    }
    let mut stack = vec![];
    for id in needed {
        visit(id, ops, &mut out, &mut stack, notes, tx);
    }
    out
}

fn identity(body: &Value) -> (String, String, String, Option<String>) {
    let s = |p: &[&str]| -> String {
        let mut cur = body;
        for k in p {
            cur = cur.get(*k).unwrap_or(&Value::Null);
        }
        cur.as_str().unwrap_or("").to_string()
    };
    let ns = s(&["metadata", "namespace"]);
    (s(&["apiVersion"]), s(&["kind"]), s(&["metadata", "name"]), if ns.is_empty() { None } else { Some(ns) })
}

struct SiteOutput {
    requires: Vec<String>,
    objects: Vec<RenderedObject>,
    releases: Vec<RenderedRelease>,
    secrets: Vec<RenderedSecret>,
    notes: Vec<(String, String)>,
}
struct RenderedObject {
    id: String,
    body: Value,
}
struct RenderedRelease {
    name: String,
    namespace: String,
    chart: String,
    repo: String,
    version: Option<String>,
    values: Value,
}
struct RenderedSecret {
    name: String,
    namespace: Option<String>,
    keys: Vec<String>,
    hint: Option<String>,
}

/// Evaluate the template's named expressions in order and add them to the context.
fn apply_vars(dsl: &Dsl, tpl: &IacTemplate, mut ctx: Value) -> Result<Value, DslError> {
    let mut mv = to_mv(&ctx);
    for (i, v) in tpl.vars.iter().enumerate() {
        let path = format!("vars[{i}].{}", v.name);
        let val = dsl.eval(&v.expr, &mv).map_err(|m| DslError { path: path.clone(), message: m })?;
        if val.is_undefined() {
            return Err(DslError { path, message: "is undefined".into() });
        }
        ctx[&v.name] = serde_json::to_value(&val).unwrap_or(Value::Null);
        mv = to_mv(&ctx);
    }
    Ok(ctx)
}

/// `ctx` already contains the evaluated `vars`.
fn render_site(dsl: &Dsl, tpl: &IacTemplate, ctx: &Value, lang: &str) -> Result<SiteOutput, DslError> {
    let mv = to_mv(ctx);
    let when = |w: &Option<String>, path: &str| -> Result<bool, DslError> {
        match w {
            Some(e) => dsl.truthy(e, &mv, path),
            None => Ok(true),
        }
    };
    let mut out = SiteOutput { requires: vec![], objects: vec![], releases: vec![], secrets: vec![], notes: vec![] };
    for (i, r) in tpl.requires.iter().enumerate() {
        if when(&r.when().map(|w| w.to_string()), &format!("requires[{i}].when"))? {
            out.requires.push(r.id().to_string());
        }
    }
    for (i, o) in tpl.objects.iter().enumerate() {
        if !when(&o.when, &format!("objects[{i}].when"))? {
            continue;
        }
        let body = dsl.render(&o.body, &mv).map_err(|e| DslError { path: format!("objects[{i}].body{}{}", if e.path.is_empty() { "" } else { "." }, e.path), message: e.message })?;
        if let Some(body) = body {
            for (k, what) in [("apiVersion", "apiVersion"), ("kind", "kind")] {
                if body.get(k).and_then(|v| v.as_str()).unwrap_or("").is_empty() {
                    return Err(DslError { path: format!("objects[{i}].body"), message: format!("{what} is missing") });
                }
            }
            if body.pointer("/metadata/name").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
                return Err(DslError { path: format!("objects[{i}].body.metadata.name"), message: "name is missing".into() });
            }
            out.objects.push(RenderedObject { id: o.id.clone(), body: with_labels(body, ctx) });
        }
    }
    for (i, r) in tpl.releases.iter().enumerate() {
        if !when(&r.when, &format!("releases[{i}].when"))? {
            continue;
        }
        let p = |f: &str| format!("releases[{i}].{f}");
        let name = dsl.text(&r.name, &mv, &p("name"))?;
        let namespace = dsl.text(&r.namespace, &mv, &p("namespace"))?;
        let chart = dsl.text(&r.chart, &mv, &p("chart"))?;
        let repo = dsl.text(&r.repo, &mv, &p("repo"))?;
        let version = match &r.version {
            Some(v) => Some(dsl.text(v, &mv, &p("version"))?).filter(|s| !s.is_empty()),
            None => None,
        };
        let values = dsl
            .render(&r.values, &mv)
            .map_err(|e| DslError { path: format!("releases[{i}].values{}{}", if e.path.is_empty() { "" } else { "." }, e.path), message: e.message })?
            .unwrap_or(Value::Object(Map::new()));
        out.releases.push(RenderedRelease { name, namespace, chart, repo, version, values });
    }
    for (i, s) in tpl.secrets.iter().enumerate() {
        if !when(&s.when, &format!("secrets[{i}].when"))? {
            continue;
        }
        let name = dsl.text(&s.name, &mv, &format!("secrets[{i}].name"))?;
        let namespace = match &s.namespace {
            Some(n) => Some(dsl.text(n, &mv, &format!("secrets[{i}].namespace"))?),
            None => None,
        };
        let hint = match &s.hint {
            Some(h) => Some(dsl.text(&h.get(lang), &mv, &format!("secrets[{i}].hint"))?),
            None => None,
        };
        out.secrets.push(RenderedSecret { name, namespace, keys: s.keys.clone(), hint });
    }
    for (i, n) in tpl.notes.iter().enumerate() {
        if !when(&n.when, &format!("notes[{i}].when"))? {
            continue;
        }
        let text = dsl.text(&n.text.get(lang), &mv, &format!("notes[{i}].text"))?;
        out.notes.push((if n.level == "warn" { "warn".into() } else { "info".into() }, text));
    }
    Ok(out)
}

/// The first `unsupported` condition that holds, with its localised, rendered reason.
fn refusal(dsl: &Dsl, tpl: &IacTemplate, ctx_json: &Value, lang: &str) -> Result<Option<String>, DslError> {
    let mv = to_mv(ctx_json);
    for (i, r) in tpl.unsupported.iter().enumerate() {
        let hit = match &r.when {
            Some(w) => dsl.truthy(w, &mv, &format!("unsupported[{i}].when"))?,
            None => true,
        };
        if hit {
            return Ok(Some(dsl.text(&r.text.get(lang), &mv, &format!("unsupported[{i}].text"))?));
        }
    }
    Ok(None)
}

/// Common labels are merged into `metadata.labels` unless the template already sets the key.
fn with_labels(mut body: Value, ctx: &Value) -> Value {
    let Some(labels) = ctx.get("labels").and_then(|l| l.as_object()) else { return body };
    if labels.is_empty() {
        return body;
    }
    if let Some(meta) = body.get_mut("metadata").and_then(|m| m.as_object_mut()) {
        let entry = meta.entry("labels").or_insert_with(|| Value::Object(Map::new()));
        if let Some(l) = entry.as_object_mut() {
            for (k, v) in labels {
                l.entry(k.clone()).or_insert(v.clone());
            }
        }
    }
    body
}

// the pieces the emitters share
impl Plan {
    pub fn cluster_objects(&self, c: usize) -> impl Iterator<Item = &PlannedObject> {
        self.objects.iter().filter(move |o| o.cluster == c)
    }
    pub fn cluster_releases(&self, c: usize) -> impl Iterator<Item = &PlannedRelease> {
        self.releases.iter().filter(move |r| r.cluster == c)
    }
    /// Every namespace the cluster needs (objects, charts and operators).
    pub fn cluster_namespaces(&self, c: usize) -> Vec<String> {
        self.namespaces.iter().filter(|(cl, _)| *cl == c).map(|(_, n)| n.clone()).collect()
    }
    /// Namespaces that objects live in (the others are created together with their chart).
    pub fn object_namespaces(&self, c: usize) -> Vec<String> {
        let used: BTreeSet<&str> = self.cluster_objects(c).filter_map(|o| o.namespace.as_deref()).collect();
        self.cluster_namespaces(c).into_iter().filter(|n| used.contains(n.as_str())).collect()
    }
}
