//! Deterministic failure simulator.
//!
//! Given an architecture and a failure scenario (instance, node, zone/site, network cut between
//! sites, several sites, a whole disaster domain) it computes the steady state after all
//! *automatic* recovery, the interruption time, the data-loss window (RPO), split-brain risk,
//! the remaining capacity and an explanation chain. No LLM is involved: results are
//! reproducible and every value says whether it was declared or assumed.
//!
//! Model summary
//! * A node belongs to a zone; a *site* (DC/AZ/cloud) is the unit of power/network failure and
//!   of network partition. Nodes without a zone are global (users, anycast DNS) and reach
//!   everything.
//! * Nodes sharing a `group` form a *unit*; the unit's behavior defines when it serves.
//! * Connectivity between sites is **pairwise**: drawn edges plus an implicit mesh between
//!   sites that host members of one unit (configurable). A `Link` scenario cuts one pair; no
//!   transit through a third site is assumed unless `transitRouting` is enabled.
//! * A node is *effective* when it is alive and every **hard** dependency has a serving member
//!   it can reach. The service is up when the entry points are served.

use crate::catalog::{Behavior, Semantics};
use crate::graph::Index;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

fn one_u32() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Scenario {
    /// Normal operation (baseline: loads, capacity).
    None,
    /// Lose `count` instances of one node (a pod, a VM). Crash, data survives.
    Instance {
        node_id: String,
        #[serde(default = "one_u32")]
        count: u32,
    },
    /// The whole node (all its instances) goes down. Crash, data survives.
    Node { node_id: String },
    /// Every member of a unit (cluster) goes down at once. Crash, data survives.
    Unit { unit: String },
    /// A zone (rack / site / geo) is destroyed: everything inside is lost.
    Zone { zone_id: String },
    /// Network partition between two sites; both sides keep running.
    Link { a: String, b: String },
    /// Several sites lose the network to every site outside the group but keep running and keep talking to each other
    /// (a WAN uplink or peering outage). With three or more sites this is the failure a single cut link never shows.
    Isolate { zone_ids: Vec<String> },
    /// Several zones at once.
    Sites { zone_ids: Vec<String> },
    /// Every site of a disaster domain (`geo` label) is destroyed.
    Geo { geo: String },
}

