//! Derived, read-only view of an [`Architecture`]: units (logical services), failure domains,
//! adjacency, dependency order and cycles. Built once per evaluation.

use crate::catalog::{Behavior, Catalog, Eff, EdgeTypeDef, KindDef, Semantics, ZoneLevel};
use crate::config::EngineSettings;
use crate::model::{Architecture, PropsExt};
use std::collections::{BTreeSet, HashMap};

/// A logical service: all nodes with the same `group` (or a single ungrouped node).
/// Members are alternatives: the unit serves as long as its behavior's condition holds.
#[derive(Debug, Clone)]
pub struct Unit {
    pub key: String,
    pub members: Vec<usize>,
    pub behavior: Behavior,
    pub traits: BTreeSet<String>,
    pub kind: String,
    pub name: String,
}

pub struct Index<'a> {
    pub arch: &'a Architecture,
    pub cat: &'a Catalog,
    pub settings: &'a EngineSettings,

    pub node_ix: HashMap<&'a str, usize>,
    pub zone_ix: HashMap<&'a str, usize>,
    pub kind: Vec<Option<&'a KindDef>>,
    pub eff: Vec<Eff>,
    pub traits: Vec<BTreeSet<String>>,

    pub unit_of: Vec<usize>,
    pub units: Vec<Unit>,
    pub unit_ix: HashMap<String, usize>,

    /// Ancestor-or-self chain of zone indices for every node's zone (innermost first).
    pub zone_chain: Vec<Vec<usize>>,
    /// Failure-domain site (index into `arch.zones`) of every node; `None` = global/external.
    pub site_of: Vec<Option<usize>>,
    /// Disaster-domain label of every node; `None` if unknown.
    pub geo_of: Vec<Option<String>>,

    pub out_edges: Vec<Vec<usize>>,
    pub in_edges: Vec<Vec<usize>>,
    pub edge_def: Vec<Option<&'a EdgeTypeDef>>,
    pub edge_eff: Vec<Eff>,
    pub edge_ends: Vec<Option<(usize, usize)>>,

    /// Units ordered so that dependencies come before dependents.
    pub unit_order: Vec<usize>,
    /// Cycles found in the unit dependency graph (each is a list of unit indices).
    pub cycles: Vec<Vec<usize>>,
    /// Unit-level dependency adjacency (u -> units it needs), including DCS references.
    pub unit_deps: Vec<Vec<usize>>,
    /// Units on the availability path: reachable from an entry unit through dependency edges.
    /// Failover of a unit off this path (e.g. a secrets store used only at start-up) does not
    /// interrupt the service and therefore does not count towards RTO.
    pub on_path: Vec<bool>,
}

