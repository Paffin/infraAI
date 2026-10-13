//! Check implementations. A [`Rule`](crate::config::Rule) binds an id, texts, severity and
//! parameters to one of these generic checks; checks reference **traits and behaviors**, never
//! concrete product names. Each violation carries a stable key (for waivers), the subjects to
//! highlight, template variables and – where a safe mechanical remedy exists – ready-to-apply
//! patch operations.

use crate::catalog::{Behavior, Semantics, ZoneLevel};
use crate::config::{CheckType, Rule, Severity};
use crate::cost;
use crate::evaluate::{Ctx, Fix, Need, Status, Subjects};
use crate::graph::Authority;
use crate::model::{as_bool, as_f64, as_string, unique_id, Edge, Node, Props, PropsExt};
use crate::ops::Op;
use crate::sim::{RecoveryKind, Scenario, ServiceState};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub struct Raw {
    pub key: String,
    pub status: Status,
    pub subjects: Subjects,
    pub vars: Map<String, Value>,
    pub fixes: Vec<Fix>,
    pub needs: Vec<Need>,
    pub scenario: Option<Scenario>,
}

impl Raw {
    fn new(key: impl Into<String>, status: Status) -> Self {
        Raw { key: key.into(), status, subjects: Subjects::default(), vars: Map::new(), fixes: vec![], needs: vec![], scenario: None }
    }
    /// A value that would make this (no_data) finding verifiable.
    fn need(mut self, target: &str, id: Option<&str>, prop: &str) -> Self {
        let n = Need { target: target.to_string(), id: id.map(String::from), prop: prop.to_string() };
        if !self.needs.contains(&n) {
            self.needs.push(n);
        }
        self
    }
    fn var(mut self, k: &str, v: impl Into<Value>) -> Self {
        self.vars.insert(k.to_string(), v.into());
        self
    }
    fn node(mut self, id: &str) -> Self {
        if !self.subjects.nodes.iter().any(|n| n == id) {
            self.subjects.nodes.push(id.to_string());
        }
        self
    }
    fn zone(mut self, id: &str) -> Self {
        if !self.subjects.zones.iter().any(|n| n == id) {
            self.subjects.zones.push(id.to_string());
        }
        self
    }
    fn edge(mut self, id: &str) -> Self {
        if !self.subjects.edges.iter().any(|n| n == id) {
            self.subjects.edges.push(id.to_string());
        }
        self
    }
    fn fix(mut self, f: Option<Fix>) -> Self {
        if let Some(f) = f {
            self.fixes.push(f);
        }
        self
    }
    fn scenario(mut self, sc: Scenario) -> Self {
        self.scenario = Some(sc);
        self
    }
}

pub struct CheckOut {
    pub applicable: bool,
    pub findings: Vec<Raw>,
}

impl CheckOut {
    fn na() -> Self {
        CheckOut { applicable: false, findings: vec![] }
    }
    fn found(findings: Vec<Raw>) -> Self {
        CheckOut { applicable: true, findings }
    }
}

// ---------------------------------------------------------------------------------------------
// parameter helpers
// ---------------------------------------------------------------------------------------------

fn p<'r>(rule: &'r Rule, key: &str) -> Option<&'r Value> {
    rule.params.get(key)
}
fn p_str(rule: &Rule, key: &str) -> Option<String> {
    p(rule, key).and_then(as_string)
}
fn p_f64(rule: &Rule, key: &str) -> Option<f64> {
    p(rule, key).and_then(as_f64)
}
fn p_bool(rule: &Rule, key: &str) -> Option<bool> {
    p(rule, key).and_then(as_bool)
}
fn p_vec(rule: &Rule, key: &str) -> Vec<String> {
    p(rule, key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(as_string).collect()).unwrap_or_default()
}
fn sev(rule: &Rule) -> Status {
    match rule.severity {
        Severity::Critical => Status::Critical,
        Severity::Risk => Status::Risk,
    }
}

pub fn tolerance(ctx: &Ctx<'_>) -> String {
    ctx.ix.arch.requirements.failure_tolerance.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| ctx.cfg.settings.assumptions.default_failure_tolerance.clone())
}

fn tol_rank(ctx: &Ctx<'_>, id: &str) -> u32 {
    ctx.cfg.settings.tolerance.rank(id)
}

/// Does the required tolerance include surviving the loss of a failure domain of `level`?
fn tol_covers(ctx: &Ctx<'_>, level: ZoneLevel) -> bool {
    ctx.cfg.settings.tolerance.covers(&tolerance(ctx), level)
}

/// How many sites may be lost at the same time (`requirements.siteFailures`), within the configured bound.
pub fn site_failures(ctx: &Ctx<'_>) -> usize {
    let max = ctx.cfg.settings.scenarios.max_site_failures.max(1);
    (ctx.ix.arch.requirements.site_failures.unwrap_or(1) as usize).clamp(1, max)
}

/// Failure-domain level a rule works on (`domain` parameter, `site` unless stated).
fn domain_level(rule: &Rule) -> ZoneLevel {
    p_str(rule, "domain").and_then(|d| serde_json::from_value(Value::String(d)).ok()).unwrap_or(ZoneLevel::Site)
}

/// Is failure-domain `level` part of what the architecture is verified against? The required tolerance has to
/// reach it. A level below the one asked for (racks when sites are required) only counts once the model actually
/// has such zones – nobody asked for racks, so a design without any is not at fault.
fn domain_applies(ctx: &Ctx<'_>, level: ZoneLevel) -> bool {
    if !tol_covers(ctx, level) {
        return false;
    }
    let tol = &ctx.cfg.settings.tolerance;
    let asked = tol.rank(&tolerance(ctx));
    match level {
        ZoneLevel::Rack => !ctx.ix.rack_zones().is_empty() || tol.rank_of_domain(level) == Some(asked),
        _ => true,
    }
}

/// Zone scenarios for every failure domain of `level` that hosts at least one node – empty if the required
/// tolerance does not reach that level.
pub fn domain_scenarios(ctx: &Ctx<'_>, level: ZoneLevel) -> Vec<Scenario> {
    if !domain_applies(ctx, level) {
        return vec![];
    }
    let zones: Vec<usize> = match level {
        ZoneLevel::Rack => ctx.ix.rack_zones(),
        ZoneLevel::Site => ctx.ix.sites(),
        _ => vec![],
    };
    zones.into_iter().map(|s| Scenario::Zone { zone_id: ctx.ix.arch.zones[s].id.clone() }).collect()
}

/// Every single-domain failure the required tolerance asks for: racks, then sites (empty if only instance failures
/// have to be survived).
pub fn site_scenarios(ctx: &Ctx<'_>) -> Vec<Scenario> {
    let mut out = domain_scenarios(ctx, ZoneLevel::Rack);
    for sc in domain_scenarios(ctx, ZoneLevel::Site) {
        if !out.contains(&sc) {
            out.push(sc);
        }
    }
    out
}

/// Partition scenarios of the standard set: cut links, isolated sites or both (`scope` of the rule). Only as many
/// sites are isolated together as the requirement lets fail at once.
fn link_scenarios(ctx: &Ctx<'_>, scope: &str) -> Vec<Scenario> {
    let k = site_failures(ctx);
    crate::sim::standard_scenarios(&ctx.ix)
        .into_iter()
        .filter(|s| match s {
            Scenario::Link { .. } => scope != "isolate",
            Scenario::Isolate { zone_ids } => scope != "link" && zone_ids.len() <= k,
            _ => false,
        })
        .collect()
}

/// One scenario per disaster domain: the multi-site `Geo` scenario, or – for a domain made of a
/// single site – the plain zone scenario.
fn geo_scenarios(ctx: &Ctx<'_>) -> Vec<(String, Scenario)> {
    let ix = &ctx.ix;
    let mut geos: Vec<String> = ix.geo_of.iter().flatten().cloned().collect();
    geos.sort();
    geos.dedup();
    geos.into_iter()
        .map(|g| {
            let mut ss: Vec<usize> = (0..ix.arch.nodes.len()).filter(|&n| ix.geo_of[n].as_deref() == Some(g.as_str())).filter_map(|n| ix.site_of[n]).collect();
            ss.sort_unstable();
            ss.dedup();
            if ss.len() == 1 {
                (g, Scenario::Zone { zone_id: ix.arch.zones[ss[0]].id.clone() })
            } else {
                (g.clone(), Scenario::Geo { geo: g })
            }
        })
        .collect()
}

fn zone_name<'c>(ctx: &'c Ctx<'_>, zi: usize) -> &'c str {
    ctx.ix.zone_name(zi)
}

fn unit_matches(ctx: &Ctx<'_>, rule: &Rule, u: usize) -> bool {
    let unit = &ctx.ix.units[u];
    if let Some(t) = p_str(rule, "trait") {
        if !unit.traits.contains(&t) {
            return false;
        }
    }
    let any = p_vec(rule, "anyTrait");
    if !any.is_empty() && !any.iter().any(|t| unit.traits.contains(t)) {
        return false;
    }
    if let Some(b) = p_str(rule, "behavior") {
        let want: Option<Behavior> = serde_json::from_value(Value::String(b)).ok();
        if want != Some(unit.behavior) {
            return false;
        }
    }
    if let Some(k) = p_str(rule, "kind") {
        if unit.kind != k {
            return false;
        }
    }
    for t in p_vec(rule, "excludeTraits") {
        if unit.traits.contains(&t) {
            return false;
        }
    }
    true
}

fn data_members(ctx: &Ctx<'_>, u: usize) -> Vec<usize> {
    ctx.ix.units[u].members.iter().copied().filter(|&m| !ctx.ix.is_witness(m)).collect()
}

fn primary_of(ctx: &Ctx<'_>, u: usize) -> usize {
    let ms = &ctx.ix.units[u].members;
    ms.iter().copied().find(|&m| ctx.ix.arch.nodes[m].role.as_deref() == Some("primary")).or_else(|| ms.iter().copied().find(|&m| !ctx.ix.is_witness(m))).unwrap_or(ms[0])
}

fn nid(ctx: &Ctx<'_>, ni: usize) -> String {
    ctx.ix.arch.nodes[ni].id.clone()
}

// ---------------------------------------------------------------------------------------------
// fix builders
// ---------------------------------------------------------------------------------------------

fn fix(kind: &str, key: &str, vars: Value, ops: Vec<Op>) -> Fix {
    Fix { id: format!("{}:{}", kind, key), kind: kind.to_string(), vars: vars.as_object().cloned().unwrap_or_default(), ops }
}

fn update_node(id: &str, kv: Vec<(&str, Value)>) -> Op {
    let mut set = Map::new();
    for (k, v) in kv {
        set.insert(k.to_string(), v);
    }
    Op::UpdateNode { id: id.to_string(), set }
}

fn set_prop_fix(ctx: &Ctx<'_>, ni: usize, key: &str, value: Value, kind: &str, vars: Value) -> Fix {
    let n = &ctx.ix.arch.nodes[ni];
    let mut v = vars.as_object().cloned().unwrap_or_default();
    v.insert("node".into(), json!(n.name));
    v.insert("key".into(), json!(key));
    v.insert("value".into(), value.clone());
    Fix { id: format!("{}:{}:{}", kind, n.id, key), kind: kind.to_string(), vars: v, ops: vec![update_node(&n.id, vec![(&format!("props.{}", key), value)])] }
}