impl Scenario {
    pub fn key(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn is_destructive(&self) -> bool {
        matches!(self, Scenario::Zone { .. } | Scenario::Sites { .. } | Scenario::Geo { .. })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Up,
    Degraded,
    Down,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    /// No interruption.
    None,
    /// Recovers automatically after `seconds`.
    Auto,
    /// Needs a human (manual failover / restore from backup) – takes `seconds`.
    Manual,
    /// Does not recover without repairing the failed component.
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub kind: RecoveryKind,
    pub seconds: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Up,
    /// Some instances lost, node still serves.
    Partial,
    Down,
    /// Alive but cannot serve: a hard dependency is unreachable.
    Stalled,
    /// Primary that stepped down because it lost the quorum.
    Fenced,
    /// Replica promoted to writer.
    Promoted,
    /// Alive, not serving, on the minority side of a partition.
    Isolated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeState {
    Ok,
    Cut,
    Dead,
}

/// One step of the explanation chain. `code` is a stable identifier translated by the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub vars: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitLoad {
    pub unit: String,
    pub load_rps: f64,
    pub capacity_rps: Option<f64>,
    pub ratio: Option<f64>,
}

/// Data-loss window caused by one unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpoItem {
    pub unit: String,
    /// `None` = cannot be determined (e.g. a replica without a replication link).
    pub seconds: Option<f64>,
    pub assumed: bool,
    pub traits: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimResult {
    pub scenario: Scenario,
    pub state: ServiceState,
    pub recovery: Recovery,
    /// Worst-case data-loss window. `None` = cannot be determined from the model (no data).
    pub rpo_seconds: Option<f64>,
    /// True if some RPO value comes from an assumption rather than a declared property.
    pub rpo_assumed: bool,
    /// Per-unit contributions to the RPO (rules may exclude some traits, e.g. queues).
    pub rpo_items: Vec<RpoItem>,
    /// Units whose data is gone with no replica or backup.
    pub data_lost: Vec<String>,
    /// Units that may have two writers at once.
    pub split_brain: Vec<String>,
    /// Remaining capacity relative to the peak load (1.0 = exactly peak). `None` = unknown.
    pub capacity_ratio: Option<f64>,
    pub bottleneck: Option<String>,
    pub unit_loads: Vec<UnitLoad>,
    /// Display names of units that cannot serve, in dependency order (root causes first).
    pub down_units: Vec<String>,
    pub events: Vec<Event>,
    pub node_states: BTreeMap<String, NodeState>,
    pub edge_states: BTreeMap<String, EdgeState>,
    /// Names of the failed zones / nodes for display.
    pub failed: Vec<String>,
}

// -------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Authority {
    None,
    External(usize),
    SelfVotes,
}

struct Sim<'a, 'b> {
    ix: &'b Index<'a>,
    alive: Vec<u32>,
    /// `link[a][b]`: sites a and b can talk (zone indices).
    link: Vec<Vec<bool>>,
    destructive: bool,
    eff_ok: Vec<bool>,
    degraded: Vec<bool>,
    serving: Vec<Vec<usize>>,
    done: Vec<bool>,
    unit_lost_members: Vec<bool>,
    fenced: Vec<bool>,
    promoted: Vec<bool>,
    isolated: Vec<bool>,
    events: Vec<Event>,
    auto_tracks: Vec<f64>,
    manual_tracks: Vec<f64>,
    restore_tracks: Vec<f64>,
    rpo_items: Vec<RpoItem>,
    split_brain: Vec<String>,
    data_lost: Vec<String>,
    cut: Vec<(usize, usize)>,
    failed: Vec<String>,
}

fn ev(code: &str, unit: Option<&str>, node: Option<&str>, vars: Value) -> Event {
    Event { code: code.into(), unit: unit.map(|s| s.into()), node: node.map(|s| s.into()), vars: vars.as_object().cloned().unwrap_or_default() }
}

impl<'a, 'b> Sim<'a, 'b> {
    fn new(ix: &'b Index<'a>, sc: &Scenario) -> Self {
        let n = ix.arch.nodes.len();
        let nz = ix.arch.zones.len();
        let mut down_zone = vec![false; nz];
        let mut lost = vec![0u32; n];
        let mut cut: Vec<(usize, usize)> = Vec::new();
        let mut failed: Vec<String> = Vec::new();

        let fail_zone = |zi: usize, dz: &mut Vec<bool>, failed: &mut Vec<String>| {
            for d in ix.zone_descendants(zi) {
                dz[d] = true;
            }
            failed.push(ix.arch.zones[zi].name.clone());
        };

        match sc {
            Scenario::None => {}
            Scenario::Instance { node_id, count } => {
                if let Some(&ni) = ix.node_ix.get(node_id.as_str()) {
                    lost[ni] = *count;
                    failed.push(format!("{} ×{}", ix.arch.nodes[ni].name, count));
                }
            }
            Scenario::Node { node_id } => {
                if let Some(&ni) = ix.node_ix.get(node_id.as_str()) {
                    lost[ni] = u32::MAX;
                    failed.push(ix.arch.nodes[ni].name.clone());
                }
            }
            Scenario::Unit { unit } => {
                if let Some(&u) = ix.unit_ix.get(unit.as_str()) {
                    for &m in &ix.units[u].members {
                        lost[m] = u32::MAX;
                    }
                    failed.push(ix.units[u].name.clone());
                }
            }
            Scenario::Zone { zone_id } => {
                if let Some(&zi) = ix.zone_ix.get(zone_id.as_str()) {
                    fail_zone(zi, &mut down_zone, &mut failed);
                }
            }
            Scenario::Sites { zone_ids } => {
                for z in zone_ids {
                    if let Some(&zi) = ix.zone_ix.get(z.as_str()) {
                        fail_zone(zi, &mut down_zone, &mut failed);
                    }
                }
            }
            Scenario::Geo { geo } => {
                let mut seen = vec![];
                for (ni, g) in ix.geo_of.iter().enumerate() {
                    if g.as_deref() == Some(geo.as_str()) {
                        if let Some(si) = ix.site_of[ni] {
                            if !seen.contains(&si) {
                                seen.push(si);
                            }
                        }
                    }
                }
                for si in seen {
                    fail_zone(si, &mut down_zone, &mut failed);
                }
            }
            Scenario::Link { a, b } => {
                if let (Some(&za), Some(&zb)) = (ix.zone_ix.get(a.as_str()), ix.zone_ix.get(b.as_str())) {
                    cut.push((za, zb));
                    failed.push(format!("{} ↔ {}", ix.arch.zones[za].name, ix.arch.zones[zb].name));
                }
            }
            Scenario::Isolate { zone_ids } => {
                let island: Vec<usize> = zone_ids.iter().filter_map(|z| ix.zone_ix.get(z.as_str()).copied()).collect();
                for &inside in &island {
                    for outside in ix.sites() {
                        if !island.contains(&outside) {
                            cut.push((inside, outside));
                        }
                    }
                    failed.push(ix.arch.zones[inside].name.clone());
                }
            }
        }

        let mut alive = vec![0u32; n];
        let site_failure = down_zone.iter().any(|&d| d);
        let mut scale_out: Vec<(usize, f64)> = Vec::new();
        for i in 0..n {
            let zone_down = ix.zone_chain[i].iter().any(|&z| down_zone[z]);
            let mut base = ix.replicas(i);
            // pilot light / warm standby: surviving nodes scale out when another site is lost
            if site_failure && !zone_down {
                if let Some(fr) = ix.eff[i].num("failoverReplicas") {
                    if fr as u32 > base {
                        base = fr as u32;
                        let secs = ix.eff[i].num("scaleUpSeconds").unwrap_or(ix.settings.assumptions.default_scale_up_seconds);
                        scale_out.push((i, secs));
                    }
                }
            }
            alive[i] = if zone_down { 0 } else { base.saturating_sub(lost[i]) };
        }

        // pairwise site connectivity
        let mut link = vec![vec![false; nz]; nz];
        let is_cut = |a: usize, b: usize| cut.iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a));
        let connect = |a: usize, b: usize, link: &mut Vec<Vec<bool>>| {
            if a != b && !down_zone[a] && !down_zone[b] && !is_cut(a, b) {
                link[a][b] = true;
                link[b][a] = true;
            }
        };
        for ends in ix.edge_ends.iter().flatten() {
            if let (Some(a), Some(b)) = (ix.site_of[ends.0], ix.site_of[ends.1]) {
                connect(a, b, &mut link);
            }
        }
        if ix.settings.assumptions.implicit_mesh {
            for u in 0..ix.units.len() {
                let sites = ix.unit_sites(u);
                for i in 0..sites.len() {
                    for j in i + 1..sites.len() {
                        connect(sites[i], sites[j], &mut link);
                    }
                }
            }
        }
        if ix.settings.assumptions.transit_routing {
            // transitive closure over surviving sites
            for k in 0..nz {
                for i in 0..nz {
                    if !link[i][k] {
                        continue;
                    }
                    for j in 0..nz {
                        if link[k][j] {
                            link[i][j] = true;
                        }
                    }
                }
            }
        }

