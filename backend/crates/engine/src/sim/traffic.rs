//! Where the traffic goes.
//!
//! The capacity model (`Sim::compute_loads`) knows how much load a *unit* carries. This module spreads the same load over
//! the *links* of the picture, so that it can be watched: what each link carries in normal operation, where the requests go
//! when a link or a member is gone, and where they are dropped. All figures are relative to the load that enters at an
//! entry point (1 = all of it); the UI multiplies them by the declared peak when there is one.
//!
//! How the traffic of a unit is spread over its members is the capacity model's: the members that serve share what arrives
//! at the unit by capacity. This module only decides which *links* carry it, so the numbers on the links add up to the
//! loads in the capacity table. The rules:
//! * a source sends to the members of the unit that it has a drawn link to, as far as their share allows;
//! * what is left, because a member's share is smaller than what its sources send it or because a source has no link to
//!   any member that serves, goes to the members that still lack traffic by a *detour*: a route no drawn link leads along
//!   (the unit model lets any member of a unit reach any serving member of the unit it depends on);
//! * a link whose target is gone (dead, cut off, fenced, not in the pool) takes nothing. While a *balancer* has not yet
//!   noticed that (health checks and, behind DNS, the TTL) it keeps sending to the whole pool it had, and what goes to the
//!   gone instances is lost;
//! * a unit that does not work takes nothing: the traffic is lost on the links that lead to it. A member that is alive but
//!   cannot serve because something it depends on is gone still passes the traffic on (the requests do arrive), so the loss
//!   shows where the break is and not at the first node that suffers from it;
//! * a database cluster (a primary with replicas) is not mixed: a replica takes the connection and hands it to the writer;
//! * a unit that works but whose capacity is below what arrives sheds the rest at its door.
//!
//! `served` is the share of the entry requests that complete: a request needs every *hard* dependency of every member it
//! passes through, so it is the product of what survives each hop, taken from the bottom up.

use super::{pending, EdgeState, Sim};
use crate::catalog::{Behavior, Semantics};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Shares below this are noise.
const EPS: f64 = 1e-12;

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// A route no drawn link leads along: the traffic of `from` goes to `to` because every link it has to that unit is gone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Detour {
    pub from: String,
    pub to: String,
    pub flow: f64,
    /// Shed on the way because the unit behind it is overloaded.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lost: f64,
}

/// The traffic at one moment, relative to the load entering an entry point (1 = all of it).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Traffic {
    /// What each dependency link carries, by edge id. Links that carry nothing are absent.
    pub flows: BTreeMap<String, f64>,
    /// What was sent into a link and dropped there, so that the requests behind it fail (a dead or cut link, a replica nobody
    /// promoted yet, an overloaded unit).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lost: BTreeMap<String, f64>,
    /// What was sent into a link and dropped there, but the requests survive it: a call to a dependency the service can do
    /// without (a cache).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub missed: BTreeMap<String, f64>,
    /// Routes the traffic takes where no link is drawn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detours: Vec<Detour>,
    /// Share of the entry requests that complete (0..=1).
    pub served: f64,
}

/// Does a member take a call that arrives at it?
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Take {
    /// Yes, and it passes it on / answers it.
    Pass,
    /// It takes the call but nobody can answer it (a replica while the unit has no writer).
    Absorb,
    /// No: it is gone, fenced, cut off or not in the pool.
    Refuse,
}

/// A part of the traffic of one hop that goes to one target.
struct Seg {
    /// The drawn link it goes along; `None` = a detour (or the entry itself).
    edge: Option<usize>,
    to: usize,
    flow: f64,
    lost: f64,
}

/// Everything a member (or the outside world, `from: None`) sends to one unit.
struct Hop {
    from: Option<usize>,
    hard: bool,
    offered: f64,
    segs: Vec<Seg>,
}