fn used_ids(ctx: &Ctx<'_>, extra: &[String]) -> HashSet<String> {
    let mut s = ctx.ix.arch.all_ids();
    for e in extra {
        s.insert(e.clone());
    }
    s
}

/// Site-level zones that do not host any member of `u`, best candidates first (other geo first).
fn candidate_sites(ctx: &Ctx<'_>, u: usize) -> Vec<usize> {
    let ix = &ctx.ix;
    let used: Vec<usize> = ix.unit_sites(u);
    let used_geo: BTreeSet<String> = ix.units[u].members.iter().filter_map(|&m| ix.geo_of[m].clone()).collect();
    let mut c: Vec<usize> = (0..ix.arch.zones.len()).filter(|&zi| ix.zone_level(zi) == ZoneLevel::Site && !used.contains(&zi)).collect();
    let geo_of_zone = |zi: usize| -> Option<String> {
        ix.arch.zones[zi].props.get("geo").and_then(as_string).or_else(|| ix.arch.nodes.iter().enumerate().find(|(ni, _)| ix.site_of[*ni] == Some(zi)).and_then(|(ni, _)| ix.geo_of[ni].clone()))
    };
    c.sort_by_key(|&zi| {
        let g = geo_of_zone(zi);
        let different = g.map(|g| !used_geo.contains(&g)).unwrap_or(false);
        (!different, zi)
    });
    c
}

/// Zones of `level` that host no member of `u`, best candidates first. Sites: another disaster domain first. Racks:
/// the sites the unit already lives in first (cheaper than a new site).
fn candidate_domains(ctx: &Ctx<'_>, u: usize, level: ZoneLevel) -> Vec<usize> {
    if level == ZoneLevel::Site {
        return candidate_sites(ctx, u);
    }
    let ix = &ctx.ix;
    let used = ix.unit_domains(u, level);
    let used_sites = ix.unit_sites(u);
    let mut c: Vec<usize> = (0..ix.arch.zones.len()).filter(|&zi| ix.zone_level(zi) == level && !used.contains(&zi)).collect();
    c.sort_by_key(|&zi| (!used_sites.contains(&ix.zone_site(zi)), zi));
    c
}

/// Add one more member of the unit in another site (replica for stateful, clone for stateless). Without another
/// site the member goes to another rack when the required tolerance reaches racks.
fn add_member_fix(ctx: &Ctx<'_>, u: usize) -> Option<Fix> {
    add_member_fix_in(ctx, u, ZoneLevel::Site)
}

fn add_member_fix_in(ctx: &Ctx<'_>, u: usize, level: ZoneLevel) -> Option<Fix> {
    let ix = &ctx.ix;
    let target = match candidate_domains(ctx, u, level).first() {
        Some(&t) => t,
        None if level == ZoneLevel::Site && tol_covers(ctx, ZoneLevel::Rack) => *candidate_domains(ctx, u, ZoneLevel::Rack).first()?,
        None => return None,
    };
    let t = primary_of(ctx, u);
    let tn: &Node = &ix.arch.nodes[t];
    let tz = zone_name(ctx, target).to_string();
    let mut ids = used_ids(ctx, &[]);
    let new_id = unique_id(&format!("{}-{}", tn.id, ix.arch.zones[target].id), &ids);
    ids.insert(new_id.clone());
    let stateful = ix.units[u].behavior == Behavior::PrimaryReplica;
    let group = tn.group.clone().unwrap_or_else(|| format!("{}-cluster", tn.id));
    let mut ops: Vec<Op> = Vec::new();
    if tn.group.is_none() {
        ops.push(update_node(&tn.id, vec![("group", json!(group))]));
        if stateful && tn.role.is_none() {
            ops.push(update_node(&tn.id, vec![("role", json!("primary"))]));
        }
    }
    let node = Node {
        id: new_id.clone(),
        kind: tn.kind.clone(),
        name: format!("{} · {}", ix.units[u].name, tz),
        zone_id: Some(ix.arch.zones[target].id.clone()),
        group: Some(group),
        role: if stateful { Some("replica".into()) } else { tn.role.clone() },
        props: tn.props.clone(),
        ui: None,
        proposed: true,
    };
    ops.push(Op::AddNode { node });
    if stateful {
        let rpo0 = ix.arch.requirements.rpo_seconds == Some(0.0);
        let sync_ok = false; // never assume a measured RTT: default to async unless the architect upgrades it
        let etype = if rpo0 && sync_ok { "sync" } else { "async" };
        let eid = unique_id(&format!("e-{}--{}", tn.id, new_id), &ids);
        ops.push(Op::AddEdge { edge: Edge { id: eid, source: tn.id.clone(), target: new_id.clone(), edge_type: etype.into(), label: None, props: Props::new() } });
    } else {
        // stateless clone: mirror dependency/control edges of the template member
        for &ei in ix.in_edges[t].iter().chain(ix.out_edges[t].iter()) {
            let e = &ix.arch.edges[ei];
            if !matches!(ix.sem(ei), Some(Semantics::Dependency) | Some(Semantics::Control)) {
                continue;
            }
            let (s, tg) = if e.source == tn.id { (new_id.clone(), e.target.clone()) } else { (e.source.clone(), new_id.clone()) };
            let eid = unique_id(&format!("e-{}--{}", s, tg), &ids);
            ids.insert(eid.clone());
            ops.push(Op::AddEdge { edge: Edge { id: eid, source: s, target: tg, edge_type: e.edge_type.clone(), label: e.label.clone(), props: e.props.clone() } });
        }
    }
    Some(fix("add_member", &ix.units[u].key, json!({"unit": ix.units[u].name, "zone": tz}), ops))
}

/// Add an arbiter/voter of the unit in a site that hosts none of its members.
fn add_witness_fix(ctx: &Ctx<'_>, u: usize) -> Option<Fix> {
    add_witness_fix_in(ctx, u, ZoneLevel::Site)
}

fn add_witness_fix_in(ctx: &Ctx<'_>, u: usize, level: ZoneLevel) -> Option<Fix> {
    let ix = &ctx.ix;
    let target = *candidate_domains(ctx, u, level).first()?;
    let t = primary_of(ctx, u);
    let tn = &ix.arch.nodes[t];
    let tz = zone_name(ctx, target).to_string();
    let mut ids = used_ids(ctx, &[]);
    let new_id = unique_id(&format!("{}-witness", ix.units[u].key), &ids);
    ids.insert(new_id.clone());
    let group = tn.group.clone().unwrap_or_else(|| format!("{}-cluster", tn.id));
    let mut ops = Vec::new();
    if tn.group.is_none() {
        ops.push(update_node(&tn.id, vec![("group", json!(group))]));
        if ix.units[u].behavior == Behavior::PrimaryReplica && tn.role.is_none() {
            ops.push(update_node(&tn.id, vec![("role", json!("primary"))]));
        }
    }
    let mut props = Props::new();
    props.insert("vcpu".into(), json!(1));
    props.insert("ramGb".into(), json!(1));
    ops.push(Op::AddNode {
        node: Node {
            id: new_id.clone(),
            kind: tn.kind.clone(),
            name: format!("{} witness", ix.units[u].name),
            zone_id: Some(ix.arch.zones[target].id.clone()),
            group: Some(group),
            role: Some("witness".into()),
            props,
            ui: None,
            proposed: true,
        },
    });
    let eid = unique_id(&format!("e-{}--{}", new_id, tn.id), &ids);
    ops.push(Op::AddEdge { edge: Edge { id: eid, source: new_id, target: tn.id.clone(), edge_type: "quorum".into(), label: None, props: Props::new() } });
    Some(fix("add_witness", &ix.units[u].key, json!({"unit": ix.units[u].name, "zone": tz}), ops))
}

fn add_backup_fix(ctx: &Ctx<'_>, u: usize) -> Option<Fix> {
    let ix = &ctx.ix;
    let src = primary_of(ctx, u);
    let sn = &ix.arch.nodes[src];
    let sites = candidate_sites(ctx, u);
    let mut ops = Vec::new();
    let mut ids = used_ids(ctx, &[]);
    // reuse an existing backup target that is outside the unit's sites
    let existing = (0..ix.arch.nodes.len()).find(|&ni| ix.has_trait(ni, "backup-target") && ix.site_of[ni].map(|s| !ix.unit_sites(u).contains(&s)).unwrap_or(true));
    let (target_id, zone_label) = if let Some(ni) = existing {
        (ix.arch.nodes[ni].id.clone(), ix.site_of[ni].map(|s| zone_name(ctx, s).to_string()).unwrap_or_default())
    } else {
        let kind = ix.cat.kinds.iter().find(|k| k.traits.iter().any(|t| t == "backup-target"))?;
        let site = *sites.first()?;
        let id = unique_id(&format!("backup-{}", ix.arch.zones[site].id), &ids);
        ids.insert(id.clone());
        let mut props = Props::new();
        props.insert("immutable".into(), json!(true));
        props.insert("retentionDays".into(), json!(30));
        props.insert("encryptionAtRest".into(), json!(true));
        let zl = zone_name(ctx, site).to_string();
        ops.push(Op::AddNode {
            node: Node {
                id: id.clone(),
                kind: kind.id.clone(),
                name: kind.name.get(&ctx.cfg.lang),
                zone_id: Some(ix.arch.zones[site].id.clone()),
                group: None,
                role: None,
                props,
                ui: None,
                proposed: true,
            },
        });
        (id, zl)
    };
    let eid = unique_id(&format!("e-{}--{}", sn.id, target_id), &ids);
    ops.push(Op::AddEdge { edge: Edge { id: eid, source: sn.id.clone(), target: target_id, edge_type: "backup".into(), label: None, props: Props::new() } });
    Some(fix("add_backup", &ix.units[u].key, json!({"unit": ix.units[u].name, "zone": zone_label}), ops))
}

fn add_required_fix(ctx: &Ctx<'_>, tr: &str) -> Option<Fix> {
    let ix = &ctx.ix;
    let kind = ix.cat.kinds.iter().find(|k| !k.hidden && k.traits.iter().any(|t| t == tr))?;
    let zone = (0..ix.arch.zones.len()).find(|&zi| ix.zone_level(zi) == ZoneLevel::Site);
    let ids = used_ids(ctx, &[]);
    let id = unique_id(&kind.id, &ids);
    let mut props = Props::new();
    if let Some(d) = ix.cat.default_props(&kind.id).get("replicas") {
        props.insert("replicas".into(), d.clone());
    }
    Some(fix(
        "add_required",
        tr,
        json!({"kind": kind.name.get(&ctx.cfg.lang), "zone": zone.map(|z| zone_name(ctx, z).to_string()).unwrap_or_default()}),
        vec![Op::AddNode {
            node: Node { id, kind: kind.id.clone(), name: kind.name.get(&ctx.cfg.lang), zone_id: zone.map(|z| ix.arch.zones[z].id.clone()), group: None, role: None, props, ui: None, proposed: true },
        }],
    ))
}

// ---------------------------------------------------------------------------------------------
// dispatch
// ---------------------------------------------------------------------------------------------