        let nu = ix.units.len();
        let mut pre_events: Vec<Event> = vec![];
        let mut pre_tracks: Vec<f64> = vec![];
        for (i, secs) in &scale_out {
            if ix.on_path[ix.unit_of[*i]] {
                pre_tracks.push(*secs);
            }
            pre_events.push(ev(
                "scale_out",
                Some(&ix.units[ix.unit_of[*i]].key),
                Some(&ix.arch.nodes[*i].id),
                json!({"node": ix.arch.nodes[*i].name, "seconds": secs, "to": ix.eff[*i].num("failoverReplicas")}),
            ));
        }
        Sim {
            ix,
            alive,
            link,
            destructive: sc.is_destructive(),
            eff_ok: vec![false; n],
            degraded: vec![false; n],
            serving: vec![vec![]; nu],
            done: vec![false; nu],
            unit_lost_members: vec![false; nu],
            fenced: vec![false; n],
            promoted: vec![false; n],
            isolated: vec![false; n],
            events: pre_events,
            auto_tracks: pre_tracks,
            manual_tracks: vec![],
            restore_tracks: vec![],
            rpo_items: vec![],
            split_brain: vec![],
            data_lost: vec![],
            cut,
            failed,
        }
    }

    /// Can node `a` talk to node `b`?
    fn reach(&self, a: usize, b: usize) -> bool {
        if a == b {
            return true;
        }
        match (self.ix.site_of[a], self.ix.site_of[b]) {
            (None, _) | (_, None) => true,
            (Some(x), Some(y)) => x == y || self.link[x][y],
        }
    }

    fn name(&self, ni: usize) -> &str {
        self.ix.node_name(ni)
    }

    /// Sum of `w(k)` over `members` that `observer` can reach.
    fn seen_weight(&self, observer: usize, members: &[usize], w: impl Fn(usize) -> f64) -> f64 {
        members.iter().filter(|&&k| self.reach(observer, k)).map(|&k| w(k)).sum()
    }

    fn eval_unit(&mut self, u: usize) {
        let ix = self.ix;
        let unit = &ix.units[u];

        // 1. effectiveness of members (own hard dependencies)
        for &m in &unit.members {
            if self.alive[m] == 0 {
                self.eff_ok[m] = false;
                continue;
            }
            let mut ok = true;
            for &ei in &ix.out_edges[m] {
                if ix.sem(ei) != Some(Semantics::Dependency) {
                    continue;
                }
                let Some((_, t)) = ix.edge_ends[ei] else { continue };
                let v = ix.unit_of[t];
                if v == u {
                    continue;
                }
                let avail = !self.done[v] || self.serving[v].iter().any(|&s| self.reach(m, s));
                if !avail {
                    if ix.is_hard(ei) {
                        ok = false;
                    } else {
                        self.degraded[m] = true;
                    }
                }
            }
            self.eff_ok[m] = ok;
        }

        for &m in &unit.members {
            if self.alive[m] < ix.replicas(m) {
                self.unit_lost_members[u] = true;
            }
        }

        match unit.behavior {
            Behavior::Stateless => {
                self.serving[u] = unit.members.iter().copied().filter(|&m| self.eff_ok[m]).collect();
            }
            Behavior::Consensus => self.eval_consensus(u),
            Behavior::QuorumLog => self.eval_quorum_log(u),
            Behavior::PrimaryReplica => self.eval_primary_replica(u),
        }
        self.done[u] = true;

        if matches!(unit.behavior, Behavior::Consensus | Behavior::QuorumLog) {
            for &m in &unit.members {
                if self.alive[m] > 0 && self.eff_ok[m] && !self.serving[u].contains(&m) {
                    self.isolated[m] = true;
                }
            }
        }
    }

    fn vote_w(&self, m: usize) -> f64 {
        self.ix.eff[m].num("votes").unwrap_or(1.0)
    }

    fn eval_consensus(&mut self, u: usize) {
        let ix = self.ix;
        let members = ix.units[u].members.clone();
        let total: f64 = members.iter().map(|&m| self.vote_w(m) * ix.replicas(m) as f64).sum();
        let alive_w = |k: usize| if self.eff_ok[k] { self.vote_w(k) * self.alive[k] as f64 } else { 0.0 };
        let serving: Vec<usize> = members.iter().copied().filter(|&m| self.eff_ok[m] && self.seen_weight(m, &members, alive_w) * 2.0 > total).collect();
        let alive_total: f64 = members.iter().map(|&k| alive_w(k)).sum();
        if serving.is_empty() {
            self.events.push(ev("quorum_lost", Some(&ix.units[u].key), None, json!({"unit": ix.units[u].name, "total": total})));
        } else if alive_total < total || serving.len() < members.len() {
            let best = serving.iter().map(|&m| self.seen_weight(m, &members, alive_w)).fold(0.0_f64, f64::max);
            self.events.push(ev("quorum_ok", Some(&ix.units[u].key), None, json!({"unit": ix.units[u].name, "votes": best, "total": total})));
        }
        self.serving[u] = serving;
    }

    fn eval_quorum_log(&mut self, u: usize) {
        let ix = self.ix;
        let members = ix.units[u].members.clone();
        let total_inst: u32 = members.iter().map(|&m| ix.replicas(m)).sum();
        let min_insync = members.iter().filter_map(|&m| ix.eff[m].num("minInsync")).fold(1.0_f64, f64::max);
        let need = min_insync.max((total_inst / 2 + 1) as f64);
        let alive_w = |k: usize| if self.eff_ok[k] { self.alive[k] as f64 } else { 0.0 };
        let serving: Vec<usize> = members.iter().copied().filter(|&m| self.eff_ok[m] && self.seen_weight(m, &members, alive_w) >= need).collect();
        if serving.is_empty() {
            if members.iter().any(|&m| self.alive[m] > 0) || self.unit_lost_members[u] {
                self.events.push(ev("quorum_lost", Some(&ix.units[u].key), None, json!({"unit": ix.units[u].name, "total": total_inst, "need": need})));
            }
        } else {
            let a: f64 = members.iter().map(|&k| alive_w(k)).sum();
            if a < total_inst as f64 {
                self.events.push(ev("quorum_ok", Some(&ix.units[u].key), None, json!({"unit": ix.units[u].name, "votes": a, "total": total_inst})));
            }
        }
        self.serving[u] = serving;
    }

    fn replication_edges(&self, p: usize, unit: usize) -> Vec<(usize, usize, String)> {
        let ix = self.ix;
        let mut out = vec![];
        for &ei in ix.out_edges[p].iter().chain(ix.in_edges[p].iter()) {
            if ix.sem(ei) != Some(Semantics::Replication) {
                continue;
            }
            if let Some((s, t)) = ix.edge_ends[ei] {
                let other = if s == p { t } else { s };
                if other != p && ix.unit_of[other] == unit {
                    let mode = ix.edge_eff[ei].text("mode").unwrap_or_else(|| "async".into());
                    out.push((other, ei, mode));
                }
            }
        }
        out
    }

    fn lag_of(&self, ei: usize) -> (f64, bool) {
        match self.ix.edge_eff[ei].num("lagSeconds") {
            Some(l) if self.ix.edge_eff[ei].is_declared("lagSeconds") => (l, false),
            _ => (self.ix.settings.assumptions.async_lag_seconds, true),
        }
    }

    fn eval_primary_replica(&mut self, u: usize) {
        let ix = self.ix;
        let unit = &ix.units[u];
        let members = unit.members.clone();
        let primary = members.iter().copied().find(|&m| ix.arch.nodes[m].role.as_deref() == Some("primary")).or_else(|| members.iter().copied().find(|&m| !ix.is_witness(m)));
        let Some(p) = primary else {
            self.serving[u] = vec![];
            return;
        };
        // failover mode is a property of the candidate: it decides how *that* node is promoted
        let fmode = |m: usize| ix.eff[m].text("failover").unwrap_or_else(|| "manual".into());
        let authority = if let Some(du) = ix.dcs_unit(u) {
            Authority::External(du)
        } else if members.iter().any(|&m| ix.is_witness(m)) {
            Authority::SelfVotes
        } else {
            Authority::None
        };

        // can member/observer m see the quorum authority?
        let total_votes: f64 = members.iter().map(|&m| self.vote_w(m)).sum();
        let has_q: Vec<bool> = (0..ix.arch.nodes.len())
            .map(|m| match authority {
                Authority::None => true,
                Authority::External(du) => !self.done[du] || self.serving[du].iter().any(|&s| self.reach(m, s)),
                Authority::SelfVotes => self.seen_weight(m, &members, |k| if self.alive[k] > 0 { self.vote_w(k) } else { 0.0 }) * 2.0 > total_votes,
            })
            .collect();

        let p_alive = self.alive[p] > 0 && self.eff_ok[p];
        let p_ok = p_alive && has_q[p];
        if p_alive && !has_q[p] {
            self.fenced[p] = true;
            self.events.push(ev("fenced", Some(&unit.key), Some(&ix.arch.nodes[p].id), json!({"node": self.name(p), "unit": unit.name})));
        }
        if self.alive[p] == 0 {
            self.events.push(ev("primary_lost", Some(&unit.key), Some(&ix.arch.nodes[p].id), json!({"node": self.name(p), "unit": unit.name})));
        }

        let reps = self.replication_edges(p, u);
        // candidates: alive replicas that are allowed to be promoted at all
        let cands: Vec<usize> = members.iter().copied().filter(|&m| m != p && !ix.is_witness(m) && self.alive[m] > 0 && self.eff_ok[m] && fmode(m) != "none").collect();

        // rank: automatic promotion first (needs quorum), then by replication quality
        // (class, auto?, lag, assumed-lag?, linked?)
        let mut ranked: Vec<(usize, bool, i32, f64, bool, bool)> = cands
            .iter()
            .map(|&r| {
                let auto = fmode(r) == "auto" && has_q[r];
                match reps.iter().find(|e| e.0 == r) {
                    Some((_, _, mode)) if mode == "sync" => (r, auto, 0, 0.0, false, true),
                    Some((_, ei, _)) => {
                        let (lag, assumed) = self.lag_of(*ei);
                        (r, auto, 1, lag, assumed, true)
                    }
                    None => (r, auto, 2, 0.0, false, false),
                }
            })
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal)));

        let mut writers: Vec<usize> = vec![];
        let promote_secs = |this: &Self, r: usize| -> f64 {
            let mut secs = ix.eff[r].num("failoverSeconds").unwrap_or(ix.settings.assumptions.default_failover_seconds);
            if let Authority::External(du) = authority {
                if this.unit_lost_members[du] {
                    let el = ix.units[du].members.iter().filter_map(|&m| ix.eff[m].num("electionSeconds")).fold(0.0_f64, f64::max);
                    secs += if el > 0.0 { el } else { ix.settings.assumptions.default_election_seconds };
                }
            }
            secs
        };

        if p_ok {
            writers.push(p);
            // without any quorum authority an auto-failover replica that cannot see the primary
            // promotes itself: two writers
            if authority == Authority::None {
                if let Some(&(r, _, _, lag, assumed, linked)) = ranked.iter().find(|c| fmode(c.0) == "auto" && !self.reach(p, c.0)) {
                    writers.push(r);
                    self.promoted[r] = true;
                    self.split_brain.push(unit.key.clone());
                    let secs = promote_secs(self, r);
                    if ix.on_path[u] {
                        self.auto_tracks.push(secs);
                    }
                    self.note_rpo(u, linked, lag, assumed);
                    self.events.push(ev("split_brain", Some(&unit.key), Some(&ix.arch.nodes[r].id), json!({"node": self.name(p), "other": self.name(r), "unit": unit.name})));
                }
            }
            let sync_all: Vec<&(usize, usize, String)> = reps.iter().filter(|r| r.2 == "sync").collect();
            let sync_up = sync_all.iter().filter(|r| self.alive[r.0] > 0 && self.eff_ok[r.0] && self.reach(p, r.0)).count();
            if !sync_all.is_empty() && sync_up == 0 && !self.split_brain.contains(&unit.key) {
                let on_loss = ix.edge_eff[sync_all[0].1].text("onReplicaLoss").unwrap_or_else(|| "degrade".into());
                if on_loss == "block" {
                    writers.clear();
                    self.events.push(ev("writes_blocked", Some(&unit.key), Some(&ix.arch.nodes[p].id), json!({"node": self.name(p), "unit": unit.name})));
                } else {
                    self.events.push(ev("async_fallback", Some(&unit.key), Some(&ix.arch.nodes[p].id), json!({"node": self.name(p), "unit": unit.name})));
                }
            }
        } else {
            match ranked.first() {
                Some(&(r, auto, _, lag, assumed, linked)) => {
                    writers.push(r);
                    self.promoted[r] = true;
                    self.note_rpo(u, linked, lag, assumed);
                    if auto {
                        let secs = promote_secs(self, r);
                        if ix.on_path[u] {
                            self.auto_tracks.push(secs);
                        }
                        self.events.push(ev(
                            "promoted",
                            Some(&unit.key),
                            Some(&ix.arch.nodes[r].id),
                            json!({"node": self.name(r), "unit": unit.name, "seconds": secs, "rpo": if linked { json!(lag) } else { Value::Null }}),
                        ));
                    } else {
                        // an operator promotes the replica (manual mode, or automatic mode
                        // without a reachable quorum)
                        let secs = ix.settings.assumptions.manual_failover_seconds;
                        if ix.on_path[u] {
                            self.manual_tracks.push(secs);
                        }
                        self.events.push(ev(
                            "manual_failover",
                            Some(&unit.key),
                            Some(&ix.arch.nodes[r].id),
                            json!({"node": self.name(r), "unit": unit.name, "seconds": secs, "noQuorum": fmode(r) == "auto"}),
                        ));
                    }
                }
                None => {
                    let any_alive = members.iter().any(|&m| m != p && !ix.is_witness(m) && self.alive[m] > 0);
                    let code = if any_alive { "no_failover" } else { "no_candidate" };
                    self.events.push(ev(code, Some(&unit.key), None, json!({"unit": unit.name})));
                }
            }
        }
        self.serving[u] = writers;
    }

    fn note_rpo(&mut self, u: usize, linked: bool, lag: f64, assumed: bool) {
        let unit = &self.ix.units[u];
        self.rpo_items.push(RpoItem {
            unit: unit.name.clone(),
            seconds: if linked { Some(lag) } else { None },
            assumed: linked && assumed && lag > 0.0,
            traits: unit.traits.iter().cloned().collect(),
        });
    }

    fn per_replica_rps(&self, m: usize) -> Option<f64> {
        self.ix.per_replica_rps(m)
    }

    /// Propagate the peak load through the dependency graph. Load enters at the entry units and
    /// is split between the *serving members* of a unit (weighted by their capacity); every
    /// member forwards its share along its own outgoing edges, so a failed member never
    /// forwards load to a dependency only it was talking to.
    fn compute_loads(&self, rps: f64) -> Vec<(usize, f64)> {
        let ix = self.ix;
        let nu = ix.units.len();
        let mut load = vec![0.0f64; nu];
        for u in 0..nu {
            if ix.unit_has_trait(u, "entry") {
                load[u] = rps;
            }
        }
        for &u in ix.unit_order.iter().rev() {
            if load[u] <= 0.0 || self.serving[u].is_empty() {
                continue;
            }
            let weights: Vec<(usize, f64)> = self.serving[u].iter().map(|&m| (m, self.alive[m] as f64 * self.per_replica_rps(m).unwrap_or(1.0))).collect();
            let total_w: f64 = weights.iter().map(|w| w.1).sum();
            if total_w <= 0.0 {
                continue;
            }
            for (m, w) in weights {
                let share = load[u] * w / total_w;
                let mut cold = false;
                let mut targets: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
                for &ei in &ix.out_edges[m] {
                    if ix.sem(ei) != Some(Semantics::Dependency) {
                        continue;
                    }
                    let Some((_, t)) = ix.edge_ends[ei] else { continue };
                    let v = ix.unit_of[t];
                    if v == u {
                        continue;
                    }
                    if self.serving[v].is_empty() {
                        // a soft dependency on a missing cache makes the datastores see cold traffic
                        if !ix.is_hard(ei) && ix.unit_has_trait(v, "cache") {
                            cold = true;
                        }
                        continue;
                    }
                    let fan = ix.edge_eff[ei].num("fanout").unwrap_or(1.0);
                    let fan_cold = ix.edge_eff[ei].num("fanoutColdCache").unwrap_or(fan);
                    let e = targets.entry(v).or_insert((0.0, 0.0));
                    e.0 = e.0.max(fan);
                    e.1 = e.1.max(fan_cold);
                }
                for (v, (fan, fan_cold)) in targets {
                    let uses_cold = cold && ix.unit_has_trait(v, "datastore") && !ix.unit_has_trait(v, "cache");
                    load[v] += share * if uses_cold { fan_cold } else { fan };
                }
            }
        }
        load.into_iter().enumerate().collect()
    }

    fn finish(mut self, sc: &Scenario) -> SimResult {
        let ix = self.ix;
        // --- reroute detection for lost members behind balancers
        let mut reroute = 0.0f64;
        for u in 0..ix.units.len() {
            if ix.units[u].behavior != Behavior::Stateless || self.serving[u].is_empty() {
                continue;
            }
            for &m in &ix.units[u].members {
                if self.serving[u].contains(&m) {
                    continue;
                }
                for &ei in &ix.in_edges[m] {
                    if ix.sem(ei) != Some(Semantics::Dependency) {
                        continue;
                    }
                    if let Some((s, _)) = ix.edge_ends[ei] {
                        if self.alive[s] > 0 && ix.has_trait(s, "balancer") {
                            let hc = ix.eff[s].num("healthCheckSeconds").unwrap_or(ix.settings.assumptions.default_health_check_seconds);
                            let th = ix.eff[s].num("failThreshold").unwrap_or(ix.settings.assumptions.default_fail_threshold);
                            let ttl = if ix.has_trait(s, "dns-routing") { ix.eff[s].num("dnsTtlSeconds").unwrap_or(0.0) } else { 0.0 };
                            reroute = reroute.max(hc * th + ttl);
                        }
                    }
                }
            }
        }
        if reroute > 0.0 {
            self.auto_tracks.push(reroute);
            self.events.push(ev("reroute", None, None, json!({"seconds": reroute})));
        }

        // --- data fate of destroyed stateful units
        if self.destructive {
            for u in 0..ix.units.len() {
                if !ix.unit_has_trait(u, "backup-required") && !ix.unit_has_trait(u, "mq") {
                    continue;
                }
                let data_members: Vec<usize> = ix.units[u].members.iter().copied().filter(|&m| !ix.is_witness(m)).collect();
                if data_members.is_empty() || data_members.iter().any(|&m| self.alive[m] > 0) {
                    continue;
                }
                // replicated into another unit that survived?
                let mut best: Option<(f64, bool, bool)> = None;
                for &m in &data_members {
                    for &ei in ix.out_edges[m].iter().chain(ix.in_edges[m].iter()) {
                        if ix.sem(ei) != Some(Semantics::Replication) {
                            continue;
                        }
                        if let Some((s, t)) = ix.edge_ends[ei] {
                            let other = if s == m { t } else { s };
                            if ix.unit_of[other] != u && self.alive[other] > 0 {
                                let mode = ix.edge_eff[ei].text("mode").unwrap_or_else(|| "async".into());
                                let (lag, assumed) = if mode == "sync" { (0.0, false) } else { self.lag_of(ei) };
                                let mitigated = ix.edge_eff[ei].text("mitigation").is_some();
                                if best.map(|b| lag < b.0).unwrap_or(true) {
                                    best = Some((lag, assumed, mitigated));
                                }
                            }
                        }
                    }
                }
                if let Some((lag, assumed, mitigated)) = best {
                    if !mitigated {
                        self.note_rpo(u, true, lag, assumed);
                    }
                    self.events.push(ev("replicated_elsewhere", Some(&ix.units[u].key), None, json!({"unit": ix.units[u].name, "rpo": lag, "mitigated": mitigated})));
                    continue;
                }
                // backup?
                let mut bk: Option<(f64, f64, bool)> = None;
                for &m in &data_members {
                    for &ei in ix.out_edges[m].iter().chain(ix.in_edges[m].iter()) {
                        if ix.sem(ei) != Some(Semantics::Backup) {
                            continue;
                        }
                        if let Some((s, t)) = ix.edge_ends[ei] {
                            let other = if s == m { t } else { s };
                            if other == m || self.alive[other] == 0 {
                                continue;
                            }
                            let interval = ix.edge_eff[ei].num("intervalSeconds").or_else(|| ix.eff[other].num("backupIntervalSeconds"));
                            let assumed = interval.is_none();
                            let interval = interval.unwrap_or(ix.settings.assumptions.backup_interval_seconds);
                            let restore = ix.eff[other].num("restoreSeconds").unwrap_or(ix.settings.assumptions.restore_seconds);
                            if bk.map(|b| interval < b.0).unwrap_or(true) {
                                bk = Some((interval, restore, assumed));
                            }
                        }
                    }
                }
                if let Some((interval, restore, assumed)) = bk {
                    self.note_rpo(u, true, interval, assumed);
                    if ix.on_path[u] {
                        self.restore_tracks.push(restore);
                    }
                    self.events.push(ev("restore_from_backup", Some(&ix.units[u].key), None, json!({"unit": ix.units[u].name, "rpo": interval, "seconds": restore})));
                } else {
                    self.data_lost.push(ix.units[u].key.clone());
                    self.events.push(ev("data_lost", Some(&ix.units[u].key), None, json!({"unit": ix.units[u].name})));
                }
            }
        }

        // --- entry state
        let mut entries: Vec<usize> = (0..ix.arch.nodes.len()).filter(|&m| ix.has_trait(m, "entry")).collect();
        if entries.is_empty() {
            entries = (0..ix.arch.nodes.len())
                .filter(|&m| ix.in_edges[m].iter().all(|&ei| ix.sem(ei) != Some(Semantics::Dependency)) && ix.out_edges[m].iter().any(|&ei| ix.sem(ei) == Some(Semantics::Dependency)))
                .collect();
        }
        let served = entries.iter().filter(|&&m| self.serving[ix.unit_of[m]].contains(&m)).count();
        let mut state = if entries.is_empty() {
            ServiceState::Up
        } else if served == 0 {
            ServiceState::Down
        } else if served < entries.len() {
            ServiceState::Degraded
        } else {
            ServiceState::Up
        };
        if state == ServiceState::Up {
            let any_degraded = (0..ix.arch.nodes.len()).any(|m| self.degraded[m] && self.eff_ok[m] && self.serving[ix.unit_of[m]].contains(&m));
            if any_degraded {
                state = ServiceState::Degraded;
                self.events.push(ev("soft_dependency_lost", None, None, json!({})));
            }
        }

        // --- capacity
        let mut unit_loads: Vec<UnitLoad> = vec![];
        let mut capacity_ratio: Option<f64> = None;
        let mut bottleneck: Option<String> = None;
        if let Some(rps) = ix.arch.requirements.rps_peak {
            for (u, load) in self.compute_loads(rps) {
                if load <= 0.0 {
                    continue;
                }
                let has_model = ix.units[u].members.iter().any(|&m| self.per_replica_rps(m).is_some());
                let cap = if has_model { Some(self.serving[u].iter().filter_map(|&m| self.per_replica_rps(m).map(|per| per * self.alive[m] as f64)).sum::<f64>()) } else { None };
                let ratio = cap.map(|c| c / load);
                if let Some(r) = ratio {
                    if capacity_ratio.map(|cr| r < cr).unwrap_or(true) {
                        capacity_ratio = Some(r);
                        bottleneck = Some(ix.units[u].key.clone());
                    }
                }
                unit_loads.push(UnitLoad { unit: ix.units[u].key.clone(), load_rps: load, capacity_rps: cap, ratio });
            }
            if state == ServiceState::Down {
                capacity_ratio = Some(0.0);
            } else if let Some(r) = capacity_ratio {
                if r < 1.0 && state == ServiceState::Up {
                    state = ServiceState::Degraded;
                    self.events.push(ev("capacity_overload", bottleneck.as_deref(), None, json!({"ratio": (r * 100.0).round() / 100.0})));
                }
            }
        }

        // --- recovery
        let auto = self.auto_tracks.iter().cloned().fold(0.0_f64, f64::max);
        let manual = self.manual_tracks.iter().cloned().fold(0.0_f64, f64::max);
        let restore = self.restore_tracks.iter().cloned().fold(0.0_f64, f64::max);
        let recovery = if state == ServiceState::Down {
            if !self.restore_tracks.is_empty() {
                Recovery { kind: RecoveryKind::Manual, seconds: Some(restore.max(manual).max(auto)) }
            } else {
                Recovery { kind: RecoveryKind::Never, seconds: None }
            }
        } else if !self.manual_tracks.is_empty() || !self.restore_tracks.is_empty() {
            Recovery { kind: RecoveryKind::Manual, seconds: Some(manual.max(restore).max(auto)) }
        } else if !self.auto_tracks.is_empty() {
            Recovery { kind: RecoveryKind::Auto, seconds: Some(auto) }
        } else {
            Recovery { kind: RecoveryKind::None, seconds: Some(0.0) }
        };

        // --- states for display
        let mut node_states = BTreeMap::new();
        for (m, n) in ix.arch.nodes.iter().enumerate() {
            let rep = ix.replicas(m);
            let st = if self.alive[m] == 0 {
                NodeState::Down
            } else if self.fenced[m] {
                NodeState::Fenced
            } else if self.promoted[m] {
                NodeState::Promoted
            } else if self.isolated[m] {
                NodeState::Isolated
            } else if !self.eff_ok[m] {
                NodeState::Stalled
            } else if self.alive[m] < rep {
                NodeState::Partial
            } else {
                NodeState::Up
            };
            node_states.insert(n.id.clone(), st);
        }
        let mut edge_states = BTreeMap::new();
        for (ei, e) in ix.arch.edges.iter().enumerate() {
            let st = match ix.edge_ends[ei] {
                Some((s, t)) => {
                    if self.alive[s] == 0 || self.alive[t] == 0 {
                        EdgeState::Dead
                    } else if let (Some(a), Some(b)) = (ix.site_of[s], ix.site_of[t]) {
                        if a != b && !self.link[a][b] {
                            EdgeState::Cut
                        } else {
                            EdgeState::Ok
                        }
                    } else {
                        EdgeState::Ok
                    }
                }
                None => EdgeState::Dead,
            };
            edge_states.insert(e.id.clone(), st);
        }
        let _ = &self.cut;
        let rpo_total = rpo_filtered(&self.rpo_items, &[]);
        let down_units: Vec<String> = ix.unit_order.iter().filter(|&&u| self.serving[u].is_empty() && !ix.unit_has_trait(u, "entry")).map(|&u| ix.units[u].name.clone()).collect();

        SimResult {
            scenario: sc.clone(),
            state,
            recovery,
            rpo_seconds: rpo_total.0,
            rpo_assumed: rpo_total.1,
            rpo_items: self.rpo_items,
            data_lost: self.data_lost,
            split_brain: self.split_brain,
            capacity_ratio,
            bottleneck,
            unit_loads,
            down_units,
            events: self.events,
            node_states,
            edge_states,
            failed: self.failed,
        }
    }
}