impl<'a> Index<'a> {
    pub fn build(arch: &'a Architecture, cat: &'a Catalog, settings: &'a EngineSettings) -> Self {
        let node_ix: HashMap<&str, usize> = arch.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
        let zone_ix: HashMap<&str, usize> = arch.zones.iter().enumerate().map(|(i, z)| (z.id.as_str(), i)).collect();

        let kind: Vec<Option<&KindDef>> = arch.nodes.iter().map(|n| cat.kind(&n.kind)).collect();
        let eff: Vec<Eff> = arch
            .nodes
            .iter()
            .map(|n| Eff::build(cat.default_props(&n.kind), &n.props))
            .collect();
        let traits: Vec<BTreeSet<String>> = kind
            .iter()
            .map(|k| k.map(|k| k.traits.iter().cloned().collect()).unwrap_or_default())
            .collect();

        // Units
        let mut units: Vec<Unit> = Vec::new();
        let mut unit_ix: HashMap<String, usize> = HashMap::new();
        let mut unit_of = vec![0usize; arch.nodes.len()];
        for (i, n) in arch.nodes.iter().enumerate() {
            let key = n.group.clone().filter(|g| !g.is_empty()).unwrap_or_else(|| n.id.clone());
            let ui = *unit_ix.entry(key.clone()).or_insert_with(|| {
                units.push(Unit {
                    key: key.clone(),
                    members: vec![],
                    behavior: kind[i].map(|k| k.behavior).unwrap_or_default(),
                    traits: BTreeSet::new(),
                    kind: n.kind.clone(),
                    name: n.name.clone(),
                });
                units.len() - 1
            });
            units[ui].members.push(i);
            unit_of[i] = ui;
            for t in &traits[i] {
                units[ui].traits.insert(t.clone());
            }
        }

        // Zone chains / sites / geo
        let parent_of = |zi: usize| -> Option<usize> {
            arch.zones[zi].parent_id.as_deref().and_then(|p| zone_ix.get(p).copied())
        };
        let chain_of = |start: usize| -> Vec<usize> {
            let mut out = vec![start];
            let mut cur = start;
            let mut guard = 0;
            while let Some(p) = parent_of(cur) {
                if out.contains(&p) || guard > 64 {
                    break;
                }
                out.push(p);
                cur = p;
                guard += 1;
            }
            out
        };
        let level_of = |zi: usize| -> ZoneLevel {
            cat.zone_type(&arch.zones[zi].zone_type).map(|t| t.level).unwrap_or(ZoneLevel::Group)
        };

        let mut zone_chain: Vec<Vec<usize>> = Vec::with_capacity(arch.nodes.len());
        let mut site_of: Vec<Option<usize>> = Vec::with_capacity(arch.nodes.len());
        let mut geo_of: Vec<Option<String>> = Vec::with_capacity(arch.nodes.len());
        for n in &arch.nodes {
            let zi = n.zone_id.as_deref().and_then(|z| zone_ix.get(z).copied());
            match zi {
                None => {
                    zone_chain.push(vec![]);
                    site_of.push(None);
                    geo_of.push(None);
                }
                Some(zi) => {
                    let chain = chain_of(zi);
                    // site = nearest Site-level zone; else nearest Geo-level; else the zone itself
                    let site = chain
                        .iter()
                        .copied()
                        .find(|&c| level_of(c) == ZoneLevel::Site)
                        .or_else(|| chain.iter().copied().find(|&c| level_of(c) == ZoneLevel::Geo))
                        .unwrap_or(zi);
                    // innermost explicit `geo` prop wins; otherwise the nearest geo-level zone
                    let geo = chain.iter().copied().find_map(|c| {
                        arch.zones[c].props.text("geo").or_else(|| {
                            if level_of(c) == ZoneLevel::Geo { Some(arch.zones[c].id.clone()) } else { None }
                        })
                    });
                    zone_chain.push(chain);
                    site_of.push(Some(site));
                    geo_of.push(geo);
                }
            }
        }

        // Edges
        let mut out_edges = vec![vec![]; arch.nodes.len()];
        let mut in_edges = vec![vec![]; arch.nodes.len()];
        let mut edge_def = Vec::with_capacity(arch.edges.len());
        let mut edge_eff = Vec::with_capacity(arch.edges.len());
        let mut edge_ends = Vec::with_capacity(arch.edges.len());
        for (ei, e) in arch.edges.iter().enumerate() {
            let def = cat.edge_type(&e.edge_type);
            edge_def.push(def);
            edge_eff.push(Eff::build(def.map(|d| d.default_props.clone()).unwrap_or_default(), &e.props));
            match (node_ix.get(e.source.as_str()), node_ix.get(e.target.as_str())) {
                (Some(&s), Some(&t)) => {
                    out_edges[s].push(ei);
                    in_edges[t].push(ei);
                    edge_ends.push(Some((s, t)));
                }
                _ => edge_ends.push(None),
            }
        }

        // friendly unit names: common word prefix of the member names
        for u in units.iter_mut() {
            if u.members.len() > 1 {
                let names: Vec<&str> = u.members.iter().map(|&m| arch.nodes[m].name.as_str()).collect();
                let p = common_word_prefix(&names);
                u.name = if p.is_empty() { names[0].to_string() } else { p };
            }
        }

        let mut ix = Index {
            arch,
            cat,
            settings,
            node_ix,
            zone_ix,
            kind,
            eff,
            traits,
            unit_of,
            units,
            unit_ix,
            zone_chain,
            site_of,
            geo_of,
            out_edges,
            in_edges,
            edge_def,
            edge_eff,
            edge_ends,
            unit_order: vec![],
            cycles: vec![],
            unit_deps: vec![],
            on_path: vec![],
        };
        ix.compute_unit_order();
        ix
    }