pub fn run(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    match rule.check {
        CheckType::Spof => spof(ctx, rule),
        CheckType::SurviveSite => survive_site(ctx, rule),
        CheckType::SurviveSites => survive_sites(ctx, rule),
        CheckType::VersionLifecycle => version_lifecycle(ctx, rule),
        CheckType::VersionSkew => version_skew(ctx, rule),
        CheckType::Rto => rto(ctx, rule),
        CheckType::Rpo => rpo(ctx, rule),
        CheckType::GeoSurvive => geo_survive(ctx, rule),
        CheckType::GeoSpread => geo_spread(ctx, rule),
        CheckType::PartitionSafety => partition_safety(ctx, rule),
        CheckType::MinInstances => min_instances(ctx, rule),
        CheckType::SpreadSites => spread_sites(ctx, rule),
        CheckType::QuorumVoters => quorum_voters(ctx, rule),
        CheckType::WitnessIndependent => witness_independent(ctx, rule),
        CheckType::ReplicationPolicy => replication_policy(ctx, rule),
        CheckType::FailoverAuthority => failover_authority(ctx, rule),
        CheckType::BackupExists => backup_exists(ctx, rule),
        CheckType::BackupOffsite => backup_offsite(ctx, rule),
        CheckType::Prop => prop_check(ctx, rule),
        CheckType::Capacity => capacity(ctx, rule),
        CheckType::CapacityUnderFailure => capacity_under_failure(ctx, rule),
        CheckType::CostBudget => cost_budget(ctx, rule),
        CheckType::PathContains => path_contains(ctx, rule),
        CheckType::ForbiddenEdge => forbidden_edge(ctx, rule),
        CheckType::NoCycles => no_cycles(ctx, rule),
        CheckType::RequiredKind => required_kind(ctx, rule),
        CheckType::CacheResilience => cache_resilience(ctx, rule),
        CheckType::GroupHygiene => group_hygiene(ctx, rule),
        CheckType::RoleRequired => role_required(ctx, rule),
    }
}

// ---------------------------------------------------------------------------------------------
// availability & disaster recovery (scenario based)
// ---------------------------------------------------------------------------------------------

fn spof(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    if ix.arch.nodes.is_empty() || !ix.arch.nodes.iter().enumerate().any(|(i, _)| ix.has_trait(i, "entry")) && ix.arch.edges.is_empty() {
        return CheckOut::na();
    }
    let ignore = p_vec(rule, "ignoreTraits");
    let mut out = vec![];
    for (ni, n) in ix.arch.nodes.iter().enumerate() {
        if ignore.iter().any(|t| ix.has_trait(ni, t)) || ix.has_trait(ni, "entry") {
            continue;
        }
        let sc = Scenario::Instance { node_id: n.id.clone(), count: 1 };
        let r = ctx.sim(&sc);
        if r.state == ServiceState::Down && r.recovery.kind != RecoveryKind::Manual {
            let u = ix.unit_of[ni];
            let fixf = if ix.units[u].members.len() == 1 && ix.units[u].behavior == Behavior::Stateless {
                let cur = ix.replicas(ni);
                Some(set_prop_fix(ctx, ni, "replicas", json!(cur.max(1) + 1), "set_replicas", json!({"replicas": cur.max(1) + 1})))
            } else {
                add_member_fix(ctx, u)
            };
            out.push(Raw::new(&n.id, status_for(rule)).var("node", n.name.clone()).var("reason", r.down_units.join(", ")).node(&n.id).scenario(sc).fix(fixf));
        }
    }
    CheckOut::found(out)
}

fn status_for(rule: &Rule) -> Status {
    sev(rule)
}

fn survive_site(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let level = domain_level(rule);
    let scs = domain_scenarios(ctx, level);
    if scs.is_empty() {
        return CheckOut::na();
    }
    let mut out = vec![];
    for sc in scs {
        let r = ctx.sim(&sc);
        if r.state == ServiceState::Down && r.recovery.kind == RecoveryKind::Never {
            let Scenario::Zone { zone_id } = &sc else { continue };
            let zi = ctx.ix.zone_ix[zone_id.as_str()];
            let mut raw = Raw::new(zone_id, sev(rule)).var("zone", zone_name(ctx, zi)).var("reason", r.down_units.join(", ")).zone(zone_id).scenario(sc.clone());
            for u in &r.down_units {
                if let Some(ui) = ctx.ix.units.iter().position(|x| &x.name == u) {
                    for &m in &ctx.ix.units[ui].members {
                        raw = raw.node(&nid(ctx, m));
                    }
                }
            }
            // remedy: give the first single-domain unit that is down a member elsewhere
            let fixf = r.down_units.iter().filter_map(|name| ctx.ix.units.iter().position(|x| &x.name == name)).find_map(|ui| add_member_fix_in(ctx, ui, level));
            out.push(raw.fix(fixf));
        }
    }
    CheckOut::found(out)
}

/// What `k` sites lost at the same time do to the service: the verdict of one combination.
pub(crate) struct ComboVerdict {
    pub scenario: Scenario,
    pub zones: Vec<String>,
    pub names: Vec<String>,
    pub problems: Vec<&'static str>,
    pub seconds: Option<f64>,
    pub rpo: Option<f64>,
    pub reason: String,
    pub lost: String,
    pub ratio: Option<f64>,
}

/// Simulates every combination of `k` sites (sampled past `scenarios.maxCombinations`) and keeps the ones that hurt.
pub(crate) fn failing_combinations(ctx: &Ctx<'_>, k: usize, excl: &[String], check_capacity: bool) -> (Vec<ComboVerdict>, usize, bool) {
    let ix = &ctx.ix;
    let (combos, sampled) = crate::sim::site_combinations(ix, k);
    let total = combos.len();
    let req = &ix.arch.requirements;
    let min_cap = ctx.cfg.settings.assumptions.min_capacity_during_failure;
    let mut out = vec![];
    for combo in combos {
        let ids: Vec<String> = combo.iter().map(|&z| ix.arch.zones[z].id.clone()).collect();
        let sc = Scenario::Sites { zone_ids: ids.clone() };
        let r = ctx.sim(&sc);
        let mut problems: Vec<&'static str> = vec![];
        if r.state == ServiceState::Down && r.recovery.kind == RecoveryKind::Never {
            problems.push("down");
        }
        if let (Some(s), Some(rto)) = (r.recovery.seconds, req.rto_seconds) {
            if s > rto && r.recovery.kind != RecoveryKind::Never {
                problems.push("rto");
            }
        }
        let lost = lost_units(ctx, &r.data_lost, excl);
        if !lost.is_empty() {
            problems.push("data_lost");
        }
        let (rpo, _) = crate::sim::rpo_filtered(&r.rpo_items, excl);
        if let (Some(v), Some(want)) = (rpo, req.rpo_seconds) {
            if v > want && lost.is_empty() {
                problems.push("rpo");
            }
        }
        if check_capacity && r.state != ServiceState::Down {
            if let Some(ratio) = r.capacity_ratio {
                if ratio < min_cap {
                    problems.push("capacity");
                }
            }
        }
        if !problems.is_empty() {
            out.push(ComboVerdict {
                scenario: sc,
                zones: ids,
                names: combo.iter().map(|&z| zone_name(ctx, z).to_string()).collect(),
                problems,
                seconds: r.recovery.seconds,
                rpo,
                reason: r.down_units.join(", "),
                lost: lost.join(", "),
                ratio: r.capacity_ratio,
            });
        }
    }
    (out, total, sampled)
}

/// The service must survive the loss of `requirements.siteFailures` sites at the same time (two of five, three of
/// ten…). Every combination of that many sites is simulated.
fn survive_sites(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let k = site_failures(ctx);
    if k < 2 || !tol_covers(ctx, ZoneLevel::Site) {
        return CheckOut::na();
    }
    let ix = &ctx.ix;
    let sites = ix.sites();
    if sites.len() <= k {
        // not even one site would be left
        let mut raw = Raw::new("sites", sev(rule))
            .var("sites", sites.len())
            .var("count", k)
            .var("need", k + 1)
            .var("zones", sites.iter().map(|&s| zone_name(ctx, s).to_string()).collect::<Vec<_>>().join(", "))
            .var("problems", "down")
            .var("reason", ix.units.iter().filter(|u| !u.traits.contains("entry")).map(|u| u.name.clone()).collect::<Vec<_>>().join(", "))
            .var("lost", "")
            .var("seconds", Value::Null)
            .var("rpo", Value::Null)
            .var("more", 0);
        for &s in &sites {
            raw = raw.zone(&ix.arch.zones[s].id);
        }
        return CheckOut::found(vec![raw]);
    }
    let excl = p_vec(rule, "excludeTraits");
    let max_findings = p_f64(rule, "maxFindings").unwrap_or(8.0).max(1.0) as usize;
    let (bad, _total, sampled) = failing_combinations(ctx, k, &excl, p_bool(rule, "capacity").unwrap_or(true));
    if sampled {
        ctx.mark_sampled();
    }
    let hidden = bad.len().saturating_sub(max_findings);
    let mut out = vec![];
    for (i, v) in bad.into_iter().take(max_findings).enumerate() {
        let st = if v.problems == ["capacity"] { Status::Risk } else { sev(rule) };
        let mut raw = Raw::new(v.zones.join("+"), st)
            .var("zones", v.names.join(", "))
            .var("count", k)
            .var("problems", v.problems.join(","))
            .var("reason", v.reason.clone())
            .var("lost", v.lost.clone())
            .var("seconds", v.seconds.map(|x| json!(x)).unwrap_or(Value::Null))
            .var("rpo", v.rpo.map(|x| json!(x)).unwrap_or(Value::Null))
            .var("ratio", v.ratio.map(|x| json!((x * 100.0).round())).unwrap_or(Value::Null))
            .var("more", if i == 0 { hidden } else { 0 })
            .scenario(v.scenario);
        for z in &v.zones {
            raw = raw.zone(z);
        }
        out.push(raw);
    }
    CheckOut::found(out)
}

fn rto(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let scs = site_scenarios(ctx);
    if scs.is_empty() {
        return CheckOut::na();
    }
    let Some(req) = ctx.ix.arch.requirements.rto_seconds else {
        return CheckOut::found(vec![Raw::new("req", Status::NoData).var("what", "rto").need("requirements", None, "rtoSeconds")]);
    };
    let mut out = vec![];
    for sc in scs {
        let r = ctx.sim(&sc);
        let Scenario::Zone { zone_id } = &sc else { continue };
        if r.recovery.kind == RecoveryKind::Never {
            continue; // covered by the survive-site rule
        }
        if let Some(s) = r.recovery.seconds {
            if s > req {
                let zi = ctx.ix.zone_ix[zone_id.as_str()];
                out.push(
                    Raw::new(zone_id, sev(rule))
                        .var("zone", zone_name(ctx, zi))
                        .var("seconds", s)
                        .var("rto", req)
                        .var("kind", if r.recovery.kind == RecoveryKind::Manual { "manual" } else { "auto" })
                        .zone(zone_id)
                        .scenario(sc.clone()),
                );
            }
        }
    }
    CheckOut::found(out)
}