/// Simulate one scenario.
pub fn simulate(ix: &Index<'_>, sc: &Scenario) -> SimResult {
    let mut sim = Sim::new(ix, sc);
    for &u in &ix.unit_order {
        sim.eval_unit(u);
    }
    for u in 0..ix.units.len() {
        if !sim.done[u] {
            sim.eval_unit(u);
        }
    }
    sim.finish(sc)
}

/// The scenarios the engine derives from an architecture, and whether the cap cut some of them.
#[derive(Debug, Clone, Default)]
pub struct ScenarioSet {
    pub scenarios: Vec<Scenario>,
    /// How many scenarios the architecture yields before `scenarios.maxCount` and the combination caps.
    pub generated: usize,
    /// True if some scenarios were left out (cap on the total, or sampled combinations of many sites).
    pub truncated: bool,
}

/// `C(n, k)`, saturating.
pub fn binomial(n: usize, k: usize) -> u128 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 0..k {
        r = match r.checked_mul((n - i) as u128) {
            Some(v) => v / (i as u128 + 1),
            None => return u128::MAX,
        };
    }
    r
}

/// The `rank`-th (0-based, lexicographic) k-combination of `0..n`.
fn unrank_combination(n: usize, k: usize, mut rank: u128) -> Vec<usize> {
    let mut out = Vec::with_capacity(k);
    let mut x = 0usize;
    for i in 0..k {
        loop {
            let c = binomial(n - x - 1, k - i - 1);
            if rank < c {
                break;
            }
            rank -= c;
            x += 1;
        }
        out.push(x);
        x += 1;
    }
    out
}