    pub fn sem(&self, ei: usize) -> Option<Semantics> {
        self.edge_def[ei].map(|d| d.semantics)
    }

    /// Is this dependency edge *hard*? Explicit `hard` prop wins; otherwise edges into
    /// `soft-dependency` kinds (caches) are soft and everything else is hard.
    pub fn is_hard(&self, ei: usize) -> bool {
        if self.edge_eff[ei].is_declared("hard") {
            return self.edge_eff[ei].flag("hard").unwrap_or(true);
        }
        match self.edge_ends[ei] {
            Some((_, t)) => !self.traits[t].contains("soft-dependency"),
            None => true,
        }
    }

    /// Unit referenced by the `dcs` prop of any member (external quorum authority).
    pub fn dcs_unit(&self, unit: usize) -> Option<usize> {
        for &m in &self.units[unit].members {
            if let Some(k) = self.eff[m].text("dcs") {
                if let Some(&u) = self.unit_ix.get(&k) {
                    if u != unit {
                        return Some(u);
                    }
                }
            }
        }
        None
    }

    fn compute_unit_order(&mut self) {
        let nu = self.units.len();
        let mut deps: Vec<Vec<usize>> = vec![vec![]; nu];
        for u in 0..nu {
            for &m in &self.units[u].members {
                for &ei in &self.out_edges[m] {
                    if self.sem(ei) == Some(Semantics::Dependency) {
                        if let Some((_, t)) = self.edge_ends[ei] {
                            let v = self.unit_of[t];
                            if v != u && !deps[u].contains(&v) {
                                deps[u].push(v);
                            }
                        }
                    }
                }
            }
            if let Some(d) = self.dcs_unit(u) {
                if !deps[u].contains(&d) {
                    deps[u].push(d);
                }
            }
        }
        // iterative DFS, white/grey/black
        let mut color = vec![0u8; nu];
        let mut order = Vec::with_capacity(nu);
        let mut cycles: Vec<Vec<usize>> = Vec::new();
        for start in 0..nu {
            if color[start] != 0 {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
            let mut path: Vec<usize> = vec![start];
            color[start] = 1;
            while let Some(&mut (u, ref mut i)) = stack.last_mut() {
                if *i < deps[u].len() {
                    let v = deps[u][*i];
                    *i += 1;
                    match color[v] {
                        0 => {
                            color[v] = 1;
                            stack.push((v, 0));
                            path.push(v);
                        }
                        1 => {
                            if let Some(pos) = path.iter().position(|&p| p == v) {
                                let cyc: Vec<usize> = path[pos..].to_vec();
                                if !cycles.iter().any(|c| same_cycle(c, &cyc)) {
                                    cycles.push(cyc);
                                }
                            }
                        }
                        _ => {}
                    }
                } else {
                    color[u] = 2;
                    order.push(u);
                    stack.pop();
                    path.pop();
                }
            }
        }
        // availability path
        let entries: Vec<usize> = (0..nu).filter(|&u| self.units[u].traits.contains("entry")).collect();
        let mut on_path = vec![entries.is_empty(); nu];
        let mut stack = entries.clone();
        for &e in &entries {
            on_path[e] = true;
        }
        while let Some(u) = stack.pop() {
            for &v in &deps[u] {
                if !on_path[v] {
                    on_path[v] = true;
                    stack.push(v);
                }
            }
        }
        self.on_path = on_path;
        self.unit_order = order;
        self.cycles = cycles;
        self.unit_deps = deps;
    }

    // ---- convenience -------------------------------------------------------------------------

    pub fn node_name(&self, ni: usize) -> &str {
        &self.arch.nodes[ni].name
    }
    pub fn zone_name(&self, zi: usize) -> &str {
        &self.arch.zones[zi].name
    }
    pub fn has_trait(&self, ni: usize, t: &str) -> bool {
        self.traits[ni].contains(t)
    }
    pub fn unit_has_trait(&self, u: usize, t: &str) -> bool {
        self.units[u].traits.contains(t)
    }
    pub fn replicas(&self, ni: usize) -> u32 {
        self.eff[ni].num("replicas").map(|r| r.max(1.0) as u32).unwrap_or(1)
    }
    pub fn is_witness(&self, ni: usize) -> bool {
        self.arch.nodes[ni].role.as_deref() == Some("witness")
    }
    /// Requests per second one instance of `m` sustains: node override → kind default →
    /// `rpsPerVcpu × vcpu`. `None` = no capacity model for this node.
    pub fn per_replica_rps(&self, m: usize) -> Option<f64> {
        if let Some(v) = self.eff[m].num("rpsPerReplica") {
            return Some(v);
        }
        let cap = self.kind[m].and_then(|k| k.capacity.as_ref())?;
        if let Some(v) = cap.rps_per_replica {
            return Some(v);
        }
        match (cap.rps_per_vcpu, self.eff[m].num("vcpu")) {
            (Some(per), Some(v)) => Some(per * v),
            _ => None,
        }
    }

    pub fn zone_level(&self, zi: usize) -> ZoneLevel {
        self.cat
            .zone_type(&self.arch.zones[zi].zone_type)
            .map(|t| t.level)
            .unwrap_or(ZoneLevel::Group)
    }

    /// All site zone indices that host at least one node.
    pub fn sites(&self) -> Vec<usize> {
        let mut s: Vec<usize> = self.site_of.iter().flatten().copied().collect();
        s.sort_unstable();
        s.dedup();
        s
    }

    /// Distinct sites hosting members of a unit.
    pub fn unit_sites(&self, u: usize) -> Vec<usize> {
        let mut s: Vec<usize> = self.units[u].members.iter().filter_map(|&m| self.site_of[m]).collect();
        s.sort_unstable();
        s.dedup();
        s
    }

    /// Descendant-or-self zone indices of `zi`.
    pub fn zone_descendants(&self, zi: usize) -> Vec<usize> {
        let mut out = vec![zi];
        loop {
            let before = out.len();
            for (i, z) in self.arch.zones.iter().enumerate() {
                if out.contains(&i) {
                    continue;
                }
                if let Some(p) = z.parent_id.as_deref().and_then(|p| self.zone_ix.get(p)) {
                    if out.contains(p) {
                        out.push(i);
                    }
                }
            }
            if out.len() == before {
                break;
            }
        }
        out
    }
}

fn same_cycle(a: &[usize], b: &[usize]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let sa: BTreeSet<_> = a.iter().collect();
    let sb: BTreeSet<_> = b.iter().collect();
    sa == sb
}

fn common_word_prefix(names: &[&str]) -> String {
    let split: Vec<Vec<&str>> = names.iter().map(|n| n.split_whitespace().collect()).collect();
    let mut out: Vec<&str> = Vec::new();
    if let Some(first) = split.first() {
        for (i, w) in first.iter().enumerate() {
            if split.iter().all(|s| s.get(i) == Some(w)) {
                out.push(w);
            } else {
                break;
            }
        }
    }
    out.join(" ").trim_end_matches(|c: char| !c.is_alphanumeric() && c != ')').to_string()
}