/// Display names of data-lost units that are not excluded from the RPO rule (by trait).
pub(crate) fn lost_units(ctx: &Ctx<'_>, data_lost: &[String], excl: &[String]) -> Vec<String> {
    data_lost.iter().filter_map(|key| ctx.ix.units.iter().find(|u| &u.key == key)).filter(|u| !excl.iter().any(|t| u.traits.contains(t))).map(|u| u.name.clone()).collect()
}

fn rpo(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let scs = site_scenarios(ctx);
    if scs.is_empty() {
        return CheckOut::na();
    }
    let Some(req) = ctx.ix.arch.requirements.rpo_seconds else {
        return CheckOut::found(vec![Raw::new("req", Status::NoData).var("what", "rpo").need("requirements", None, "rpoSeconds")]);
    };
    let excl = p_vec(rule, "excludeTraits");
    let mut out = vec![];
    for sc in scs {
        let r = ctx.sim(&sc);
        let Scenario::Zone { zone_id } = &sc else { continue };
        let zi = ctx.ix.zone_ix[zone_id.as_str()];
        let lost = lost_units(ctx, &r.data_lost, &excl);
        if !lost.is_empty() {
            out.push(Raw::new(format!("{}:lost", zone_id), sev(rule)).var("zone", zone_name(ctx, zi)).var("units", lost.join(", ")).zone(zone_id).scenario(sc.clone()));
            continue;
        }
        let (rpo, assumed) = crate::sim::rpo_filtered(&r.rpo_items, &excl);
        match rpo {
            None => out.push(Raw::new(format!("{}:unknown", zone_id), Status::NoData).var("zone", zone_name(ctx, zi)).zone(zone_id).scenario(sc.clone())),
            Some(v) if v > req => out.push(Raw::new(zone_id, sev(rule)).var("zone", zone_name(ctx, zi)).var("seconds", v).var("rpo", req).zone(zone_id).scenario(sc.clone())),
            Some(v) if assumed && v > 0.0 => out.push(Raw::new(format!("{}:assumed", zone_id), Status::NoData).var("zone", zone_name(ctx, zi)).var("seconds", v).zone(zone_id).scenario(sc.clone())),
            _ => {}
        }
    }
    CheckOut::found(out)
}

fn geo_survive(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    if !tol_covers(ctx, ZoneLevel::Geo) {
        return CheckOut::na();
    }
    let scs = geo_scenarios(ctx);
    let ix = &ctx.ix;
    let mut out = vec![];
    // sites without a geo label make the whole analysis unverifiable
    let unlabeled: Vec<usize> = ix.sites().into_iter().filter(|&s| (0..ix.arch.nodes.len()).all(|ni| ix.site_of[ni] != Some(s) || ix.geo_of[ni].is_none())).collect();
    for s in &unlabeled {
        out.push(Raw::new(format!("nogeo:{}", ix.arch.zones[*s].id), Status::NoData).var("zone", zone_name(ctx, *s)).zone(&ix.arch.zones[*s].id).need("zone", Some(&ix.arch.zones[*s].id), "geo"));
    }
    let req = &ix.arch.requirements;
    for (geo, sc) in scs {
        let r = ctx.sim(&sc);
        let geo = &geo;
        let mut problems: Vec<&str> = vec![];
        if r.state == ServiceState::Down && r.recovery.kind == RecoveryKind::Never {
            problems.push("down");
        }
        if let (Some(s), Some(rto)) = (r.recovery.seconds, req.rto_seconds) {
            if s > rto {
                problems.push("rto");
            }
        }
        if let (Some(v), Some(rpo)) = (r.rpo_seconds, req.rpo_seconds) {
            if v > rpo {
                problems.push("rpo");
            }
        }
        if !r.data_lost.is_empty() {
            problems.push("data_lost");
        }
        if !problems.is_empty() {
            out.push(
                Raw::new(geo, sev(rule))
                    .var("geo", geo.clone())
                    .var("problems", problems.join(","))
                    .var("seconds", r.recovery.seconds.map(|v| json!(v)).unwrap_or(Value::Null))
                    .var("rpo", r.rpo_seconds.map(|v| json!(v)).unwrap_or(Value::Null))
                    .var("lost", r.data_lost.join(", "))
                    .scenario(sc.clone()),
            );
        }
    }
    CheckOut::found(out)
}

fn geo_spread(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    if !tol_covers(ctx, ZoneLevel::Geo) {
        return CheckOut::na();
    }
    let ix = &ctx.ix;
    let mut geos: BTreeSet<String> = BTreeSet::new();
    let mut unknown = false;
    for u in 0..ix.units.len() {
        if !ix.unit_has_trait(u, "stateful") {
            continue;
        }
        for &m in &ix.units[u].members {
            match &ix.geo_of[m] {
                Some(g) => {
                    geos.insert(g.clone());
                }
                None => {
                    if ix.site_of[m].is_some() {
                        unknown = true;
                    }
                }
            }
        }
    }
    if unknown {
        return CheckOut::found(vec![Raw::new("nogeo", Status::NoData)]);
    }
    if geos.len() < 2 {
        return CheckOut::found(vec![Raw::new("spread", sev(rule)).var("geo", geos.into_iter().collect::<Vec<_>>().join(", "))]);
    }
    CheckOut::found(vec![])
}

fn partition_safety(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let scope = p_str(rule, "scope").unwrap_or_else(|| "link".into());
    let scs = link_scenarios(ctx, &scope);
    if scs.is_empty() {
        return CheckOut::na();
    }
    let mode = p_str(rule, "mode").unwrap_or_else(|| "both".into());
    let mut out = vec![];
    for sc in scs {
        let r = ctx.sim(&sc);
        // the zones that take part, a stable key and the label of the cut
        let (zones, key, link): (Vec<String>, String, String) = match &sc {
            Scenario::Link { a, b } => {
                let (za, zb) = (ctx.ix.zone_ix[a.as_str()], ctx.ix.zone_ix[b.as_str()]);
                (vec![a.clone(), b.clone()], format!("{}|{}", a, b), format!("{} ↔ {}", zone_name(ctx, za), zone_name(ctx, zb)))
            }
            Scenario::Isolate { zone_ids } => {
                let names: Vec<String> = zone_ids.iter().filter_map(|z| ctx.ix.zone_ix.get(z.as_str())).map(|&zi| zone_name(ctx, zi).to_string()).collect();
                (zone_ids.clone(), format!("iso:{}", zone_ids.join("+")), names.join(" + "))
            }
            _ => continue,
        };
        if !r.split_brain.is_empty() {
            if mode == "unavailable" {
                continue;
            }
            let mut raw = Raw::new(format!("{}:split", key), Status::Critical)
                .var("link", link.clone())
                .var("unit", r.split_brain.iter().filter_map(|k| ctx.ix.unit_ix.get(k)).map(|&u| ctx.ix.units[u].name.clone()).collect::<Vec<_>>().join(", "))
                .scenario(sc.clone());
            for z in &zones {
                raw = raw.zone(z);
            }
            for k in &r.split_brain {
                if let Some(&u) = ctx.ix.unit_ix.get(k) {
                    for &m in &ctx.ix.units[u].members {
                        raw = raw.node(&nid(ctx, m));
                    }
                }
            }
            out.push(raw);
        } else if r.state == ServiceState::Down && mode != "split_brain" {
            let mut raw = Raw::new(format!("{}:down", key), sev(rule)).var("link", link).var("reason", r.down_units.join(", ")).scenario(sc.clone());
            for z in &zones {
                raw = raw.zone(z);
            }
            out.push(raw);
        }
    }
    CheckOut::found(out)
}

// ---------------------------------------------------------------------------------------------
// structure
// ---------------------------------------------------------------------------------------------

fn instance_count(ctx: &Ctx<'_>, u: usize, mode: &str) -> u32 {
    let ix = &ctx.ix;
    match mode {
        "members" => ix.units[u].members.len() as u32,
        "dataMembers" => data_members(ctx, u).len() as u32,
        _ => ix.units[u].members.iter().filter(|&&m| !ix.is_witness(m)).map(|&m| ix.replicas(m)).sum(),
    }
}