/// Combinations of `k` out of `n` items (indices), at most `cap` of them. Within the cap every combination is
/// returned; past it they are picked at even steps through the lexicographic order, so the choice is
/// deterministic and spread over the whole range. The flag says the result was sampled.
pub fn combinations(n: usize, k: usize, cap: usize) -> (Vec<Vec<usize>>, bool) {
    if k == 0 || k > n || cap == 0 {
        return (vec![], false);
    }
    let total = binomial(n, k);
    if total <= cap as u128 {
        let mut out = Vec::with_capacity(total as usize);
        let mut idx: Vec<usize> = (0..k).collect();
        loop {
            out.push(idx.clone());
            let mut i = k;
            while i > 0 && idx[i - 1] == n - k + i - 1 {
                i -= 1;
            }
            if i == 0 {
                break;
            }
            idx[i - 1] += 1;
            for j in i..k {
                idx[j] = idx[j - 1] + 1;
            }
        }
        return (out, false);
    }
    let out = (0..cap as u128).map(|i| unrank_combination(n, k, i * total / cap as u128)).collect();
    (out, true)
}

/// Sites ordered for scenario generation (zone index order, the order of the document).
fn scenario_sites(ix: &Index<'_>) -> Vec<usize> {
    ix.sites()
}

/// Groups of sites that are cut off from the rest, smallest first. A partition and its complement are the same
/// cut, so only the smaller side is listed (for two equal halves the side holding the first site).
fn isolation_groups(ix: &Index<'_>, sites: &[usize]) -> (Vec<Vec<usize>>, bool) {
    let cfg = &ix.settings.scenarios;
    let n = sites.len();
    let mut out: Vec<Vec<usize>> = vec![];
    let mut sampled = false;
    for size in 1..=cfg.isolation_max_sites.min(n.saturating_sub(1)) {
        if size * 2 > n {
            break;
        }
        let (combos, s) = combinations(n, size, cfg.max_combinations);
        sampled |= s;
        for c in combos {
            if size * 2 == n && !c.contains(&0) {
                continue;
            }
            out.push(c.into_iter().map(|i| sites[i]).collect());
        }
    }
    (out, sampled)
}