/// What a source sends to one unit, not yet placed on links.
struct Offer {
    from: Option<usize>,
    hard: bool,
    amount: f64,
    /// The drawn dependency links from the source into the unit.
    edges: Vec<usize>,
}

struct Walk {
    hops: Vec<Hop>,
    /// What arrives at each member.
    inflow: Vec<f64>,
    /// What is waiting to be placed, by target unit.
    offers: Vec<Vec<Offer>>,
}

impl<'a, 'b> Sim<'a, 'b> {
    /// The traffic in the world as this simulation left it.
    pub(super) fn traffic(&self) -> Traffic {
        let ix = self.ix;
        let mut w = Walk { hops: vec![], inflow: vec![0.0; ix.arch.nodes.len()], offers: (0..ix.units.len()).map(|_| vec![]).collect() };
        self.inject(&mut w);
        // dependents first: everything that sends to a unit has been walked before the unit itself
        for &u in ix.unit_order.iter().rev() {
            self.resolve(u, &mut w);
            self.shed(u, &mut w);
            for &t in &ix.units[u].members {
                let load = self.passes_on(u, t, &w);
                if load > EPS && self.alive[t] > 0 {
                    self.forward(t, load, &mut w);
                }
            }
        }
        // a dependency cycle leaves offers for units that are already walked: place them, nothing is forwarded from them
        for u in 0..ix.units.len() {
            self.resolve(u, &mut w);
        }
        self.settle(w)
    }

    /// What a member sends on. In a unit that works the capacity model gives every serving member its share of the unit's
    /// load, and in a database cluster only the writers pass anything on (the replicas hand the connections to them).
    fn passes_on(&self, u: usize, t: usize, w: &Walk) -> f64 {
        let ix = self.ix;
        if ix.units[u].behavior != Behavior::PrimaryReplica || self.serving[u].is_empty() {
            return w.inflow[t];
        }
        if !self.serving[u].contains(&t) {
            return 0.0;
        }
        let arriving: f64 = ix.units[u].members.iter().map(|&m| w.inflow[m]).sum();
        let total: f64 = self.serving[u].iter().map(|&m| self.weight(m)).sum();
        if total <= EPS {
            arriving / self.serving[u].len() as f64
        } else {
            arriving * self.weight(t) / total
        }
    }

