//! The interpreter: pattern + stack + request → architecture.

use super::model::*;
use crate::catalog::Behavior;
use crate::config::EngineConfig;
use crate::model::{unique_id, Architecture, Edge, Meta, Node, Props, PropsExt, Requirements, Zone};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SiteRole {
    Working,
    Standby,
    Witness,
    Backup,
}

#[derive(Clone, Debug)]
struct Site {
    id: String,
    name: String,
    role: SiteRole,
    geo: usize,
    zone_type: String,
    parent: Option<String>,
    racks: Vec<(String, String)>,
    /// Position among the sites of its disaster domain.
    pos: usize,
}

/// A place a node can live: a site, or one rack of a site when racks are modelled.
#[derive(Clone, Debug)]
struct Slot {
    site: usize,
    zone: String,
    /// Suffix of generated node ids.
    key: String,
    /// Suffix of generated node names.
    label: String,
    geo: usize,
    role: SiteRole,
    /// Index of the rack inside its site (0 when racks are not modelled).
    rack: usize,
}

#[derive(Clone, Debug)]
struct Member {
    node: usize,
    slot: Option<usize>,
    witness: bool,
}

struct Placed {
    members: Vec<Member>,
    /// Group (unit) of the layer's nodes – what `dcs` props refer to.
    group: String,
    /// Member the backup stream starts at.
    backup_src: Option<usize>,
}

struct Gen<'a> {
    cfg: &'a EngineConfig,
    pc: &'a PlacementConfig,
    pattern: &'a Pattern,
    lang: String,
    req: Requirements,
    k: u32,
    sites: Vec<Site>,
    slots: Vec<Slot>,
    /// Working slots in spread order (geo → site → rack round-robin), standby slots last.
    order: Vec<usize>,
    witness_slots: Vec<usize>,
    backup_slots: Vec<usize>,
    zones: Vec<Zone>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    taken: HashSet<String>,
    placed: BTreeMap<String, Placed>,
    notes: Vec<Note>,
    max_nodes: usize,
    site_type: Option<String>,
    /// The requirement reaches whole sites (not just racks): arbiters go to other sites.
    site_level: bool,
}

fn note(code: &str, level: &str, vars: Value) -> Note {
    Note { code: code.into(), level: level.into(), vars: vars.as_object().cloned().unwrap_or_default() }
}

fn err(code: &str, msg: impl Into<String>) -> PlacementError {
    PlacementError::new(code, msg)
}