fn min_instances(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let min = p_f64(rule, "min").unwrap_or(2.0) as u32;
    let mode = p_str(rule, "count").unwrap_or_else(|| "instances".into());
    let mut out = vec![];
    let mut any = false;
    for u in 0..ctx.ix.units.len() {
        if !unit_matches(ctx, rule, u) || ctx.ix.unit_has_trait(u, "entry") {
            continue;
        }
        any = true;
        let n = instance_count(ctx, u, &mode);
        if n < min {
            let unit = &ctx.ix.units[u];
            let mut raw = Raw::new(&unit.key, sev(rule)).var("unit", unit.name.clone()).var("count", n).var("min", min);
            for &m in &unit.members {
                raw = raw.node(&nid(ctx, m));
            }
            let f = if mode == "instances" && unit.members.len() == 1 && unit.behavior == Behavior::Stateless {
                let m = unit.members[0];
                let new = ctx.ix.replicas(m) + (min - n);
                Some(set_prop_fix(ctx, m, "replicas", json!(new), "set_replicas", json!({"replicas": new})))
            } else {
                add_member_fix(ctx, u)
            };
            out.push(raw.fix(f));
        }
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}

fn spread_sites(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let required = p_str(rule, "requireTolerance").unwrap_or_else(|| "site".into());
    if tol_rank(ctx, &tolerance(ctx)) < tol_rank(ctx, &required) {
        return CheckOut::na();
    }
    let level = domain_level(rule);
    if !domain_applies(ctx, level) {
        return CheckOut::na();
    }
    let mut min = p_f64(rule, "minSites").unwrap_or(2.0) as usize;
    // losing k sites at once leaves copies only if the unit lives in at least k + 1 of them
    if level == ZoneLevel::Site && p_bool(rule, "scaleWithSiteFailures").unwrap_or(false) {
        min = min.max(site_failures(ctx) + 1);
    }
    let accept_repl = p_bool(rule, "acceptReplication").unwrap_or(false);
    let ix = &ctx.ix;
    let mut out = vec![];
    let mut any = false;
    for u in 0..ix.units.len() {
        if !unit_matches(ctx, rule, u) {
            continue;
        }
        let sites = ix.unit_domains(u, level);
        // global (zone-less) units are not bound to a site
        if sites.is_empty() {
            continue;
        }
        any = true;
        if sites.len() >= min {
            continue;
        }
        if accept_repl {
            let replicated = ix.units[u].members.iter().any(|&m| {
                ix.out_edges[m].iter().chain(ix.in_edges[m].iter()).any(|&ei| {
                    ix.sem(ei) == Some(Semantics::Replication)
                        && ix.edge_ends[ei]
                            .map(|(s, t)| {
                                let o = if s == m { t } else { s };
                                ix.unit_of[o] != u && ix.site_of[o].is_some() && ix.site_of[o] != ix.site_of[m]
                            })
                            .unwrap_or(false)
                })
            });
            if replicated {
                continue;
            }
        }
        let unit = &ix.units[u];
        let mut raw = Raw::new(&unit.key, sev(rule)).var("unit", unit.name.clone()).var("sites", sites.len()).var("min", min);
        for &m in &unit.members {
            raw = raw.node(&nid(ctx, m));
        }
        out.push(raw.fix(add_member_fix_in(ctx, u, level)));
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}

fn voting_units(ctx: &Ctx<'_>) -> Vec<usize> {
    let ix = &ctx.ix;
    (0..ix.units.len())
        .filter(|&u| match ix.units[u].behavior {
            Behavior::Consensus => true,
            Behavior::PrimaryReplica => ix.authority(u) == Authority::SelfVotes,
            _ => false,
        })
        .collect()
}

fn vote_weight(ctx: &Ctx<'_>, u: usize, m: usize) -> f64 {
    let v = ctx.ix.eff[m].num("votes").unwrap_or(1.0);
    if ctx.ix.units[u].behavior == Behavior::Consensus {
        v * ctx.ix.replicas(m) as f64
    } else {
        v
    }
}

fn quorum_voters(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let units = voting_units(ctx);
    if units.is_empty() {
        return CheckOut::na();
    }
    let mode = p_str(rule, "mode").unwrap_or_else(|| "odd_min3".into());
    let level = domain_level(rule);
    if mode == "survive_site" {
        // a quorum that must outlive a failure domain only matters when that domain is part of the requirement
        let required = p_str(rule, "requireTolerance");
        if let Some(r) = &required {
            if tol_rank(ctx, &tolerance(ctx)) < tol_rank(ctx, r) {
                return CheckOut::na();
            }
        }
        if !domain_applies(ctx, level) {
            return CheckOut::na();
        }
    }
    let ix = &ctx.ix;
    let mut out = vec![];
    for u in units {
        let total: f64 = ix.units[u].members.iter().map(|&m| vote_weight(ctx, u, m)).sum();
        let unit = &ix.units[u];
        if mode == "survive_site" {
            if total < 3.0 {
                continue; // reported by the odd/min-3 rule
            }
            // quorum must outlive the loss of `siteFailures` sites at once (one by default; racks: one rack)
            let doms = ix.unit_domains(u, level);
            let want = if level == ZoneLevel::Site { site_failures(ctx) } else { 1 };
            let k = want.min(doms.len());
            if k == 0 {
                continue;
            }
            let held = |d: usize| -> f64 { unit.members.iter().filter(|&&m| ix.domain_of(m, level) == Some(d)).map(|&m| vote_weight(ctx, u, m)).sum() };
            let (combos, sampled) = crate::sim::combinations(doms.len(), k, ctx.cfg.settings.scenarios.max_combinations);
            if sampled {
                ctx.mark_sampled();
            }
            let max_findings = p_f64(rule, "maxFindings").unwrap_or(12.0).max(1.0) as usize;
            let mut emitted = 0;
            for c in combos {
                let lost_doms: Vec<usize> = c.iter().map(|&i| doms[i]).collect();
                let rem: f64 = total - lost_doms.iter().map(|&d| held(d)).sum::<f64>();
                if rem * 2.0 > total {
                    continue;
                }
                if emitted >= max_findings {
                    break;
                }
                emitted += 1;
                let ids: Vec<&str> = lost_doms.iter().map(|&d| ix.arch.zones[d].id.as_str()).collect();
                let mut raw = Raw::new(format!("{}@{}", unit.key, ids.join("+")), sev(rule))
                    .var("unit", unit.name.clone())
                    .var("zone", lost_doms.iter().map(|&d| zone_name(ctx, d).to_string()).collect::<Vec<_>>().join(", "))
                    .var("votes", rem)
                    .var("total", total)
                    .var("count", k);
                for id in &ids {
                    raw = raw.zone(id);
                }
                for &m in &unit.members {
                    raw = raw.node(&nid(ctx, m));
                }
                out.push(raw.fix(add_witness_fix_in(ctx, u, level)));
            }
        } else {
            let odd = (total as i64) % 2 == 1;
            if !odd || total < 3.0 {
                let mut raw = Raw::new(&unit.key, sev(rule)).var("unit", unit.name.clone()).var("votes", total);
                for &m in &unit.members {
                    raw = raw.node(&nid(ctx, m));
                }
                out.push(raw.fix(add_witness_fix(ctx, u)));
            }
        }
    }
    CheckOut::found(out)
}

fn witness_independent(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let mut out = vec![];
    let mut any = false;
    // an arbiter must not share a failure domain with the data it arbitrates – at every level the requirement reaches
    let mut levels = vec![];
    if tol_covers(ctx, ZoneLevel::Site) {
        levels.push(ZoneLevel::Site);
    }
    if domain_applies(ctx, ZoneLevel::Rack) {
        levels.push(ZoneLevel::Rack);
    }
    if levels.is_empty() {
        levels.push(ZoneLevel::Site);
    }
    for u in 0..ix.units.len() {
        let witnesses: Vec<usize> = ix.units[u].members.iter().copied().filter(|&m| ix.is_witness(m)).collect();
        if witnesses.is_empty() {
            continue;
        }
        any = true;
        let data = data_members(ctx, u);
        for w in witnesses {
            for &level in &levels {
                let Some(wd) = ix.domain_of(w, level) else { continue };
                if data.iter().any(|&d| ix.domain_of(d, level) == Some(wd)) {
                    out.push(Raw::new(nid(ctx, w), sev(rule)).var("node", ix.node_name(w)).var("zone", zone_name(ctx, wd)).node(&nid(ctx, w)).zone(&ix.arch.zones[wd].id));
                    break;
                }
            }
        }
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}

fn replication_policy(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let mut candidates: Vec<usize> = vec![];
    for (ei, ends) in ix.edge_ends.iter().enumerate() {
        if ix.sem(ei) != Some(Semantics::Replication) {
            continue;
        }
        let Some((s, t)) = ends else { continue };
        let (Some(a), Some(b)) = (ix.site_of[*s], ix.site_of[*t]) else { continue };
        if a == b {
            continue;
        }
        let u = ix.unit_of[*s];
        if !unit_matches(ctx, rule, u) {
            continue;
        }
        candidates.push(ei);
    }
    if candidates.is_empty() {
        return CheckOut::na();
    }
    let Some(req) = ix.arch.requirements.rpo_seconds else {
        return CheckOut::found(vec![Raw::new("req", Status::NoData).var("what", "rpo").need("requirements", None, "rpoSeconds")]);
    };
    let mut out = vec![];
    for ei in candidates {
        let e = &ix.arch.edges[ei];
        let eff = &ix.edge_eff[ei];
        let mode = eff.text("mode").unwrap_or_else(|| "async".into());
        if mode == "sync" {
            continue;
        }
        if eff.text("mitigation").is_some() {
            continue;
        }
        let declared = eff.is_declared("lagSeconds");
        let lag = eff.num("lagSeconds").unwrap_or(ctx.cfg.settings.assumptions.async_lag_seconds);
        let (s, t) = ix.edge_ends[ei].unwrap();
        // RPO is carried by a synchronous peer in another site: the extra async copy is a bonus
        let has_sync_peer = ix.out_edges[s].iter().any(|&oi| {
            oi != ei
                && ix.sem(oi) == Some(Semantics::Replication)
                && ix.edge_eff[oi].text("mode").as_deref() == Some("sync")
                && ix.edge_ends[oi].map(|(_, tt)| ix.site_of[tt].is_some() && ix.site_of[tt] != ix.site_of[s]).unwrap_or(false)
        });
        if has_sync_peer && req == 0.0 {
            continue;
        }
        let base = Raw::new(&e.id, Status::Risk).var("source", ix.node_name(s)).var("target", ix.node_name(t)).var("lag", lag).var("rpo", req).edge(&e.id).node(&nid(ctx, s)).node(&nid(ctx, t));
        if lag > req {
            let mut r = base;
            r.status = sev(rule);
            let can_sync = eff.num("latencyMs").map(|l| l <= ctx.cfg.settings.assumptions.sync_max_rtt_ms).unwrap_or(false);
            if can_sync {
                let mut set = Map::new();
                set.insert("type".into(), json!("sync"));
                r.fixes.push(fix("make_sync", &e.id, json!({"source": ix.node_name(s), "target": ix.node_name(t)}), vec![Op::UpdateEdge { id: e.id.clone(), set }]));
            }
            out.push(r);
        } else if !declared {
            let mut r = base.need("edge", Some(&e.id), "lagSeconds");
            r.status = Status::NoData;
            out.push(r);
        }
    }
    CheckOut::found(out)
}

fn failover_authority(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let mut out = vec![];
    let mut any = false;
    for u in 0..ix.units.len() {
        if ix.units[u].behavior != Behavior::PrimaryReplica {
            continue;
        }
        let data = data_members(ctx, u);
        if data.len() < 2 {
            continue;
        }
        any = true;
        let auto = data.iter().any(|&m| ix.eff[m].text("failover").as_deref() == Some("auto"));
        if auto && ix.authority(u) == Authority::None {
            let unit = &ix.units[u];
            let mut raw = Raw::new(&unit.key, sev(rule)).var("unit", unit.name.clone());
            for &m in &unit.members {
                raw = raw.node(&nid(ctx, m));
            }
            // remedy 1: reference an existing consensus cluster as DCS; remedy 2: add a witness
            if let Some(cu) = (0..ix.units.len()).find(|&c| ix.units[c].behavior == Behavior::Consensus) {
                let set_ops: Vec<Op> = data.iter().map(|&m| update_node(&ix.arch.nodes[m].id, vec![("props.dcs", json!(ix.units[cu].key))])).collect();
                raw = raw.fix(Some(fix("set_dcs", &unit.key, json!({"unit": unit.name, "dcs": ix.units[cu].name}), set_ops)));
            }
            raw = raw.fix(add_witness_fix(ctx, u));
            out.push(raw);
        }
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}

fn has_backup_edge(ctx: &Ctx<'_>, u: usize) -> bool {
    let ix = &ctx.ix;
    ix.units[u].members.iter().any(|&m| ix.out_edges[m].iter().chain(ix.in_edges[m].iter()).any(|&ei| ix.sem(ei) == Some(Semantics::Backup)))
}

fn replicated_elsewhere(ctx: &Ctx<'_>, u: usize) -> bool {
    let ix = &ctx.ix;
    ix.units[u].members.iter().any(|&m| {
        ix.out_edges[m].iter().chain(ix.in_edges[m].iter()).any(|&ei| {
            ix.sem(ei) == Some(Semantics::Replication)
                && ix.edge_ends[ei]
                    .map(|(s, t)| {
                        let o = if s == m { t } else { s };
                        ix.unit_of[o] != u && ix.site_of[o].is_some() && ix.site_of[o] != ix.site_of[m]
                    })
                    .unwrap_or(false)
        })
    })
}

fn backup_exists(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let accept = p_bool(rule, "acceptReplication").unwrap_or(false);
    let mut out = vec![];
    let mut any = false;
    for u in 0..ctx.ix.units.len() {
        if !unit_matches(ctx, rule, u) {
            continue;
        }
        any = true;
        if has_backup_edge(ctx, u) || (accept && replicated_elsewhere(ctx, u)) {
            continue;
        }
        let unit = &ctx.ix.units[u];
        let mut raw = Raw::new(&unit.key, sev(rule)).var("unit", unit.name.clone());
        for &m in &unit.members {
            raw = raw.node(&nid(ctx, m));
        }
        out.push(raw.fix(add_backup_fix(ctx, u)));
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}

fn backup_offsite(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let geo_tol = tol_covers(ctx, ZoneLevel::Geo);
    let mut out = vec![];
    let mut any = false;
    for (ei, ends) in ix.edge_ends.iter().enumerate() {
        if ix.sem(ei) != Some(Semantics::Backup) {
            continue;
        }
        let Some((s, t)) = ends else { continue };
        let (Some(a), Some(b)) = (ix.site_of[*s], ix.site_of[*t]) else { continue };
        any = true;
        let e = &ix.arch.edges[ei];
        let base = |st: Status| Raw::new(&e.id, st).var("source", ix.node_name(*s)).var("target", ix.node_name(*t)).var("zone", zone_name(ctx, a)).edge(&e.id).node(&nid(ctx, *s)).node(&nid(ctx, *t));
        if a == b {
            out.push(base(sev(rule)));
        } else if geo_tol {
            match (&ix.geo_of[*s], &ix.geo_of[*t]) {
                (Some(g1), Some(g2)) if g1 == g2 => out.push(base(Status::Risk).var("geo", g1.clone())),
                (Some(_), Some(_)) => {}
                (g1, g2) => {
                    let mut raw = base(Status::NoData);
                    for (g, node) in [(g1, s), (g2, t)] {
                        if g.is_none() {
                            if let Some(z) = ix.arch.nodes[*node].zone_id.as_deref() {
                                raw = raw.need("zone", Some(z), "geo");
                            }
                        }
                    }
                    out.push(raw);
                }
            }
        }
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}

// ---------------------------------------------------------------------------------------------
// generic property check (encryption, immutability, latency, TLS, HPA, health-check, tier, …)
// ---------------------------------------------------------------------------------------------

fn setting_ref(ctx: &Ctx<'_>, path: &str) -> Option<Value> {
    let mut cur = serde_json::to_value(&ctx.cfg.settings).ok()?;
    for part in path.split('.') {
        cur = cur.get(part)?.clone();
    }
    Some(cur)
}

fn compare(op: &str, actual: &Value, expected: &Value) -> bool {
    match op {
        "present" => !matches!(actual, Value::Null),
        "eq" => match (as_bool(actual), as_bool(expected)) {
            (Some(a), Some(b)) if expected.is_boolean() || actual.is_boolean() => a == b,
            _ => match (as_f64(actual), as_f64(expected)) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-9,
                _ => as_string(actual) == as_string(expected),
            },
        },
        "ne" => !compare("eq", actual, expected),
        "in" => expected.as_array().map(|arr| arr.iter().any(|x| compare("eq", actual, x))).unwrap_or(false),
        "gte" | "lte" | "gt" | "lt" => match (as_f64(actual), as_f64(expected)) {
            (Some(a), Some(b)) => match op {
                "gte" => a >= b,
                "lte" => a <= b,
                "gt" => a > b,
                _ => a < b,
            },
            _ => false,
        },
        _ => true,
    }
}

fn prop_check(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let subject = p_str(rule, "subject").unwrap_or_else(|| "node".into());
    let key = p_str(rule, "key").unwrap_or_default();
    let op = p_str(rule, "op").unwrap_or_else(|| "present".into());
    let expected = p(rule, "value").cloned().or_else(|| p_str(rule, "valueRef").and_then(|r| setting_ref(ctx, &r))).unwrap_or(Value::Null);
    let on_missing = p_str(rule, "onMissing").unwrap_or_else(|| "no_data".into());
    let declared_only = p_bool(rule, "declaredOnly").unwrap_or(false);
    let viol_status = match p_str(rule, "violationStatus").as_deref() {
        Some("no_data") => Status::NoData,
        Some("critical") => Status::Critical,
        Some("risk") => Status::Risk,
        _ => sev(rule),
    };
    let w = p(rule, "where").cloned().unwrap_or(Value::Null);
    let w_str = |k: &str| w.get(k).and_then(as_string);
    let w_vec = |k: &str| -> Vec<String> { w.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(as_string).collect()).unwrap_or_default() };

    let mut out = vec![];
    let mut matched = 0;

    // returns Some(raw) for violation / missing data
    let judge = |actual: Option<Value>, declared: bool, key_id: String, mk: &dyn Fn(Status) -> Raw| -> Option<Raw> {
        let actual = if declared_only && !declared { None } else { actual };
        match actual {
            None | Some(Value::Null) => match on_missing.as_str() {
                "skip" => None,
                "violation" => Some(mk(sev(rule)).var("actual", Value::Null)),
                _ => Some(mk(Status::NoData).var("actual", Value::Null)),
            },
            Some(a) => {
                if compare(&op, &a, &expected) {
                    None
                } else {
                    let _ = key_id;
                    Some(mk(viol_status).var("actual", a))
                }
            }
        }
    };

    match subject.as_str() {
        "node" => {
            for (ni, n) in ix.arch.nodes.iter().enumerate() {
                if let Some(k) = w_str("kind") {
                    if n.kind != k {
                        continue;
                    }
                }
                if let Some(t) = w_str("trait") {
                    if !ix.has_trait(ni, &t) {
                        continue;
                    }
                }
                if let Some(b) = w_str("behavior") {
                    let want: Option<Behavior> = serde_json::from_value(Value::String(b)).ok();
                    if want != Some(ix.behavior[ni]) {
                        continue;
                    }
                }
                if let Some(r) = w_str("role") {
                    if n.role.as_deref() != Some(r.as_str()) {
                        continue;
                    }
                }
                if w_vec("excludeTraits").iter().any(|t| ix.has_trait(ni, t)) {
                    continue;
                }
                let skip_roles = w_vec("excludeZoneRoles");
                if !skip_roles.is_empty() && ix.zone_chain[ni].iter().any(|&zi| ix.arch.zones[zi].role.as_deref().map(|r| skip_roles.iter().any(|x| x == r)).unwrap_or(false)) {
                    continue;
                }
                matched += 1;
                let actual = ix.eff[ni].map.get(&key).cloned();
                let declared = ix.eff[ni].is_declared(&key);
                let mk = |st: Status| Raw::new(format!("{}:{}", n.id, key), st).var("node", n.name.clone()).var("key", key.clone()).var("expected", expected.clone()).node(&n.id);
                if let Some(mut raw) = judge(actual, declared, n.id.clone(), &mk) {
                    if raw.status == Status::NoData {
                        raw = raw.need("node", Some(&n.id), &key);
                    }
                    // mechanical remedy when the expected literal is unambiguous
                    if raw.status != Status::NoData && matches!(op.as_str(), "eq" | "gte" | "lte") && !expected.is_null() {
                        raw.fixes.push(set_prop_fix(ctx, ni, &key, expected.clone(), "set_prop", json!({})));
                    }
                    out.push(raw);
                } else if false {
                    unreachable!();
                }
                // missing data with a known sensible value is still no_data: no fix is offered
            }
        }
        "edge" => {
            let types = {
                let mut t = w_vec("types");
                if let Some(x) = w_str("type") {
                    t.push(x);
                }
                t
            };
            let cross = w.get("crossSite").and_then(as_bool).unwrap_or(false);
            for (ei, e) in ix.arch.edges.iter().enumerate() {
                if !types.is_empty() && !types.contains(&e.edge_type) {
                    continue;
                }
                let Some((s, t)) = ix.edge_ends[ei] else { continue };
                if cross {
                    match (ix.site_of[s], ix.site_of[t]) {
                        (Some(a), Some(b)) if a != b => {}
                        _ => continue,
                    }
                }
                matched += 1;
                let actual = ix.edge_eff[ei].map.get(&key).cloned();
                let declared = ix.edge_eff[ei].is_declared(&key);
                let label = e.label.clone().unwrap_or_else(|| format!("{} → {}", ix.node_name(s), ix.node_name(t)));
                let mk = |st: Status| {
                    Raw::new(format!("{}:{}", e.id, key), st)
                        .var("edge", label.clone())
                        .var("source", ix.node_name(s))
                        .var("target", ix.node_name(t))
                        .var("key", key.clone())
                        .var("expected", expected.clone())
                        .edge(&e.id)
                        .node(&nid(ctx, s))
                        .node(&nid(ctx, t))
                };
                if let Some(mut raw) = judge(actual, declared, e.id.clone(), &mk) {
                    if raw.status == Status::NoData {
                        raw = raw.need("edge", Some(&e.id), &key);
                    }
                    if raw.status != Status::NoData && matches!(op.as_str(), "eq" | "gte" | "lte") && !expected.is_null() {
                        let mut set = Map::new();
                        set.insert(format!("props.{}", key), expected.clone());
                        raw.fixes.push(fix("set_prop", &format!("{}:{}", e.id, key), json!({"node": label, "key": key, "value": expected}), vec![Op::UpdateEdge { id: e.id.clone(), set }]));
                    }
                    out.push(raw);
                }
            }
        }
        "zone" => {
            for (zi, z) in ix.arch.zones.iter().enumerate() {
                if let Some(t) = w_str("type") {
                    if z.zone_type != t {
                        continue;
                    }
                }
                if let Some(l) = w_str("level") {
                    let want: Option<ZoneLevel> = serde_json::from_value(Value::String(l)).ok();
                    if want != Some(ix.zone_level(zi)) {
                        continue;
                    }
                }
                matched += 1;
                let actual = z.props.get(&key).cloned();
                let mk = |st: Status| Raw::new(format!("{}:{}", z.id, key), st).var("zone", z.name.clone()).var("key", key.clone()).var("expected", expected.clone()).zone(&z.id);
                if let Some(mut raw) = judge(actual, true, z.id.clone(), &mk) {
                    if raw.status == Status::NoData {
                        raw = raw.need("zone", Some(&z.id), &key);
                    }
                    out.push(raw);
                }
            }
        }
        _ => return CheckOut::na(),
    }
    if matched == 0 {
        CheckOut::na()
    } else {
        CheckOut::found(out)
    }
}

// ---------------------------------------------------------------------------------------------
// capacity & cost
// ---------------------------------------------------------------------------------------------

fn capacity(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let Some(rps) = ix.arch.requirements.rps_peak else {
        return CheckOut::found(vec![Raw::new("req", Status::NoData).var("what", "rps").need("requirements", None, "rpsPeak")]);
    };
    let growth = ix.arch.requirements.growth_percent.unwrap_or(0.0) / 100.0;
    let headroom = ctx.cfg.settings.assumptions.headroom_percent / 100.0;
    let base = ctx.sim(&Scenario::None);
    let model_traits = capacity_model_traits(Some(rule));
    let _ = rps;
    let mut out = vec![];
    for l in &base.unit_loads {
        let Some(&u) = ix.unit_ix.get(&l.unit) else { continue };
        let unit = &ix.units[u];
        let raw_base = |st: Status| {
            let mut r = Raw::new(&unit.key, st).var("unit", unit.name.clone()).var("load", l.load_rps.round());
            for &m in &unit.members {
                r = r.node(&nid(ctx, m));
            }
            r
        };
        match l.capacity_rps {
            None => {
                if model_traits.iter().any(|t| unit.traits.contains(t)) {
                    let mut raw = raw_base(Status::NoData).var("capacity", Value::Null);
                    for &m in unit.members.iter().filter(|&&m| !ix.is_witness(m)).take(3) {
                        raw = raw.need("node", Some(&ix.arch.nodes[m].id), "rpsPerReplica");
                    }
                    out.push(raw);
                }
            }
            Some(cap) => {
                let need = l.load_rps * (1.0 + growth);
                if cap < need {
                    let f = capacity_fix(ctx, u, need, headroom);
                    out.push(raw_base(Status::Critical).var("capacity", cap.round()).var("needed", need.round()).fix(f));
                } else if need > cap * (1.0 - headroom) {
                    let f = capacity_fix(ctx, u, need, headroom);
                    out.push(raw_base(sev_or(rule, Status::Risk)).var("capacity", cap.round()).var("needed", (need / (1.0 - headroom)).round()).fix(f));
                }
            }
        }
    }
    let _ = rule;
    CheckOut::found(out)
}

/// Traits of the kinds that must come with a capacity model (`requireModelTraits` of the capacity rule): a unit
/// without one of them (an external client, a monitoring agent…) is not asked for RPS per instance – neither by the
/// rule nor in the capacity plan.
pub fn capacity_model_traits(rule: Option<&Rule>) -> Vec<String> {
    let v = rule.map(|r| p_vec(r, "requireModelTraits")).unwrap_or_default();
    if v.is_empty() {
        vec!["workload".to_string(), "datastore".to_string(), "balancer".to_string()]
    } else {
        v
    }
}

fn sev_or(_rule: &Rule, s: Status) -> Status {
    s
}

/// Scale the members of a stateless unit so that `need` fits within the headroom.
fn capacity_fix(ctx: &Ctx<'_>, u: usize, need: f64, headroom: f64) -> Option<Fix> {
    let ix = &ctx.ix;
    let unit = &ix.units[u];
    if unit.behavior != Behavior::Stateless {
        return None;
    }
    let members: Vec<usize> = unit.members.iter().copied().filter(|&m| !ix.is_witness(m)).collect();
    let per = members.iter().find_map(|&m| ix.per_replica_rps(m))?;
    if per <= 0.0 || members.is_empty() {
        return None;
    }
    let total = (need / ((1.0 - headroom) * per)).ceil();
    let each = (total / members.len() as f64).ceil().max(1.0) as u64;
    let ops: Vec<Op> = members.iter().map(|&m| update_node(&ix.arch.nodes[m].id, vec![("props.replicas", json!(each))])).collect();
    Some(fix("scale", &unit.key, json!({"unit": unit.name, "replicas": each}), ops))
}

fn capacity_under_failure(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let scs = site_scenarios(ctx);
    if scs.is_empty() {
        return CheckOut::na();
    }
    let ix = &ctx.ix;
    if ix.arch.requirements.rps_peak.is_none() {
        return CheckOut::found(vec![Raw::new("req", Status::NoData).var("what", "rps").need("requirements", None, "rpsPeak")]);
    }
    let min = ctx.cfg.settings.assumptions.min_capacity_during_failure;
    let mut out = vec![];
    for sc in scs {
        let r = ctx.sim(&sc);
        let Scenario::Zone { zone_id } = &sc else { continue };
        if r.state == ServiceState::Down {
            continue;
        }
        match r.capacity_ratio {
            None => {}
            Some(ratio) if ratio < min => {
                let zi = ix.zone_ix[zone_id.as_str()];
                let mut raw = Raw::new(zone_id, sev(rule))
                    .var("zone", zone_name(ctx, zi))
                    .var("ratio", (ratio * 100.0).round())
                    .var("min", (min * 100.0).round())
                    .var("unit", r.bottleneck.as_ref().and_then(|k| ix.unit_ix.get(k)).map(|&u| ix.units[u].name.clone()).unwrap_or_default())
                    .zone(zone_id)
                    .scenario(sc.clone());
                // remedy: scale the bottleneck so the surviving sites carry `min` of the peak load
                if let Some(&u) = r.bottleneck.as_ref().and_then(|k| ix.unit_ix.get(k)) {
                    if ix.units[u].behavior == Behavior::Stateless && ratio > 0.0 {
                        let factor = (min / ratio).max(1.0);
                        let ops: Vec<Op> = ix.units[u]
                            .members
                            .iter()
                            .filter(|&&m| !ix.is_witness(m))
                            .map(|&m| {
                                let cur = ix.replicas(m) as f64;
                                update_node(&ix.arch.nodes[m].id, vec![("props.replicas", json!((cur * factor).ceil() as u64))])
                            })
                            .collect();
                        raw = raw.fix(Some(fix("scale", &format!("{}@{}", ix.units[u].key, zone_id), json!({"unit": ix.units[u].name, "factor": (factor * 100.0).round() / 100.0}), ops)));
                    }
                }
                out.push(raw);
            }
            _ => {}
        }
    }
    CheckOut::found(out)
}

fn cost_budget(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let Some(budget) = ctx.ix.arch.requirements.budget_monthly else {
        return CheckOut::found(vec![Raw::new("req", Status::NoData).var("what", "budget").need("requirements", None, "budgetMonthly")]);
    };
    let c = cost::estimate(&ctx.ix);
    let warn = ctx.cfg.settings.cost.warn_percent / 100.0;
    let vars = |st: Status| {
        Raw::new("budget", st)
            .var("total", c.total.round())
            .var("budget", budget)
            .var("symbol", c.symbol.clone())
            .var("percent", if budget > 0.0 { (c.total / budget * 100.0).round() } else { 0.0 })
            .var("incomplete", c.incomplete)
    };
    if c.total > budget {
        return CheckOut::found(vec![vars(sev(rule))]);
    }
    if c.total > budget * warn {
        return CheckOut::found(vec![vars(Status::Risk)]);
    }
    if c.incomplete > 0 {
        return CheckOut::found(vec![vars(Status::NoData)]);
    }
    CheckOut::found(vec![])
}

// ---------------------------------------------------------------------------------------------
// topology hygiene
// ---------------------------------------------------------------------------------------------

fn trait_of(rule: &Rule, key: &str) -> Option<String> {
    p(rule, key).and_then(|v| v.get("trait")).and_then(as_string)
}

fn path_contains(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let (Some(from), Some(to), Some(via)) = (trait_of(rule, "from"), trait_of(rule, "to"), trait_of(rule, "via")) else {
        return CheckOut::na();
    };
    let starts: Vec<usize> = (0..ix.arch.nodes.len()).filter(|&n| ix.has_trait(n, &from)).collect();
    let targets: Vec<usize> = (0..ix.arch.nodes.len()).filter(|&n| ix.has_trait(n, &to)).collect();
    if starts.is_empty() || targets.is_empty() {
        return CheckOut::na();
    }
    let mut seen = vec![false; ix.arch.nodes.len()];
    let mut stack = starts.clone();
    for &s in &starts {
        seen[s] = true;
    }
    while let Some(n) = stack.pop() {
        if ix.has_trait(n, &via) {
            continue; // traffic is inspected here; do not expand beyond
        }
        for &ei in &ix.out_edges[n] {
            if ix.sem(ei) != Some(Semantics::Dependency) {
                continue;
            }
            if let Some((_, t)) = ix.edge_ends[ei] {
                if !seen[t] {
                    seen[t] = true;
                    stack.push(t);
                }
            }
        }
    }
    let mut out = vec![];
    for &t in &targets {
        if seen[t] && !ix.has_trait(t, &via) {
            out.push(Raw::new(nid(ctx, t), sev(rule)).var("node", ix.node_name(t)).var("via", via.clone()).node(&nid(ctx, t)).fix(add_required_fix(ctx, &via)));
        }
    }
    CheckOut::found(out)
}

fn forbidden_edge(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let (Some(from), Some(to)) = (trait_of(rule, "from"), trait_of(rule, "to")) else {
        return CheckOut::na();
    };
    let any_from = (0..ix.arch.nodes.len()).any(|n| ix.has_trait(n, &from));
    let any_to = (0..ix.arch.nodes.len()).any(|n| ix.has_trait(n, &to));
    if !any_from || !any_to {
        return CheckOut::na();
    }
    let mut out = vec![];
    for (ei, e) in ix.arch.edges.iter().enumerate() {
        let Some((s, t)) = ix.edge_ends[ei] else { continue };
        if ix.has_trait(s, &from) && ix.has_trait(t, &to) {
            out.push(Raw::new(&e.id, sev(rule)).var("source", ix.node_name(s)).var("target", ix.node_name(t)).edge(&e.id).node(&nid(ctx, s)).node(&nid(ctx, t)).fix(Some(fix(
                "remove_edge",
                &e.id,
                json!({"source": ix.node_name(s), "target": ix.node_name(t)}),
                vec![Op::RemoveEdge { id: e.id.clone() }],
            ))));
        }
    }
    CheckOut::found(out)
}

fn no_cycles(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let mut out = vec![];
    for c in &ix.cycles {
        let names: Vec<String> = c.iter().map(|&u| ix.units[u].name.clone()).collect();
        let mut keys: Vec<String> = c.iter().map(|&u| ix.units[u].key.clone()).collect();
        keys.sort();
        let mut raw = Raw::new(keys.join("+"), sev(rule)).var("units", names.join(" → "));
        for &u in c {
            for &m in &ix.units[u].members {
                raw = raw.node(&nid(ctx, m));
            }
        }
        out.push(raw);
    }
    CheckOut::found(out)
}

fn required_kind(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let Some(tr) = p_str(rule, "trait") else { return CheckOut::na() };
    if let Some(w) = p_str(rule, "whenTrait") {
        if !(0..ix.arch.nodes.len()).any(|n| ix.has_trait(n, &w)) {
            return CheckOut::na();
        }
    } else if ix.arch.nodes.is_empty() {
        return CheckOut::na();
    }
    if (0..ix.arch.nodes.len()).any(|n| ix.has_trait(n, &tr)) {
        return CheckOut::found(vec![]);
    }
    CheckOut::found(vec![Raw::new(tr.clone(), sev(rule)).var("trait", tr.clone()).fix(add_required_fix(ctx, &tr))])
}

fn cache_resilience(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let caches: Vec<usize> = (0..ix.units.len()).filter(|&u| ix.unit_has_trait(u, "cache")).collect();
    if caches.is_empty() {
        return CheckOut::na();
    }
    let mode = p_str(rule, "mode").unwrap_or_else(|| "both".into());
    let mut out = vec![];
    let required_ok = p_str(rule, "requireTolerance").map(|r| tol_rank(ctx, &tolerance(ctx)) >= tol_rank(ctx, &r)).unwrap_or(true);
    let multi_site = ix.sites().len() >= 2 && required_ok;
    for &u in &caches {
        let unit = &ix.units[u];
        if mode != "cold" && multi_site && ix.unit_sites(u).len() < 2 {
            let mut raw = Raw::new(&unit.key, sev(rule)).var("unit", unit.name.clone());
            for &m in &unit.members {
                raw = raw.node(&nid(ctx, m));
            }
            out.push(raw.fix(add_member_fix(ctx, u)));
        }
    }
    // cold-cache load on the datastores
    if mode != "replication" && ix.arch.requirements.rps_peak.is_some() {
        let declared = ix.edge_eff.iter().any(|e| e.is_declared("fanoutColdCache"));
        if !declared {
            out.push(Raw::new("cold-cache-data", Status::NoData));
        } else {
            let min = ctx.cfg.settings.assumptions.min_capacity_during_failure;
            for &u in &caches {
                let sc = Scenario::Unit { unit: ix.units[u].key.clone() };
                let r = ctx.sim(&sc);
                if let Some(ratio) = r.capacity_ratio {
                    if ratio < min && r.state != ServiceState::Down {
                        out.push(
                            Raw::new(format!("{}:cold", ix.units[u].key), sev(rule))
                                .var("unit", ix.units[u].name.clone())
                                .var("ratio", (ratio * 100.0).round())
                                .var("bottleneck", r.bottleneck.as_ref().and_then(|k| ix.unit_ix.get(k)).map(|&b| ix.units[b].name.clone()).unwrap_or_default())
                                .scenario(sc),
                        );
                    }
                }
            }
        }
    }
    CheckOut::found(out)
}

fn group_hygiene(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    use std::collections::BTreeMap;
    let ix = &ctx.ix;
    // nodes of the same stateless kind fed by the same sources but belonging to different units
    let mut buckets: BTreeMap<(String, Vec<String>), Vec<usize>> = BTreeMap::new();
    for (ni, n) in ix.arch.nodes.iter().enumerate() {
        if ix.has_trait(ni, "entry") || ix.has_trait(ni, "stateful") || ix.units[ix.unit_of[ni]].behavior != Behavior::Stateless {
            continue;
        }
        let mut src: Vec<String> =
            ix.in_edges[ni].iter().filter(|&&ei| ix.sem(ei) == Some(Semantics::Dependency)).filter_map(|&ei| ix.edge_ends[ei].map(|(s, _)| ix.units[ix.unit_of[s]].key.clone())).collect();
        if src.is_empty() {
            continue;
        }
        src.sort();
        src.dedup();
        buckets.entry((n.kind.clone(), src)).or_default().push(ni);
    }
    let mut out = vec![];
    for ((_, _), nodes) in buckets {
        let units: BTreeSet<usize> = nodes.iter().map(|&n| ix.unit_of[n]).collect();
        if nodes.len() < 2 || units.len() < 2 {
            continue;
        }
        let key: Vec<String> = nodes.iter().map(|&n| nid(ctx, n)).collect();
        let group = nid(ctx, nodes[0]).trim_end_matches(|c: char| c.is_ascii_digit() || c == '-').to_string();
        let group = if group.is_empty() { "group".to_string() } else { group };
        let names: Vec<String> = nodes.iter().map(|&n| ix.node_name(n).to_string()).collect();
        let ops: Vec<Op> = nodes.iter().map(|&n| update_node(&ix.arch.nodes[n].id, vec![("group", json!(group))])).collect();
        let mut raw = Raw::new(key.join("+"), sev(rule)).var("nodes", names.join(", "));
        for id in &key {
            raw = raw.node(id);
        }
        out.push(raw.fix(Some(fix("group", &key.join("+"), json!({"nodes": names.join(", "), "group": group}), ops))));
    }
    CheckOut::found(out)
}

fn role_required(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let mut out = vec![];
    let mut any = false;
    for u in 0..ix.units.len() {
        if ix.units[u].behavior != Behavior::PrimaryReplica {
            continue;
        }
        let data = data_members(ctx, u);
        if data.len() < 2 {
            continue;
        }
        any = true;
        let primaries = data.iter().filter(|&&m| ix.arch.nodes[m].role.as_deref() == Some("primary")).count();
        if primaries != 1 {
            let unit = &ix.units[u];
            let mut raw = Raw::new(&unit.key, sev(rule)).var("unit", unit.name.clone()).var("count", primaries);
            for &m in &unit.members {
                raw = raw.node(&nid(ctx, m));
            }
            if primaries == 0 {
                let m = data[0];
                raw = raw.fix(Some(fix("set_role", &unit.key, json!({"node": ix.node_name(m)}), vec![update_node(&ix.arch.nodes[m].id, vec![("role", json!("primary"))])])));
            }
            out.push(raw);
        }
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}

// ---------------------------------------------------------------------------------------------
// product lifecycle
// ---------------------------------------------------------------------------------------------

/// A version a node declares and the release table that judges it.
struct VersionRef<'c> {
    /// The prop that holds it (`version`, `osVersion`).
    prop: String,
    set: &'c crate::lifecycle::VersionSet,
    /// What is being judged, for the message (`PostgreSQL`, `Ubuntu`).
    product: String,
}

/// The release table of the product a node runs, if the catalog has one. A variant may bring its own table (a managed
/// offering has its own lifecycle); an empty id switches the lifecycle off for it.
fn version_set<'c>(ctx: &'c Ctx<'_>, ni: usize) -> Option<&'c crate::lifecycle::VersionSet> {
    let k = ctx.ix.kind[ni]?;
    let id = ctx.ix.variant[ni].and_then(|v| v.versions.as_deref()).or(k.versions.as_deref()).unwrap_or(&k.id);
    ctx.versions.get(id).copied()
}

/// Every version a node can declare that a release table judges: its own `version` and the props marked `lifecycle`.
fn version_refs<'c>(ctx: &'c Ctx<'_>, ni: usize) -> Vec<VersionRef<'c>> {
    let ix = &ctx.ix;
    let mut out = vec![];
    let Some(k) = ix.kind[ni] else { return out };
    let lang = &ctx.cfg.lang;
    if let Some(set) = version_set(ctx, ni) {
        let own_table = ix.variant[ni].map(|v| v.versions.is_some()).unwrap_or(false);
        let product = if !set.name.is_empty() {
            set.name.clone()
        } else if own_table {
            ix.variant[ni].map(|v| v.name.get(lang)).unwrap_or_default()
        } else {
            k.name.get(lang)
        };
        out.push(VersionRef { prop: "version".into(), set, product });
    }
    for p in &k.props {
        let Some(l) = &p.lifecycle else { continue };
        let table = match l.strip_prefix('@') {
            Some(key) => ix.eff[ni].text(key),
            None => Some(l.clone()),
        };
        let Some(set) = table.and_then(|t| ctx.versions.get(t.as_str()).copied()) else { continue };
        let product = if set.name.is_empty() { set.id.clone() } else { set.name.clone() };
        out.push(VersionRef { prop: p.key.clone(), set, product });
    }
    out
}

/// Versions that no longer get security fixes, or soon will not. Only what the architect declared is judged: a version
/// the catalog assumes is not a statement about the real system.
fn version_lifecycle(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    use crate::lifecycle::Phase;
    let Some(today) = ctx.today else { return CheckOut::na() };
    let ix = &ctx.ix;
    let lc = &ix.settings.lifecycle;
    let warn = p_f64(rule, "warnDays").map(|d| d as i64).unwrap_or(lc.warn_days);
    let on_missing = p_str(rule, "onMissing").unwrap_or_else(|| "skip".into());
    let on_unknown = p_str(rule, "onUnknown").unwrap_or_else(|| "no_data".into());
    let mut out = vec![];
    let mut any = false;
    // tables that judged a declared version and are too old to be trusted: one finding per table
    let mut stale: BTreeMap<&str, (String, i64, Vec<String>)> = BTreeMap::new();
    for (ni, n) in ix.arch.nodes.iter().enumerate() {
        for r in version_refs(ctx, ni) {
            any = true;
            if n.props.text(&r.prop).is_some() {
                if let Some(age) = r.set.age_days(today).filter(|a| *a > lc.stale_days) {
                    let e = stale.entry(r.set.id.as_str()).or_insert_with(|| (r.product.clone(), age, vec![]));
                    e.2.push(n.id.clone());
                }
            }
            let key = format!("{}:{}", n.id, r.prop);
            let Some(version) = n.props.text(&r.prop) else {
                if on_missing == "no_data" {
                    out.push(Raw::new(key, Status::NoData).var("node", n.name.clone()).var("product", r.product.clone()).var("variant", "missing").node(&n.id).need("node", Some(&n.id), &r.prop));
                }
                continue;
            };
            let Some(l) = r.set.lifecycle(&version, today) else {
                if on_unknown == "no_data" {
                    out.push(Raw::new(key, Status::NoData).var("node", n.name.clone()).var("product", r.product.clone()).var("version", version.clone()).var("variant", "unknown").node(&n.id));
                }
                continue;
            };
            let rec = r.set.recommended(today, lc.prefer_lts, lc.mature_days);
            let base = |st: Status, variant: &str| {
                Raw::new(key.clone(), st)
                    .var("node", n.name.clone())
                    .var("product", r.product.clone())
                    .var("version", version.clone())
                    .var("cycle", l.cycle.clone())
                    .var("eol", l.eol.clone().map(Value::String).unwrap_or(Value::Null))
                    .var("days", l.days_to_eol.map(|d| json!(d)).unwrap_or(Value::Null))
                    .var("latest", l.latest.clone().map(Value::String).unwrap_or(Value::Null))
                    .var("recommended", rec.map(|c| json!(c.cycle)).unwrap_or(Value::Null))
                    .var("variant", variant)
                    .node(&n.id)
            };
            let fixf = |ctx: &Ctx<'_>| rec.filter(|c| c.cycle != l.cycle).map(|c| set_prop_fix(ctx, ni, &r.prop, json!(c.cycle), "set_version", json!({"recommended": c.cycle, "product": r.product})));
            match l.phase {
                Phase::Eol => out.push(base(sev(rule), if l.eol.is_some() { "expired" } else { "expired_undated" }).fix(fixf(ctx))),
                _ if l.days_to_eol.map(|d| d <= warn).unwrap_or(false) => out.push(base(Status::Risk, "expiring").fix(fixf(ctx))),
                _ => {}
            }
        }
    }
    for (table, (product, age, nodes)) in stale {
        let as_of = ctx.versions.get(table).map(|s| s.as_of.clone()).unwrap_or_default();
        let mut raw = Raw::new(format!("stale:{table}"), Status::NoData).var("product", product).var("asOf", as_of).var("days", age).var("variant", "stale");
        for id in nodes {
            raw = raw.node(&id);
        }
        out.push(raw);
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}

/// Members of one cluster on different release lines: replication between major versions does not work, and a
/// half-upgraded cluster is a state to leave, not to design.
fn version_skew(ctx: &Ctx<'_>, rule: &Rule) -> CheckOut {
    let ix = &ctx.ix;
    let mut out = vec![];
    let mut any = false;
    for u in 0..ix.units.len() {
        let unit = &ix.units[u];
        if unit.members.len() < 2 {
            continue;
        }
        let mut lines: Vec<(usize, String)> = vec![];
        for &m in &unit.members {
            if ix.is_witness(m) {
                continue;
            }
            let Some(v) = ix.arch.nodes[m].props.text("version") else { continue };
            let line = version_set(ctx, m).and_then(|s| s.cycle_of(&v)).map(|c| c.cycle.clone()).unwrap_or_else(|| v.clone());
            lines.push((m, line));
        }
        if lines.len() < 2 {
            continue;
        }
        any = true;
        let distinct: BTreeSet<&String> = lines.iter().map(|(_, l)| l).collect();
        if distinct.len() > 1 {
            let mut raw = Raw::new(&unit.key, sev(rule)).var("unit", unit.name.clone()).var("versions", distinct.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
            for (m, _) in &lines {
                raw = raw.node(&nid(ctx, *m));
            }
            out.push(raw);
        }
    }
    if any {
        CheckOut::found(out)
    } else {
        CheckOut::na()
    }
}