    /// Does the link `ei` work (both ends alive, the sites can talk)?
    pub(super) fn edge_state(&self, ei: usize) -> EdgeState {
        match self.ix.edge_ends[ei] {
            Some((s, t)) => {
                if self.alive[s] == 0 || self.alive[t] == 0 {
                    EdgeState::Dead
                } else if let (Some(a), Some(b)) = (self.ix.site_of[s], self.ix.site_of[t]) {
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
        }
    }

    fn weight(&self, m: usize) -> f64 {
        self.alive[m] as f64 * self.per_replica_rps(m).unwrap_or(1.0)
    }

    /// What the member weighed before the failure: a balancer that has not noticed anything yet still uses it.
    fn weight_before(&self, m: usize) -> f64 {
        self.ix.replicas(m) as f64 * self.per_replica_rps(m).unwrap_or(1.0)
    }

    fn takes(&self, t: usize) -> Take {
        let ix = self.ix;
        let u = ix.unit_of[t];
        if self.alive[t] == 0 || self.fenced[t] || self.isolated[t] {
            return Take::Refuse;
        }
        if self.serving[u].contains(&t) {
            return Take::Pass;
        }
        if !self.eff_ok[t] {
            // alive, but something it depends on is gone. While nobody in the unit works it still takes the call and
            // passes it on, so the traffic shows where the break is; once somebody works, a balancer drops it from the pool
            return if self.serving[u].is_empty() { Take::Pass } else { Take::Refuse };
        }
        match ix.units[u].behavior {
            // a replica (or a sentinel) takes the connection and hands it to the writer; with no writer the call has nowhere to go
            Behavior::PrimaryReplica => {
                if self.serving[u].is_empty() {
                    Take::Absorb
                } else {
                    Take::Pass
                }
            }
            _ => Take::Refuse,
        }
    }

    /// How long a balancer keeps sending to members that are gone: health checks and, behind DNS, the TTL.
    fn detection_window(&self, s: usize) -> Option<f64> {
        let ix = self.ix;
        if self.alive[s] == 0 || !ix.has_trait(s, "balancer") {
            return None;
        }
        let hc = ix.eff[s].num("healthCheckSeconds").unwrap_or(ix.settings.assumptions.default_health_check_seconds);
        let th = ix.eff[s].num("failThreshold").unwrap_or(ix.settings.assumptions.default_fail_threshold);
        let ttl = if ix.has_trait(s, "dns-routing") { ix.eff[s].num("dnsTtlSeconds").unwrap_or(0.0) } else { 0.0 };
        Some(hc * th + ttl)
    }

    /// The load enters at the entry units, one full share each.
    fn inject(&self, w: &mut Walk) {
        let ix = self.ix;
        let n = ix.arch.nodes.len();
        let mut entries: Vec<usize> = (0..n).filter(|&m| ix.has_trait(m, "entry")).collect();
        if entries.is_empty() {
            entries =
                (0..n).filter(|&m| ix.in_edges[m].iter().all(|&ei| ix.sem(ei) != Some(Semantics::Dependency)) && ix.out_edges[m].iter().any(|&ei| ix.sem(ei) == Some(Semantics::Dependency))).collect();
        }
        let mut units: Vec<usize> = entries.iter().map(|&m| ix.unit_of[m]).collect();
        units.sort_unstable();
        units.dedup();
        for u in units {
            w.offers[u].push(Offer { from: None, hard: true, amount: 1.0, edges: vec![] });
        }
    }

    /// Places the offers that wait for unit `v` on links, and records what arrives at its members.
    fn resolve(&self, v: usize, w: &mut Walk) {
        let offers = std::mem::take(&mut w.offers[v]);
        if offers.is_empty() {
            return;
        }
        let mixes = !self.serving[v].is_empty() && self.ix.units[v].behavior != Behavior::PrimaryReplica;
        let hops: Vec<Hop> = if mixes { self.mix(v, offers) } else { offers.into_iter().map(|o| self.place(v, o)).collect() };
        for hop in hops {
            for s in &hop.segs {
                w.inflow[s.to] += s.flow;
            }
            w.hops.push(hop);
        }
    }

    /// A unit that works and shares its load between members: each serving member is due its share of everything that
    /// arrives. The sources fill it through their own links as far as they can and the rest goes by detours to whoever they
    /// can reach, with as little of the traffic on detours as possible.
    fn mix(&self, v: usize, offers: Vec<Offer>) -> Vec<Hop> {
        let ix = self.ix;
        let targets: Vec<(usize, f64)> = self.serving[v].iter().copied().filter(|&t| self.takes(t) == Take::Pass).map(|t| (t, self.weight(t))).collect();
        if targets.is_empty() {
            return offers.into_iter().map(|o| self.place(v, o)).collect();
        }
        let supply: Vec<f64> = offers.iter().map(|o| o.amount).collect();
        let due = split(supply.iter().sum(), &targets);
        let mut arcs: Vec<(usize, usize, i64)> = vec![];
        let mut link: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for (i, o) in offers.iter().enumerate() {
            for (k, &(t, _)) in targets.iter().enumerate() {
                let own = o.edges.iter().copied().find(|&ei| ix.edge_ends[ei].map(|e| e.1) == Some(t) && self.edge_state(ei) == EdgeState::Ok);
                if let Some(ei) = own {
                    arcs.push((i, k, 0));
                    link.insert((i, k), ei);
                } else if o.from.map(|m| self.reach(m, t)).unwrap_or(true) {
                    arcs.push((i, k, 1));
                }
            }
        }
        let shipped = transport(&supply, &due, &arcs);
        offers
            .into_iter()
            .enumerate()
            .map(|(i, o)| {
                let mut segs: Vec<Seg> = vec![];
                for (k, &(to, _)) in targets.iter().enumerate() {
                    if shipped[i][k] > EPS {
                        segs.push(Seg { edge: link.get(&(i, k)).copied(), to, flow: shipped[i][k], lost: 0.0 });
                    }
                }
                // what none of the members it can reach is due: dropped on the links that lead to the unit
                let rest = o.amount - shipped[i].iter().sum::<f64>();
                if rest > EPS {
                    let ends: Vec<(usize, usize)> = o.edges.iter().filter_map(|&ei| ix.edge_ends[ei].map(|e| (ei, e.1))).collect();
                    let pool: Vec<(usize, f64)> = ends.iter().map(|&(_, to)| (to, self.weight_before(to))).collect();
                    for (&(ei, to), part) in ends.iter().zip(split(rest, &pool)) {
                        segs.push(Seg { edge: Some(ei), to, flow: 0.0, lost: part });
                    }
                }
                Hop { from: o.from, hard: o.hard, offered: o.amount, segs }
            })
            .collect()
    }

    /// A working unit whose capacity is below what arrives sheds the rest at its door.
    fn shed(&self, u: usize, w: &mut Walk) {
        let ix = self.ix;
        let Some(rps) = ix.arch.requirements.rps_peak else { return };
        if self.serving[u].is_empty() || !ix.units[u].members.iter().any(|&m| self.per_replica_rps(m).is_some()) {
            return;
        }
        let arriving = ix.units[u].members.iter().map(|&m| w.inflow[m]).sum::<f64>() * rps;
        if arriving <= EPS {
            return;
        }
        let capacity: f64 = self.serving[u].iter().filter_map(|&m| self.per_replica_rps(m).map(|per| per * self.alive[m] as f64)).sum();
        if capacity >= arriving {
            return;
        }
        let keep = (capacity / arriving).max(0.0);
        for hop in w.hops.iter_mut() {
            for seg in hop.segs.iter_mut().filter(|s| ix.unit_of[s.to] == u && s.flow > 0.0) {
                let cut = seg.flow * (1.0 - keep);
                seg.flow -= cut;
                seg.lost += cut;
            }
        }
        for &m in &ix.units[u].members {
            w.inflow[m] *= keep;
        }
    }

    /// The member sends what arrives at it on, along its dependency links, one unit at a time.
    fn forward(&self, t: usize, load: f64, w: &mut Walk) {
        let ix = self.ix;
        let u = ix.unit_of[t];
        let mut by_unit: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut cold = false;
        for &ei in &ix.out_edges[t] {
            if ix.sem(ei) != Some(Semantics::Dependency) {
                continue;
            }
            let Some((_, to)) = ix.edge_ends[ei] else { continue };
            let v = ix.unit_of[to];
            if v == u {
                continue;
            }
            // a soft dependency on a missing cache makes the datastores see cold traffic (the same rule as the capacity model)
            if self.serving[v].is_empty() && !ix.is_hard(ei) && ix.unit_has_trait(v, "cache") {
                cold = true;
            }
            by_unit.entry(v).or_default().push(ei);
        }
        for (v, edges) in by_unit {
            let (mut fan, mut fan_cold) = (0.0f64, 0.0f64);
            for &ei in &edges {
                let f = ix.edge_eff[ei].num("fanout").unwrap_or(1.0);
                fan = fan.max(f);
                fan_cold = fan_cold.max(ix.edge_eff[ei].num("fanoutColdCache").unwrap_or(f));
            }
            let uses_cold = cold && ix.unit_has_trait(v, "datastore") && !ix.unit_has_trait(v, "cache");
            let offered = load * if uses_cold { fan_cold } else { fan };
            if offered <= EPS {
                continue;
            }
            let hard = edges.iter().any(|&ei| ix.is_hard(ei));
            // a balancer that has not yet noticed the loss keeps sending to everything it had in the pool: that is decided at
            // the source and does not take part in the sharing of the unit's load
            if let Some(hop) = self.believed(t, v, &edges, offered, hard) {
                for s in &hop.segs {
                    w.inflow[s.to] += s.flow;
                }
                w.hops.push(hop);
            } else {
                w.offers[v].push(Offer { from: Some(t), hard, amount: offered, edges });
            }
        }
    }

    /// The hop of a balancer that has not yet noticed that members are gone, if that is the case.
    fn believed(&self, t: usize, v: usize, edges: &[usize], offered: f64, hard: bool) -> Option<Hop> {
        let ix = self.ix;
        let window = self.detection_window(t)?;
        if ix.units[v].behavior != Behavior::Stateless || !pending(self.horizon, window) {
            return None;
        }
        let ends: Vec<(usize, usize)> = edges.iter().filter_map(|&ei| ix.edge_ends[ei].map(|e| (ei, e.1))).collect();
        let pool: Vec<(usize, f64)> = ends.iter().map(|&(_, to)| (to, self.weight_before(to))).collect();
        let segs = ends
            .iter()
            .zip(split(offered, &pool))
            .map(|(&(ei, to), part)| {
                let works = self.edge_state(ei) == EdgeState::Ok && self.takes(to) == Take::Pass;
                let live = if works { (self.alive[to] as f64 / ix.replicas(to) as f64).min(1.0) } else { 0.0 };
                Seg { edge: Some(ei), to, flow: part * live, lost: part * (1.0 - live) }
            })
            .collect();
        Some(Hop { from: Some(t), hard, offered, segs })
    }

    /// Where an offer goes when the unit is not shared between its members: along the source's own links, else by a detour
    /// to whoever serves, else nowhere (a database cluster, or a unit that does not work).
    fn place(&self, v: usize, o: Offer) -> Hop {
        let ix = self.ix;
        let hop = |segs: Vec<Seg>| Hop { from: o.from, hard: o.hard, offered: o.amount, segs };
        let Some(from) = o.from else {
            // the entry: whoever takes it
            let cand: Vec<(usize, f64)> = ix.units[v].members.iter().copied().filter(|&m| self.takes(m) != Take::Refuse).map(|m| (m, self.weight(m))).collect();
            let segs = cand
                .iter()
                .zip(split(o.amount, &cand))
                .map(|(&(to, _), part)| if self.takes(to) == Take::Pass { Seg { edge: None, to, flow: part, lost: 0.0 } } else { Seg { edge: None, to, flow: 0.0, lost: part } })
                .collect();
            return hop(segs);
        };
        let ends: Vec<(usize, usize)> = o.edges.iter().filter_map(|&ei| ix.edge_ends[ei].map(|e| (ei, e.1))).collect();

        // the source's own links that take the traffic now
        let cand: Vec<(usize, usize, f64)> =
            ends.iter().copied().filter(|&(ei, to)| self.edge_state(ei) == EdgeState::Ok && self.takes(to) != Take::Refuse).map(|(ei, to)| (ei, to, self.weight(to))).collect();
        if !cand.is_empty() {
            let parts = split(o.amount, &cand.iter().map(|c| (c.1, c.2)).collect::<Vec<_>>());
            let segs = cand
                .iter()
                .zip(parts)
                .map(|(&(ei, to, _), part)| if self.takes(to) == Take::Pass { Seg { edge: Some(ei), to, flow: part, lost: 0.0 } } else { Seg { edge: Some(ei), to, flow: 0.0, lost: part } })
                .collect();
            return hop(segs);
        }

        // none does, but the unit works: a detour to whoever serves
        let serving: Vec<(usize, f64)> = self.serving[v].iter().copied().filter(|&s| self.reach(from, s) && self.takes(s) == Take::Pass).map(|s| (s, self.weight(s))).collect();
        if !serving.is_empty() {
            let segs = serving.iter().zip(split(o.amount, &serving)).map(|(&(to, _), part)| Seg { edge: None, to, flow: part, lost: 0.0 }).collect();
            return hop(segs);
        }

        // nothing takes it: it is dropped on the links that lead there
        let pool: Vec<(usize, f64)> = ends.iter().map(|&(_, to)| (to, self.weight_before(to))).collect();
        let segs = ends.iter().zip(split(o.amount, &pool)).map(|(&(ei, to), part)| Seg { edge: Some(ei), to, flow: 0.0, lost: part }).collect();
        hop(segs)
    }

    /// Survival of the requests, bottom up, and the maps the UI draws.
    fn settle(&self, w: Walk) -> Traffic {
        let ix = self.ix;
        let n = ix.arch.nodes.len();
        let mut by_source: Vec<Vec<usize>> = vec![vec![]; n];
        for (h, hop) in w.hops.iter().enumerate() {
            if let Some(m) = hop.from {
                by_source[m].push(h);
            }
        }
        // a call survives when it reaches a member that takes it and that member survives in turn
        let call = |hop: &Hop, alive_share: &[f64]| -> f64 {
            if hop.offered <= EPS {
                return 1.0;
            }
            (hop.segs.iter().map(|s| s.flow * alive_share[s.to]).sum::<f64>() / hop.offered).clamp(0.0, 1.0)
        };
        let mut survives = vec![1.0f64; n];
        // dependencies first
        for &u in &ix.unit_order {
            for &m in &ix.units[u].members {
                let mut s = 1.0;
                for &h in &by_source[m] {
                    if w.hops[h].hard {
                        s *= call(&w.hops[h], &survives);
                    }
                }
                survives[m] = s;
            }
        }
        let entry: Vec<f64> = w.hops.iter().filter(|h| h.from.is_none()).map(|h| call(h, &survives)).collect();
        let served = if entry.is_empty() { 1.0 } else { entry.iter().sum::<f64>() / entry.len() as f64 };

        let mut flows: BTreeMap<String, f64> = BTreeMap::new();
        let mut lost: BTreeMap<String, f64> = BTreeMap::new();
        let mut missed: BTreeMap<String, f64> = BTreeMap::new();
        let mut detours: BTreeMap<(usize, usize), (f64, f64)> = BTreeMap::new();
        for hop in &w.hops {
            for s in &hop.segs {
                match (s.edge, hop.from) {
                    (Some(ei), _) => {
                        let id = &ix.arch.edges[ei].id;
                        *flows.entry(id.clone()).or_default() += s.flow;
                        *(if hop.hard { &mut lost } else { &mut missed }).entry(id.clone()).or_default() += s.lost;
                    }
                    (None, Some(from)) => {
                        let e = detours.entry((from, s.to)).or_default();
                        e.0 += s.flow;
                        e.1 += s.lost;
                    }
                    (None, None) => {}
                }
            }
        }
        let tidy = |m: BTreeMap<String, f64>| -> BTreeMap<String, f64> { m.into_iter().map(|(k, v)| (k, round(v))).filter(|(_, v)| *v > 0.0).collect() };
        Traffic {
            flows: tidy(flows),
            lost: tidy(lost),
            missed: tidy(missed),
            detours: detours
                .into_iter()
                .map(|((from, to), (flow, lost))| Detour { from: ix.arch.nodes[from].id.clone(), to: ix.arch.nodes[to].id.clone(), flow: round(flow), lost: round(lost) })
                .filter(|d| d.flow > 0.0 || d.lost > 0.0)
                .collect(),
            served: round(served),
        }
    }
}

/// `total` split in proportion to the weights; equally when they are all zero.
fn split(total: f64, parts: &[(usize, f64)]) -> Vec<f64> {
    let sum: f64 = parts.iter().map(|p| p.1.max(0.0)).sum();
    if parts.is_empty() {
        return vec![];
    }
    if sum <= EPS {
        return vec![total / parts.len() as f64; parts.len()];
    }
    parts.iter().map(|p| total * p.1.max(0.0) / sum).collect()
}

/// Nine decimals: far below anything the picture shows, and the JSON stays short and the same on every platform.
fn round(v: f64) -> f64 {
    let r = (v * 1e9).round() / 1e9;
    if r.abs() < 1e-9 {
        0.0
    } else {
        r
    }
}

/// What a target takes beyond its share costs more than any detour: the shares say how the load is spread when everybody
/// can reach everybody, they are not a limit when somebody cannot.
const OVERFLOW_COST: i64 = 1000;

/// Ships `supply[i]` from the sources to the targets along `arcs` (source, target, cost): all that can be shipped, and of
/// that as much as possible within the share `due[k]` of every target, at the least cost (successive shortest paths). The
/// result is the amount on every pair.
fn transport(supply: &[f64], due: &[f64], arcs: &[(usize, usize, i64)]) -> Vec<Vec<f64>> {
    struct Edge {
        to: usize,
        cap: f64,
        cost: i64,
    }
    let (n, m) = (supply.len(), due.len());
    let (source, sink) = (0, n + m + 1);
    let mut edges: Vec<Edge> = vec![];
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n + m + 2];
    let mut add = |a: usize, b: usize, cap: f64, cost: i64| {
        adj[a].push(edges.len());
        edges.push(Edge { to: b, cap, cost });
        adj[b].push(edges.len());
        edges.push(Edge { to: a, cap: 0.0, cost: -cost });
    };
    for (i, &s) in supply.iter().enumerate() {
        add(source, 1 + i, s, 0);
    }
    for (k, &d) in due.iter().enumerate() {
        add(1 + n + k, sink, d, 0);
        add(1 + n + k, sink, f64::INFINITY, OVERFLOW_COST);
    }
    // every edge is stored with its reverse: the arcs start after the edges of the supplies, the shares and the overflows
    let first_arc = 2 * (n + 2 * m);
    for &(i, k, cost) in arcs {
        add(1 + i, 1 + n + k, f64::INFINITY, cost);
    }
    // every round saturates an edge, so the number of rounds is bounded; the cap only guards against rounding
    for _ in 0..(arcs.len() + n + m) * 4 + 64 {
        let mut dist = vec![i64::MAX; n + m + 2];
        let mut back = vec![usize::MAX; n + m + 2];
        let mut queued = vec![false; n + m + 2];
        let mut queue = std::collections::VecDeque::from([source]);
        dist[source] = 0;
        while let Some(a) = queue.pop_front() {
            queued[a] = false;
            for &e in &adj[a] {
                let Edge { to, cap, cost } = edges[e];
                if cap > EPS && dist[a] + cost < dist[to] {
                    dist[to] = dist[a] + cost;
                    back[to] = e;
                    if !queued[to] {
                        queued[to] = true;
                        queue.push_back(to);
                    }
                }
            }
        }
        if dist[sink] == i64::MAX {
            break;
        }
        let mut push = f64::INFINITY;
        let mut at = sink;
        while at != source {
            push = push.min(edges[back[at]].cap);
            at = edges[back[at] ^ 1].to;
        }
        let mut at = sink;
        while at != source {
            edges[back[at]].cap -= push;
            edges[back[at] ^ 1].cap += push;
            at = edges[back[at] ^ 1].to;
        }
    }
    let mut shipped = vec![vec![0.0; m]; n];
    for (a, &(i, k, _)) in arcs.iter().enumerate() {
        shipped[i][k] += edges[first_arc + 2 * a + 1].cap;
    }
    shipped
}