fn tpl(t: &crate::text::LText, lang: &str, vars: &[(&str, String)]) -> String {
    let mut s = t.get(lang);
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// Odd number ≥ `n`.
fn odd_up(n: u32) -> u32 {
    if n % 2 == 0 {
        n + 1
    } else {
        n
    }
}

pub fn generate(cfg: &EngineConfig, pc: &PlacementConfig, req: &PlacementRequest) -> Result<PlacementResult, PlacementError> {
    let pattern = pc.pattern(&req.pattern).filter(|p| p.enabled).ok_or_else(|| err("unknown_pattern", format!("no placement pattern '{}'", req.pattern)))?;
    let lang = req.lang.clone().unwrap_or_else(|| cfg.lang.clone());

    // ---- numbers: sites, regions, racks, tolerated losses ----------------------------------------------------
    let n = req.sites.unwrap_or(pattern.default_sites);
    let max_sites = pattern.max_sites.unwrap_or(u32::MAX);
    if n < pattern.min_sites.max(1) || n > max_sites {
        return Err(err(
            "sites_out_of_range",
            format!("pattern '{}' takes {}..{} sites, got {}", pattern.id, pattern.min_sites, pattern.max_sites.map(|m| m.to_string()).unwrap_or_else(|| "∞".into()), n),
        ));
    }
    let regions = match pattern.regions.mode {
        RegionMode::Single => 1,
        RegionMode::PerSite => n,
        RegionMode::Count => req.regions.unwrap_or(pattern.regions.default).clamp(pattern.regions.min.max(1), pattern.regions.max.max(pattern.regions.min).max(1)).min(n),
    };
    let racks = req.racks.unwrap_or(pattern.racks.default).clamp(pattern.racks.min.max(1), pattern.racks.max.max(pattern.racks.min).max(1));
    let tolerance = if pattern.tolerance.is_empty() { cfg.settings.assumptions.default_failure_tolerance.clone() } else { pattern.tolerance.clone() };
    if cfg.settings.tolerance.level(&tolerance).is_none() {
        return Err(err("unknown_tolerance", format!("unknown tolerance level '{tolerance}'")));
    }
    let bound = cfg.settings.scenarios.max_site_failures as u32;
    let mut notes: Vec<Note> = vec![];
    let k_wanted = match (req.site_failures, pattern.site_failures.mode.as_str()) {
        (Some(k), _) => k,
        (None, "fixed") => pattern.site_failures.value,
        (None, _) => (n.saturating_sub(1) / 2).max(1).min(pattern.site_failures.max.max(1)),
    };
    let site_rank = cfg.settings.tolerance.rank_of_domain(crate::catalog::ZoneLevel::Site).unwrap_or(u32::MAX);
    let wants_sites = cfg.settings.tolerance.rank(&tolerance) >= site_rank;
    let k = if !wants_sites || n < 2 { 0 } else { k_wanted.min(n - 1).min(bound).max(1) };
    if wants_sites && n >= 2 && k != k_wanted {
        notes.push(note("site_failures_clamped", "info", json!({"wanted": k_wanted, "used": k, "sites": n})));
    }

    // ---- stack and layers -------------------------------------------------------------------------------------
    let stack_id = req.stack.clone().unwrap_or_else(|| pattern.stack.clone());
    let stack = pc.stack(&stack_id).ok_or_else(|| err("unknown_stack", format!("no stack '{stack_id}'")))?;
    let mut enabled: BTreeSet<String> = stack.layers.iter().filter(|l| !l.optional || req.layers.as_ref().map(|ls| ls.contains(&l.id)).unwrap_or(l.default)).map(|l| l.id.clone()).collect();
    loop {
        let before = enabled.len();
        for l in &stack.layers {
            if enabled.contains(&l.id) {
                for d in &l.needs {
                    if stack.layers.iter().any(|x| &x.id == d) {
                        enabled.insert(d.clone());
                    }
                }
            }
        }
        if enabled.len() == before {
            break;
        }
    }
    let mut kinds: BTreeMap<String, String> = BTreeMap::new();
    for l in &stack.layers {
        let kind = req.kinds.get(&l.id).cloned().unwrap_or_else(|| l.kind.clone());
        if kind != l.kind && !l.alternatives.contains(&kind) {
            return Err(err("bad_kind", format!("layer '{}' cannot use kind '{}'", l.id, kind)));
        }
        if cfg.catalog.kind(&kind).is_none() {
            return Err(err("unknown_kind", format!("kind '{kind}' is not in the catalog")));
        }
        kinds.insert(l.id.clone(), kind);
    }

    let mut requirements = pattern.requirements.clone();
    if let Some(r) = &req.requirements {
        merge_requirements(&mut requirements, r);
    }
    requirements.failure_tolerance = Some(tolerance.clone());
    if k >= 2 {
        requirements.site_failures = Some(k);
    }

    let mut g = Gen {
        cfg,
        pc,
        pattern,
        lang,
        req: requirements,
        k,
        sites: vec![],
        slots: vec![],
        order: vec![],
        witness_slots: vec![],
        backup_slots: vec![],
        zones: vec![],
        nodes: vec![],
        edges: vec![],
        taken: HashSet::new(),
        placed: BTreeMap::new(),
        notes,
        max_nodes: req.max_nodes.unwrap_or(pc.settings.max_nodes),
        site_type: req.site_type.clone(),
        site_level: wants_sites || cfg.settings.tolerance.covers(&tolerance, crate::catalog::ZoneLevel::Site),
    };
    g.build_topology(n, regions, racks, &tolerance)?;
    for layer in &stack.layers {
        if !enabled.contains(&layer.id) {
            continue;
        }
        let kind = &kinds[&layer.id];
        g.place_layer(layer, kind, &enabled, &kinds)?;
        if g.nodes.len() > g.max_nodes {
            return Err(err("too_many_nodes", format!("the design needs more than {} nodes", g.max_nodes)));
        }
    }
    g.place_singles(stack, &enabled, &kinds)?;
    g.wire(stack, &enabled)?;
    g.apply_dcs(stack, &enabled);
    g.secure_links();
    if g.nodes.len() > g.max_nodes {
        return Err(err("too_many_nodes", format!("the design has {} nodes, the limit is {}", g.nodes.len(), g.max_nodes)));
    }

    let name = req.name.clone().unwrap_or_else(|| format!("{} · {}", pattern.name.get(&g.lang), n));
    let arch = Architecture {
        schema_version: crate::model::SCHEMA_VERSION,
        meta: Meta { name: Some(name), description: pattern.description.as_ref().map(|d| d.get(&g.lang)) },
        requirements: g.req.clone(),
        zones: g.zones.clone(),
        nodes: g.nodes.clone(),
        edges: g.edges.clone(),
    };
    let sites = g
        .sites
        .iter()
        .map(|s| PlacedSite {
            id: s.id.clone(),
            name: s.name.clone(),
            role: match s.role {
                SiteRole::Working => "working",
                SiteRole::Standby => "standby",
                SiteRole::Witness => "witness",
                SiteRole::Backup => "backup",
            }
            .into(),
            geo: g.geo_label(s.geo),
            racks: s.racks.iter().map(|r| r.0.clone()).collect(),
        })
        .collect();
    Ok(PlacementResult { architecture: arch, sites, site_failures: g.k, notes: g.notes })
}

fn merge_requirements(base: &mut Requirements, over: &Requirements) {
    macro_rules! m {
        ($f:ident) => {
            if over.$f.is_some() {
                base.$f = over.$f.clone();
            }
        };
    }
    m!(rps_peak);
    m!(growth_percent);
    m!(rto_seconds);
    m!(rpo_seconds);
    m!(availability_slo);
    m!(budget_monthly);
    m!(notes);
    if !over.compliance.is_empty() {
        base.compliance = over.compliance.clone();
    }
}

impl<'a> Gen<'a> {
    fn st(&self) -> &'a PlacementSettings {
        &self.pc.settings
    }

    fn geo_label(&self, g: usize) -> String {
        format!("{}-{}", self.st().geo_prefix, g + 1)
    }

    fn id(&mut self, base: &str) -> String {
        let id = unique_id(base, &self.taken);
        self.taken.insert(id.clone());
        id
    }

    // ---------------------------------------------------------------------------------------------------------
    // topology
    // ---------------------------------------------------------------------------------------------------------

    fn build_topology(&mut self, n: u32, regions: u32, racks: u32, tolerance: &str) -> Result<(), PlacementError> {
        let p = self.pattern;
        let site_type = self.site_type.clone().filter(|t| !t.is_empty()).or_else(|| Some(p.site_type.clone()).filter(|t| !t.is_empty())).unwrap_or_else(|| "dc".to_string());
        if self.cfg.catalog.zone_type(&site_type).is_none() {
            return Err(err("unknown_zone_type", format!("zone type '{site_type}' is not in the catalog")));
        }
        let standby = p.standby_sites.min(n.saturating_sub(1));
        // geo of site i: contiguous blocks
        let geo_of = |i: u32| -> usize {
            match p.regions.mode {
                RegionMode::Single => 0,
                RegionMode::PerSite => i as usize,
                RegionMode::Count => (i as u64 * regions as u64 / n as u64) as usize,
            }
        };
        let region_zone = site_type == "az";
        let mut region_ids: Vec<String> = vec![];
        if region_zone {
            for r in 0..regions {
                let id = self.id(&if regions > 1 { format!("region{}", r + 1) } else { "region".to_string() });
                let name = tpl(&self.st().region_name, &self.lang, &[("n", (r + 1).to_string())]);
                self.zones.push(Zone { id: id.clone(), name, zone_type: self.st().region_zone_type.clone(), parent_id: None, role: None, props: Props::new(), ui: None });
                region_ids.push(id);
            }
        }
        let site_name_tpl = self.st().site_names.get(&site_type).cloned().unwrap_or_default();
        for i in 0..n {
            let id = self.id(&format!("{}{}", site_type, i + 1));
            let name = tpl(&site_name_tpl, &self.lang, &[("n", (i + 1).to_string())]);
            let role = if i >= n - standby { SiteRole::Standby } else { SiteRole::Working };
            let geo = geo_of(i);
            let pos = self.sites.iter().filter(|s| s.geo == geo).count();
            self.sites.push(Site { id, name, role, geo, zone_type: site_type.clone(), parent: region_ids.get(geo).cloned(), racks: vec![], pos });
        }
        // arbiter sites: enough locations that the votes outlive the planned losses
        let mut witnesses = 0u32;
        if p.witness && (self.k > 0 || tolerance == "geo" || self.cfg.settings.tolerance.covers(tolerance, crate::catalog::ZoneLevel::Geo)) {
            let locations_site = 2 * self.k + 1;
            witnesses = locations_site.saturating_sub(n);
            if self.cfg.settings.tolerance.covers(tolerance, crate::catalog::ZoneLevel::Geo) {
                // the biggest disaster domain must not hold half of the votes
                let mut per_geo: BTreeMap<usize, u32> = BTreeMap::new();
                for s in &self.sites {
                    *per_geo.entry(s.geo).or_default() += 1;
                }
                let biggest = per_geo.values().copied().max().unwrap_or(0);
                if !per_geo.is_empty() {
                    witnesses = witnesses.max((2 * biggest + 1).saturating_sub(n));
                }
            }
        }
        // an even number of locations splits into two halves without a majority: one more place for the deciding vote
        if p.witness && n >= 2 && (n + witnesses) % 2 == 0 {
            witnesses += 1;
        }
        let first_geo = regions as usize;
        for w in 0..witnesses {
            let id = self.id(&format!("witness{}", w + 1));
            let name = tpl(&self.st().witness_name, &self.lang, &[("n", (w + 1).to_string())]);
            self.sites.push(Site { id, name, role: SiteRole::Witness, geo: first_geo + w as usize, zone_type: self.st().witness_site_type.clone(), parent: None, racks: vec![], pos: 0 });
        }
        if witnesses > 0 {
            self.notes.push(note("witness_sites", "info", json!({"count": witnesses})));
        }
        let mut next_geo = first_geo + witnesses as usize;
        if p.offsite_backup_site {
            let id = self.id("backup-site");
            let name = tpl(&self.st().backup_site_name, &self.lang, &[]);
            self.sites.push(Site { id, name, role: SiteRole::Backup, geo: next_geo, zone_type: self.st().witness_site_type.clone(), parent: None, racks: vec![], pos: 0 });
            next_geo += 1;
        }
        let _ = next_geo;

        // racks
        if racks > 1 {
            for si in 0..self.sites.len() {
                if matches!(self.sites[si].role, SiteRole::Working | SiteRole::Standby) {
                    for r in 0..racks {
                        let zid = self.id(&format!("{}-r{}", self.sites[si].id, r + 1));
                        let name = tpl(&self.st().rack_name, &self.lang, &[("n", (r + 1).to_string()), ("site", self.sites[si].name.clone())]);
                        self.sites[si].racks.push((zid, name));
                    }
                }
            }
        }

        // zones: sites first, then their racks
        for s in self.sites.clone() {
            let mut props = Props::new();
            props.insert("geo".into(), json!(self.geo_label(s.geo)));
            let role = match s.role {
                SiteRole::Working => Some("active"),
                SiteRole::Standby => Some("standby"),
                SiteRole::Witness => Some("witness"),
                SiteRole::Backup => Some("backup"),
            };
            self.zones.push(Zone { id: s.id.clone(), name: s.name.clone(), zone_type: s.zone_type.clone(), parent_id: s.parent.clone(), role: role.map(String::from), props, ui: None });
            for (rid, rname) in &s.racks {
                self.zones.push(Zone { id: rid.clone(), name: rname.clone(), zone_type: self.st().rack_zone_type.clone(), parent_id: Some(s.id.clone()), role: None, props: Props::new(), ui: None });
            }
        }

        // slots
        let mut working: Vec<usize> = vec![];
        let max_racks = self.sites.iter().map(|s| s.racks.len().max(1)).max().unwrap_or(1);
        for r in 0..max_racks {
            for (si, s) in self.sites.clone().iter().enumerate() {
                if !matches!(s.role, SiteRole::Working | SiteRole::Standby) || r >= s.racks.len().max(1) {
                    continue;
                }
                let single_site = self.sites.iter().filter(|x| matches!(x.role, SiteRole::Working | SiteRole::Standby)).count() == 1;
                let short = tpl(&self.st().rack_short_name, &self.lang, &[("n", (r + 1).to_string())]);
                let (zone, label) = if s.racks.is_empty() {
                    (s.id.clone(), s.name.clone())
                } else if single_site && !short.is_empty() {
                    (s.racks[r].0.clone(), short)
                } else {
                    (s.racks[r].0.clone(), s.racks[r].1.clone())
                };
                self.slots.push(Slot { site: si, zone: zone.clone(), key: zone, label, geo: s.geo, role: s.role, rack: r });
                working.push(self.slots.len() - 1);
            }
        }
        for (si, s) in self.sites.clone().iter().enumerate() {
            if matches!(s.role, SiteRole::Witness | SiteRole::Backup) {
                self.slots.push(Slot { site: si, zone: s.id.clone(), key: s.id.clone(), label: s.name.clone(), geo: s.geo, role: s.role, rack: 0 });
                let ix = self.slots.len() - 1;
                if s.role == SiteRole::Witness {
                    self.witness_slots.push(ix);
                } else {
                    self.backup_slots.push(ix);
                }
            }
        }
        // spread order: standby last, then rack, then position of the site inside its disaster domain, then the domain
        let mut idx = working.clone();
        idx.sort_by_key(|&i| {
            let sl = &self.slots[i];
            (sl.role == SiteRole::Standby, sl.rack, self.sites[sl.site].pos, sl.geo)
        });
        self.order = idx;
        Ok(())
    }

    fn active_slots(&self) -> Vec<usize> {
        self.order.iter().copied().filter(|&s| self.slots[s].role == SiteRole::Working).collect()
    }

    fn link_profile(&self, a: Option<usize>, b: Option<usize>) -> &'a LinkProfile {
        match (a, b) {
            (Some(a), Some(b)) => {
                let (x, y) = (&self.slots[a], &self.slots[b]);
                if x.site == y.site {
                    &self.st().intra_site
                } else if x.geo == y.geo {
                    &self.st().intra_geo
                } else {
                    &self.st().inter_geo
                }
            }
            _ => &self.st().inter_geo,
        }
    }

    /// How far apart two slots are: 0 same slot, 1 same site, 2 same disaster domain, 3 elsewhere.
    fn dist(&self, a: Option<usize>, b: Option<usize>) -> u8 {
        match (a, b) {
            (Some(a), Some(b)) => {
                let (x, y) = (&self.slots[a], &self.slots[b]);
                if a == b {
                    0
                } else if x.site == y.site {
                    1
                } else if x.geo == y.geo {
                    2
                } else {
                    3
                }
            }
            _ => 3,
        }
    }

    // ---------------------------------------------------------------------------------------------------------
    // nodes and edges
    // ---------------------------------------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn add_node(&mut self, id_base: &str, kind: &str, name: String, slot: Option<usize>, group: Option<String>, role: Option<&str>, props: Props, witness: bool) -> Member {
        let id = self.id(id_base);
        let zone_id = slot.map(|s| self.slots[s].zone.clone());
        self.nodes.push(Node { id, kind: kind.to_string(), name, zone_id, group, role: role.map(String::from), props, ui: None, proposed: false });
        Member { node: self.nodes.len() - 1, slot, witness }
    }

    fn add_edge(&mut self, src: usize, dst: usize, edge_type: &str, props: Props, label: Option<String>) {
        let (s, t) = (self.nodes[src].id.clone(), self.nodes[dst].id.clone());
        let id = self.id(&format!("e-{s}--{t}"));
        self.edges.push(Edge { id, source: s, target: t, edge_type: edge_type.to_string(), label, props });
    }

    fn link_props(&self, a: Option<usize>, b: Option<usize>, asynchronous: bool) -> Props {
        let p = self.link_profile(a, b).clone();
        let mut out = Props::new();
        out.insert("latencyMs".into(), json!(p.latency_ms));
        out.insert("tls".into(), json!(true));
        out.insert("paths".into(), json!(p.paths));
        out.insert("bandwidthMbps".into(), json!(p.bandwidth_mbps));
        if asynchronous {
            out.insert("lagSeconds".into(), json!(p.lag_seconds));
        }
        out
    }

    fn per_replica(&self, kind: &str, props: &Props) -> Option<f64> {
        let cap = self.cfg.catalog.kind(kind)?.capacity.as_ref()?;
        cap.rps_per_replica.or_else(|| cap.rps_per_vcpu.zip(props.num("vcpu").or_else(|| self.cfg.catalog.default_props(kind).num("vcpu"))).map(|(per, v)| per * v))
    }

    /// Instances per slot so that the surviving slots carry the declared load with the configured headroom.
    fn replicas_for_load(&self, layer: &Layer, kind: &str, props: &Props, slots: usize) -> Option<u32> {
        if layer.sizing.mode != "load" {
            return None;
        }
        let peak = self.req.rps_peak?;
        let per = self.per_replica(kind, props)?;
        let a = &self.cfg.settings.assumptions;
        let share = if layer.sizing.share > 0.0 { layer.sizing.share } else { 1.0 };
        let growth = self.req.growth_percent.unwrap_or(0.0) / 100.0;
        let need = peak * (1.0 + growth) / (1.0 - a.headroom_percent / 100.0).max(0.05) * share;
        let floor = a.min_capacity_during_failure * peak * share;
        // losing a site loses all of its racks; with racks alone one rack may go
        let racks = self.sites.iter().filter(|s| s.role == SiteRole::Working).map(|s| s.racks.len().max(1)).max().unwrap_or(1);
        let lost = if self.k > 0 {
            self.k as usize * racks
        } else if racks > 1 {
            1
        } else {
            0
        };
        let n = slots.max(1);
        let after = n.saturating_sub(lost).max(1);
        let per_slot = (need / n as f64).max(floor / after as f64);
        Some(((per_slot / per).ceil() as u32).max(layer.sizing.min_replicas.max(1)))
    }

    fn layer_props(&self, layer: &Layer, kind: &str) -> Props {
        let mut p = layer.props.clone();
        // a prop the chosen kind does not know would only confuse the inspector
        if let Some(k) = self.cfg.catalog.kind(kind) {
            p.retain(|key, _| k.props.iter().any(|d| &d.key == key) || self.cfg.catalog.common_props.iter().any(|d| &d.key == key) || key == "rpsLabel");
        }
        p
    }

    // ---------------------------------------------------------------------------------------------------------
    // layers
    // ---------------------------------------------------------------------------------------------------------

    fn place_layer(&mut self, layer: &Layer, kind: &str, enabled: &BTreeSet<String>, kinds: &BTreeMap<String, String>) -> Result<(), PlacementError> {
        let def = self.cfg.catalog.kind(kind).ok_or_else(|| err("unknown_kind", kind))?.clone();
        let name = layer.name.get(&self.lang);
        let props = self.layer_props(layer, kind);
        let entry = def.traits.iter().any(|t| t == "entry");
        match layer.scope {
            LayerScope::Global => {
                let m = self.add_node(&layer.id, kind, name, None, if entry { None } else { Some(layer.id.clone()) }, None, props, false);
                self.placed.insert(layer.id.clone(), Placed { members: vec![m], group: layer.id.clone(), backup_src: None });
            }
            LayerScope::Single => {} // after the clusters: its place depends on theirs
            LayerScope::Site => {
                let slots: Vec<usize> = self.order.clone();
                let active = self.active_slots().len();
                let full = self.replicas_for_load(layer, kind, &props, active.max(1));
                let mut members = vec![];
                for s in slots {
                    let mut p = props.clone();
                    let standby = self.slots[s].role == SiteRole::Standby;
                    if let Some(r) = full {
                        if layer.sizing.hpa_min_ratio > 0.0 {
                            p.insert("hpaMin".into(), json!(((r as f64 * layer.sizing.hpa_min_ratio).ceil() as u32).max(2)));
                            p.insert("hpaMax".into(), json!(((r as f64 * layer.sizing.hpa_max_ratio.max(layer.sizing.hpa_min_ratio)).ceil() as u32).max(2)));
                        }
                        if standby {
                            p.insert("replicas".into(), json!(self.st().standby_min_replicas.max(layer.sizing.min_replicas).max(1)));
                            p.insert("failoverReplicas".into(), json!(r));
                            p.insert("scaleUpSeconds".into(), json!(self.st().standby_scale_up_seconds));
                        } else {
                            p.insert("replicas".into(), json!(r));
                        }
                    }
                    let id = format!("{}-{}", layer.id, self.slots[s].key);
                    let nm = format!("{} · {}", name, self.slots[s].label);
                    members.push(self.add_node(&id, kind, nm, Some(s), Some(layer.id.clone()), None, p, false));
                }
                self.placed.insert(layer.id.clone(), Placed { members, group: layer.id.clone(), backup_src: None });
            }
            LayerScope::Cluster => match def.behavior {
                Behavior::Stateless => self.place_stateless_cluster(layer, kind, &name, props),
                Behavior::PrimaryReplica => self.place_primary_replica(layer, kind, &name, props, enabled, kinds)?,
                Behavior::Consensus => self.place_consensus(layer, kind, &name, props),
                Behavior::QuorumLog => self.place_quorum_log(layer, kind, &name, props),
            },
        }
        Ok(())
    }

    fn ring(&mut self, members: &[Member], etype: &str, label: Option<String>) {
        let n = members.len();
        if n < 2 {
            return;
        }
        let pairs: Vec<(usize, usize)> = if n == 2 { vec![(0, 1)] } else { (0..n).map(|i| (i, (i + 1) % n)).collect() };
        for (i, (a, b)) in pairs.into_iter().enumerate() {
            let props = self.link_props(members[a].slot, members[b].slot, false);
            let mut props = props;
            props.remove("lagSeconds");
            self.add_edge(members[a].node, members[b].node, etype, props, if i == 0 { label.clone() } else { None });
        }
    }

    fn place_stateless_cluster(&mut self, layer: &Layer, kind: &str, name: &str, props: Props) {
        let mut members = vec![];
        let mut slots = self.order.clone();
        if layer.copies.max > 0 {
            slots.truncate(layer.copies.max as usize);
        }
        for s in slots {
            let id = format!("{}-{}", layer.id, self.slots[s].key);
            let nm = format!("{} · {}", name, self.slots[s].label);
            members.push(self.add_node(&id, kind, nm, Some(s), Some(layer.id.clone()), None, props.clone(), false));
        }
        self.ring(&members, "quorum", None);
        self.placed.insert(layer.id.clone(), Placed { members, group: layer.id.clone(), backup_src: None });
    }

    /// Up to `count` places for arbiters, each in a failure domain of its own and away from `used`: arbiter sites,
    /// the backup location and working sites first when whole sites are tolerated; racks when only racks are.
    fn independent_slots(&self, used: &[usize], count: usize) -> Vec<usize> {
        let used_sites: BTreeSet<usize> = used.iter().map(|&s| self.slots[s].site).collect();
        let used_zones: BTreeSet<&str> = used.iter().map(|&s| self.slots[s].zone.as_str()).collect();
        let far: Vec<usize> = self.witness_slots.iter().copied().chain(self.backup_slots.iter().copied()).collect();
        let sites: Vec<usize> = self.order.iter().copied().filter(|s| !used_sites.contains(&self.slots[*s].site)).collect();
        let racks: Vec<usize> = self.order.iter().copied().filter(|s| !used_zones.contains(self.slots[*s].zone.as_str())).collect();
        let passes: Vec<Vec<usize>> = if self.site_level { vec![far, sites, racks] } else { vec![racks, far, sites] };
        let mut out: Vec<usize> = vec![];
        for pass in passes {
            for c in pass {
                if out.len() >= count {
                    return out;
                }
                let site_free = !out.iter().any(|&o| self.slots[o].site == self.slots[c].site);
                let zone_free = !out.iter().any(|&o| self.slots[o].zone == self.slots[c].zone);
                if (self.site_level && site_free) || (!self.site_level && zone_free) {
                    out.push(c);
                }
            }
        }
        out
    }

    fn place_primary_replica(&mut self, layer: &Layer, kind: &str, name: &str, props: Props, enabled: &BTreeSet<String>, kinds: &BTreeMap<String, String>) -> Result<(), PlacementError> {
        let mut want = (self.k + 1).max(layer.copies.min.max(1));
        if layer.copies.max > 0 {
            want = want.min(layer.copies.max);
        }
        let total_slots = self.order.len() as u32;
        let copies = want.min(total_slots.max(1));
        if copies < want {
            self.notes.push(note("copies_limited", "warn", json!({"layer": layer.id, "wanted": want, "got": copies})));
        }
        if copies == 1 && self.k > 0 {
            self.notes.push(note("single_copy", "warn", json!({"layer": layer.id})));
        }
        // the copies: active slots in spread order, the last one in a standby site when there is one
        let active = self.active_slots();
        let standby: Vec<usize> = self.order.iter().copied().filter(|&s| self.slots[s].role == SiteRole::Standby).collect();
        let mut chosen: Vec<usize> =
            if copies >= 2 && !standby.is_empty() { active.iter().copied().take(copies as usize - 1).collect() } else { active.iter().copied().take(copies as usize).collect() };
        for s in standby.iter().copied().chain(self.order.iter().copied()) {
            if chosen.len() >= copies as usize {
                break;
            }
            if !chosen.contains(&s) {
                chosen.push(s);
            }
        }
        let def = self.cfg.catalog.kind(kind).cloned().ok_or_else(|| err("unknown_kind", kind))?;
        let has_dcs_prop = def.props.iter().any(|p| p.key == "dcs");
        let dcs_layer =
            layer.dcs.as_ref().filter(|d| enabled.contains(*d) && has_dcs_prop && kinds.get(*d).and_then(|k| self.cfg.catalog.kind(k)).map(|k| k.behavior == Behavior::Consensus).unwrap_or(false));
        let mut members: Vec<Member> = vec![];
        for (i, &s) in chosen.iter().enumerate() {
            let mut p = props.clone();
            // a standby copy is promoted by an operator – automatic failover would pull the pilot light into a split-brain
            if self.slots[s].role == SiteRole::Standby && def.props.iter().any(|d| d.key == "failover") {
                p.insert("failover".into(), json!("manual"));
            }
            let role = if i == 0 { "primary" } else { "replica" };
            let id = format!("{}-{}", layer.id, self.slots[s].key);
            let nm = format!("{} · {}", name, self.slots[s].label);
            members.push(self.add_node(&id, kind, nm, Some(s), Some(layer.id.clone()), Some(role), p, false));
        }
        // replication: the nearest replicas synchronously (as many as sites that may fail at once, so a synchronous
        // copy always survives) when they share the disaster domain and the round trip is short
        let primary_slot = members[0].slot;
        let mut by_dist: Vec<usize> = (1..members.len()).collect();
        by_dist.sort_by_key(|&i| (self.dist(primary_slot, members[i].slot), i));
        let sync_wanted = if self.pattern.sync_within_geo { self.k.max(1) as usize } else { 0 };
        let mut synced = 0usize;
        for &i in &by_dist {
            let near = self.slots_geo_equal(primary_slot, members[i].slot) && self.link_profile(primary_slot, members[i].slot).latency_ms <= self.cfg.settings.assumptions.sync_max_rtt_ms;
            let sync = synced < sync_wanted && near && self.nodes[members[i].node].props.get("failover").and_then(|v| v.as_str()) != Some("manual");
            if sync {
                synced += 1;
            }
            let mut props = self.link_props(primary_slot, members[i].slot, !sync);
            if sync {
                props.remove("lagSeconds");
            }
            self.add_edge(members[0].node, members[i].node, if sync { "sync" } else { "async" }, props, None);
        }
        // a quorum authority: the layer's DCS, or arbiters in places of their own (an odd number of votes in all)
        let mut backup_src = if members.len() > 1 { Some(members.len() - 1) } else { Some(0) };
        let defaults = self.cfg.catalog.default_props(kind);
        let auto = members.iter().any(|m| {
            let p = &self.nodes[m.node].props;
            p.get("failover").or_else(|| defaults.get("failover")).and_then(|v| v.as_str()) == Some("auto")
        });
        if dcs_layer.is_none() && copies >= 2 && auto {
            let used: Vec<usize> = members.iter().filter_map(|m| m.slot).collect();
            let want = if copies % 2 == 0 { 1 } else { 2 };
            let spots = self.independent_slots(&used, want);
            if spots.is_empty() {
                // nobody can arbitrate: failing over by itself would risk two writers
                for m in &members {
                    if def.props.iter().any(|d| d.key == "failover") {
                        self.nodes[m.node].props.insert("failover".into(), json!("manual"));
                    }
                }
                self.notes.push(note("no_authority", "warn", json!({"layer": layer.id})));
            } else {
                for ws in spots.iter().copied() {
                    let mut p = self.st().witness_props.clone();
                    if let Some(mode) = props.get("mode") {
                        p.insert("mode".into(), mode.clone());
                    }
                    let id = format!("{}-witness-{}", layer.id, self.slots[ws].key);
                    let nm = format!("{} · {}", name, self.slots[ws].label);
                    let w = self.add_node(&id, kind, nm, Some(ws), Some(layer.id.clone()), Some("witness"), p, true);
                    for m in members.iter().filter(|m| !m.witness).cloned().collect::<Vec<_>>() {
                        let mut props = self.link_props(w.slot, m.slot, false);
                        props.remove("lagSeconds");
                        self.add_edge(w.node, m.node, "quorum", props, None);
                    }
                    members.push(w);
                }
                if spots.len() < want {
                    self.notes.push(note("arbiters_share_locations", "warn", json!({"layer": layer.id, "wanted": want, "got": spots.len()})));
                }
            }
        }
        if backup_src == Some(0) && members.len() > 1 {
            backup_src = Some(1);
        }
        self.placed.insert(layer.id.clone(), Placed { members, group: layer.id.clone(), backup_src });
        Ok(())
    }

    fn slots_geo_equal(&self, a: Option<usize>, b: Option<usize>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => self.slots[a].geo == self.slots[b].geo,
            _ => false,
        }
    }

    fn place_consensus(&mut self, layer: &Layer, kind: &str, name: &str, props: Props) {
        let st = self.st().clone();
        let voters = odd_up((2 * self.k + 1).max(st.min_voters.max(1))).min(odd_up(st.max_voters.max(1)));
        // locations: working slots in spread order, then the arbiter sites
        let mut loc: Vec<usize> = self.active_slots();
        loc.extend(self.order.iter().copied().filter(|s| self.slots[*s].role == SiteRole::Standby));
        loc.extend(self.witness_slots.iter().copied());
        // a design that tolerates only racks may still need a place outside them for the deciding vote
        if (loc.len() as u32) < voters {
            loc.extend(self.backup_slots.iter().copied());
        }
        if loc.is_empty() {
            return;
        }
        // prefer to use the arbiter sites first when there are only a few working locations
        let mut count: BTreeMap<usize, u32> = BTreeMap::new();
        let working = self.order.len();
        let mut picked: Vec<usize> = vec![];
        for i in 0..voters as usize {
            // fill every distinct location once before doubling up
            let s = if i < loc.len() { loc[i] } else { loc[i % loc.len()] };
            *count.entry(s).or_default() += 1;
            if !picked.contains(&s) {
                picked.push(s);
            }
        }
        let _ = working;
        if voters as usize > loc.len() {
            self.notes.push(note("voters_share_locations", "warn", json!({"layer": layer.id, "voters": voters, "locations": loc.len()})));
        }
        let mut members = vec![];
        for s in picked {
            let mut p = props.clone();
            let c = count[&s];
            if c > 1 {
                p.insert("replicas".into(), json!(c));
            }
            let witness = matches!(self.slots[s].role, SiteRole::Witness | SiteRole::Backup);
            let id = format!("{}-{}", layer.id, self.slots[s].key);
            let nm = format!("{} · {}", name, self.slots[s].label);
            members.push(self.add_node(&id, kind, nm, Some(s), Some(layer.id.clone()), Some(if witness { "witness" } else { "voter" }), p, witness));
        }
        self.ring(&members, "quorum", None);
        self.placed.insert(layer.id.clone(), Placed { members, group: layer.id.clone(), backup_src: None });
    }

    fn place_quorum_log(&mut self, layer: &Layer, kind: &str, name: &str, props: Props) {
        // one cluster per disaster domain: a stretched cluster only works within a metro-sized distance
        let mut geos: Vec<usize> = vec![];
        for &s in &self.order {
            if !geos.contains(&self.slots[s].geo) {
                geos.push(self.slots[s].geo);
            }
        }
        let multi = geos.len() > 1;
        let mut firsts: Vec<Member> = vec![];
        let mut all: Vec<Member> = vec![];
        let mut group_of_first = layer.id.clone();
        for (gi, geo) in geos.iter().enumerate() {
            let mut locs: Vec<usize> = self.order.iter().copied().filter(|&s| self.slots[s].geo == *geo).collect();
            if layer.copies.max > 0 {
                locs.truncate(layer.copies.max as usize);
            }
            // two locations cannot hold a majority after losing one: add a tie-breaker from an arbiter site
            let mut tiebreak: Option<usize> = None;
            if locs.len() % 2 == 0 && !multi {
                tiebreak = self.witness_slots.first().copied();
            }
            let group = if multi { format!("{}-{}", layer.id, gi + 1) } else { layer.id.clone() };
            if gi == 0 {
                group_of_first = group.clone();
            }
            let mut members = vec![];
            for s in locs.into_iter().chain(tiebreak) {
                let id = format!("{}-{}", layer.id, self.slots[s].key);
                let nm = format!("{} · {}", name, self.slots[s].label);
                let witness = self.slots[s].role == SiteRole::Witness;
                let mut p = props.clone();
                // one broker per location unless the stack says otherwise (the catalog default would be a whole cluster)
                p.entry("replicas".into()).or_insert(json!(1));
                if witness {
                    // a tie-breaker holds a copy but is sized down
                    p.insert("vcpu".into(), self.st().witness_props.get("vcpu").cloned().unwrap_or(json!(2)));
                }
                members.push(self.add_node(&id, kind, nm, Some(s), Some(group.clone()), None, p, false));
            }
            // an even number of brokers can split into two halves without a majority: one more in the first location
            let total: u32 = members.iter().map(|m| self.nodes[m.node].props.num("replicas").map(|r| r as u32).unwrap_or(1).max(1)).sum();
            if total % 2 == 0 {
                if let Some(first) = members.first() {
                    let cur = self.nodes[first.node].props.num("replicas").map(|r| r as u32).unwrap_or(1).max(1);
                    self.nodes[first.node].props.insert("replicas".into(), json!(cur + 1));
                }
            }
            self.ring(&members, "quorum", None);
            if let Some(f) = members.first() {
                firsts.push(f.clone());
            }
            all.extend(members);
        }
        // clusters in different disaster domains are kept in step asynchronously
        if firsts.len() > 1 {
            for i in 1..firsts.len() {
                let props = self.link_props(firsts[i - 1].slot, firsts[i].slot, true);
                self.add_edge(firsts[i - 1].node, firsts[i].node, "async", props, None);
            }
            self.notes.push(note("cluster_per_domain", "info", json!({"layer": layer.id, "clusters": firsts.len()})));
        }
        self.placed.insert(layer.id.clone(), Placed { members: all, group: group_of_first, backup_src: None });
    }

    /// Layers with one node (`single`): arbiters' neighbours, GitOps, the backup storage…
    fn place_singles(&mut self, stack: &Stack, enabled: &BTreeSet<String>, kinds: &BTreeMap<String, String>) -> Result<(), PlacementError> {
        for layer in stack.layers.iter().filter(|l| l.scope == LayerScope::Single && enabled.contains(&l.id)) {
            let kind = &kinds[&layer.id];
            let name = layer.name.get(&self.lang);
            let props = self.layer_props(layer, kind);
            let slot = match layer.place.as_deref().unwrap_or("last") {
                "first" => self.order.first().copied(),
                "witness" => self.witness_slots.first().copied().or_else(|| self.order.last().copied()),
                "offsite" => self.offsite_slot(stack, &layer.id),
                _ => self.order.iter().rev().find(|&&s| self.slots[s].role == SiteRole::Working).copied().or_else(|| self.order.last().copied()),
            };
            let id = match slot {
                Some(s) => format!("{}-{}", layer.id, self.slots[s].key),
                None => layer.id.clone(),
            };
            let nm = match slot {
                Some(s) if self.sites.len() > 1 => format!("{} · {}", name, self.slots[s].label),
                _ => name,
            };
            let m = self.add_node(&id, kind, nm, slot, Some(layer.id.clone()), None, props, false);
            self.placed.insert(layer.id.clone(), Placed { members: vec![m], group: layer.id.clone(), backup_src: None });
        }
        Ok(())
    }

    /// Location for a backup target: away from the sites (and, if possible, disaster domains) of what it protects.
    fn offsite_slot(&self, stack: &Stack, target: &str) -> Option<usize> {
        let sources: Vec<Option<usize>> = stack
            .layers
            .iter()
            .find(|l| l.id == target)
            .map(|t| {
                t.from
                    .iter()
                    .filter(|f| f.edge_type.as_deref() == Some("backup"))
                    .filter_map(|f| self.placed.get(&f.layer))
                    .map(|p| p.backup_src.and_then(|i| p.members.get(i)).and_then(|m| m.slot))
                    .collect()
            })
            .unwrap_or_default();
        let mut cands: Vec<usize> = self.backup_slots.clone();
        cands.extend(self.witness_slots.iter().copied());
        cands.extend(self.order.iter().rev().copied());
        let score = |c: usize| -> (usize, usize) {
            let diff_site = sources.iter().filter(|s| self.dist(**s, Some(c)) >= 2 || s.map(|x| self.slots[x].site != self.slots[c].site).unwrap_or(true)).count();
            let diff_geo = sources.iter().filter(|s| self.dist(**s, Some(c)) >= 3).count();
            (diff_geo, diff_site)
        };
        let mut best: Option<(usize, (usize, usize))> = None;
        for c in cands {
            let sc = score(c);
            if best.map(|b| sc > b.1).unwrap_or(true) {
                best = Some((c, sc));
            }
        }
        best.map(|b| b.0)
    }

    // ---------------------------------------------------------------------------------------------------------
    // wiring
    // ---------------------------------------------------------------------------------------------------------

    fn nearest(&self, from: &Member, to: &[Member]) -> Option<usize> {
        to.iter().enumerate().filter(|(_, t)| !t.witness).min_by_key(|(i, t)| (self.dist(from.slot, t.slot), *i)).map(|(i, _)| i)
    }

    fn wire(&mut self, stack: &Stack, enabled: &BTreeSet<String>) -> Result<(), PlacementError> {
        for layer in stack.layers.iter().filter(|l| enabled.contains(&l.id)) {
            for link in &layer.from {
                if link.unless.as_ref().map(|u| self.placed.contains_key(u)).unwrap_or(false) {
                    continue;
                }
                let (Some(src), Some(dst)) = (self.placed.get(&link.layer), self.placed.get(&layer.id)) else { continue };
                let (src_members, dst_members) = (src.members.clone(), dst.members.clone());
                let src_backup = src.backup_src;
                let etype = link.edge_type.clone().unwrap_or_else(|| "traffic".into());
                let label = link.label.as_ref().map(|l| l.get(&self.lang));
                let mut pairs: Vec<(usize, usize)> = vec![];
                match link.wire {
                    Wire::All => {
                        for s in src_members.iter().filter(|m| !m.witness) {
                            for t in dst_members.iter().filter(|m| !m.witness) {
                                pairs.push((s.node, t.node));
                            }
                        }
                    }
                    Wire::Same => {
                        if etype == "backup" {
                            // one stream from the replica the backup is taken from
                            if let Some(s) = src_backup.and_then(|i| src_members.get(i)).or_else(|| src_members.first()) {
                                if let Some(ti) = self.nearest(s, &dst_members) {
                                    pairs.push((s.node, dst_members[ti].node));
                                }
                            }
                        } else {
                            for s in src_members.iter().filter(|m| !m.witness) {
                                if let Some(ti) = self.nearest(s, &dst_members) {
                                    pairs.push((s.node, dst_members[ti].node));
                                }
                            }
                        }
                    }
                    Wire::EachTarget => {
                        for t in dst_members.iter().filter(|m| !m.witness) {
                            let from = src_members.iter().enumerate().filter(|(_, s)| !s.witness).min_by_key(|(i, s)| (self.dist(t.slot, s.slot), *i)).map(|(i, _)| i);
                            if let Some(si) = from {
                                pairs.push((src_members[si].node, t.node));
                            }
                        }
                    }
                }
                for (s, t) in pairs {
                    let mut props = link.props.clone();
                    if etype == "backup" {
                        let sm = src_members.iter().find(|m| m.node == s).cloned();
                        let tm = dst_members.iter().find(|m| m.node == t).cloned();
                        if let (Some(a), Some(b)) = (sm, tm) {
                            let lp = self.link_props(a.slot, b.slot, false);
                            for (k, v) in lp {
                                if matches!(k.as_str(), "latencyMs" | "bandwidthMbps") {
                                    props.entry(k).or_insert(v);
                                }
                            }
                        }
                    }
                    self.add_edge(s, t, &etype, props, label.clone());
                }
            }
        }
        Ok(())
    }

    /// Traffic between sites is encrypted unless the stack already decided otherwise.
    fn secure_links(&mut self) {
        if !self.st().tls_between_sites {
            return;
        }
        let site_of: BTreeMap<String, Option<String>> = self.nodes.iter().map(|n| (n.id.clone(), n.zone_id.clone())).collect();
        let sites: BTreeMap<String, String> = self.slots.iter().map(|s| (s.zone.clone(), self.sites[s.site].id.clone())).collect();
        let site = |node: &str| -> Option<String> { site_of.get(node).cloned().flatten().and_then(|z| sites.get(&z).cloned()) };
        for e in self.edges.iter_mut() {
            if let (Some(a), Some(b)) = (site(&e.source), site(&e.target)) {
                if a != b {
                    e.props.entry("tls".into()).or_insert(json!(true));
                }
            }
        }
    }

    /// Point the members of every layer at the consensus cluster that arbitrates them.
    fn apply_dcs(&mut self, stack: &Stack, enabled: &BTreeSet<String>) {
        for layer in stack.layers.iter().filter(|l| enabled.contains(&l.id)) {
            let Some(d) = layer.dcs.as_ref() else { continue };
            let (Some(p), Some(dp)) = (self.placed.get(&layer.id), self.placed.get(d)) else { continue };
            let group = dp.group.clone();
            let members: Vec<usize> = p.members.iter().filter(|m| !m.witness).map(|m| m.node).collect();
            for ni in members {
                let kind = self.nodes[ni].kind.clone();
                if self.cfg.catalog.prop_def(&kind, "dcs").is_some() {
                    self.nodes[ni].props.insert("dcs".into(), Value::String(group.clone()));
                }
            }
        }
    }
}