/// All combinations of `k` sites (zone indices), capped at `scenarios.maxCombinations`; the flag says they were sampled.
pub fn site_combinations(ix: &Index<'_>, k: usize) -> (Vec<Vec<usize>>, bool) {
    let sites = scenario_sites(ix);
    let (combos, sampled) = combinations(sites.len(), k, ix.settings.scenarios.max_combinations);
    (combos.into_iter().map(|c| c.into_iter().map(|i| sites[i]).collect()).collect(), sampled)
}

/// Standard scenario set derived from the architecture, bounded by `scenarios.maxCount`. Ordered by importance, so
/// the cap drops the single-instance scenarios first.
pub fn scenario_set(ix: &Index<'_>) -> ScenarioSet {
    let cfg = &ix.settings.scenarios;
    let mut out: Vec<Scenario> = Vec::new();
    let mut truncated = false;
    let sites = scenario_sites(ix);

    // racks inside sites (a rack that is itself the failure domain of its nodes is a site already)
    if cfg.include_racks {
        for zi in ix.rack_zones() {
            if !sites.contains(&zi) {
                out.push(Scenario::Zone { zone_id: ix.arch.zones[zi].id.clone() });
            }
        }
    }
    for &s in &sites {
        out.push(Scenario::Zone { zone_id: ix.arch.zones[s].id.clone() });
    }

    // sites cut off the network while they keep running
    if cfg.include_isolation && sites.len() >= 3 {
        let (groups, sampled) = isolation_groups(ix, &sites);
        truncated |= sampled;
        for g in groups {
            out.push(Scenario::Isolate { zone_ids: g.into_iter().map(|s| ix.arch.zones[s].id.clone()).collect() });
        }
    }

    // links between sites that are actually connected by an edge or an implicit mesh
    let mut pairs: Vec<(usize, usize)> = vec![];
    for ends in ix.edge_ends.iter().flatten() {
        if let (Some(a), Some(b)) = (ix.site_of[ends.0], ix.site_of[ends.1]) {
            if a != b {
                let p = (a.min(b), a.max(b));
                if !pairs.contains(&p) {
                    pairs.push(p);
                }
            }
        }
    }
    if ix.settings.assumptions.implicit_mesh {
        for u in 0..ix.units.len() {
            let ss = ix.unit_sites(u);
            for i in 0..ss.len() {
                for j in i + 1..ss.len() {
                    let p = (ss[i].min(ss[j]), ss[i].max(ss[j]));
                    if !pairs.contains(&p) {
                        pairs.push(p);
                    }
                }
            }
        }
    }
    pairs.sort();
    if pairs.len() > cfg.max_combinations {
        // keep an even spread of the links when there are far too many to show
        let n = pairs.len();
        pairs = (0..cfg.max_combinations).map(|i| pairs[i * n / cfg.max_combinations]).collect();
        truncated = true;
    }
    for (a, b) in &pairs {
        out.push(Scenario::Link { a: ix.arch.zones[*a].id.clone(), b: ix.arch.zones[*b].id.clone() });
    }

    // disaster domains with several sites (a single-site domain is the zone scenario itself)
    let mut geos: Vec<String> = ix.geo_of.iter().flatten().cloned().collect();
    geos.sort();
    geos.dedup();
    let sites_of_geo = |g: &str| -> Vec<usize> {
        let mut v: Vec<usize> = (0..ix.arch.nodes.len()).filter(|&n| ix.geo_of[n].as_deref() == Some(g)).filter_map(|n| ix.site_of[n]).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let mut geo_sets: Vec<Vec<usize>> = vec![];
    for g in geos {
        let ss = sites_of_geo(&g);
        if ss.len() >= 2 {
            geo_sets.push(ss);
            out.push(Scenario::Geo { geo: g });
        }
    }
    if cfg.include_pairs && sites.len() >= 3 {
        let (combos, sampled) = site_combinations(ix, 2);
        truncated |= sampled;
        for pair in combos {
            if geo_sets.contains(&pair) {
                continue;
            }
            out.push(Scenario::Sites { zone_ids: pair.into_iter().map(|s| ix.arch.zones[s].id.clone()).collect() });
        }
    }
    if cfg.include_instances {
        for n in &ix.arch.nodes {
            out.push(Scenario::Instance { node_id: n.id.clone(), count: 1 });
        }
    }
    if cfg.include_nodes {
        for (i, n) in ix.arch.nodes.iter().enumerate() {
            // whole-node loss differs from the instance loss only if the node has >1 instance
            if ix.replicas(i) > 1 {
                out.push(Scenario::Node { node_id: n.id.clone() });
            }
        }
    }
    let generated = out.len();
    if out.len() > cfg.max_count {
        out.truncate(cfg.max_count);
        truncated = true;
    }
    ScenarioSet { scenarios: out, generated, truncated }
}

/// Standard scenarios of the architecture (see [`scenario_set`]).
pub fn standard_scenarios(ix: &Index<'_>) -> Vec<Scenario> {
    scenario_set(ix).scenarios
}

/// Worst-case RPO over the per-unit items, ignoring units that carry any of `exclude_traits`.
/// Returns `(seconds, assumed)`; `seconds == None` if some contributing unit is undetermined.
pub fn rpo_filtered(items: &[RpoItem], exclude_traits: &[String]) -> (Option<f64>, bool) {
    let mut worst = Some(0.0_f64);
    let mut assumed = false;
    for it in items.iter().filter(|i| !i.traits.iter().any(|t| exclude_traits.contains(t))) {
        match (it.seconds, worst) {
            (None, _) => worst = None,
            (Some(v), Some(w)) => worst = Some(w.max(v)),
            _ => {}
        }
        assumed |= it.assumed;
    }
    (worst, assumed)
}
