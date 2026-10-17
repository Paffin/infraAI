//! Expected availability: how much of the year the service is down, from what can fail, how often, and what the
//! architecture does about it.
//!
//! The estimate is built from the same simulator the rest of the engine uses. Every failure the architecture can
//! meet (one instance, a whole service, a rack, a site, a link, a disaster, a provider's incident) is a scenario with a
//! rate of its own (events per year, from the engine settings `reliability`, a kind's or variant's own numbers, a
//! data center's tier or a provider's SLA). The simulator says how long the service is down when it happens: until
//! the automatic failover is done, until an operator has promoted a replica, until the data is restored, or until the
//! failed thing is repaired when nothing recovers it – and how many of the requests fail while that lasts. The year's
//! downtime is the sum of rate × downtime over the scenarios (first order: failures are rare enough that two at once
//! add up to very little, and the shared causes – a region, a carrier – are scenarios of their own).
//!
//! Nothing here is measured. Every rate is an assumption the architect can correct, and the report carries them, so
//! that the number is never shown without what it rests on.

use crate::catalog::{ReliabilityDef, ZoneLevel};
use crate::config::ReliabilitySettings;
use crate::evaluate::Ctx;
use crate::model::PropsExt;
use crate::sim::{scenario_set, Interruption, RecoveryKind, Scenario, ServiceState, SimResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What fails.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    /// One instance (a pod, a VM, a process).
    Instance,
    /// A whole service at once (a bad release, an expired certificate).
    Unit,
    /// A service a provider runs, down within its SLA.
    Provider,
    Rack,
    /// A data center, an availability zone, a cloud region as a site.
    Site,
    /// All paths between two sites.
    Link,
    /// A site (or a group of sites) cut off the network.
    Isolation,
    /// A disaster that takes a whole disaster domain.
    Geo,
}

/// One kind of failure and what it costs a year.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contribution {
    pub scenario: Scenario,
    pub cause: Cause,
    /// Display names of what fails.
    pub failed: Vec<String>,
    /// How often it happens, per year.
    pub per_year: f64,
    /// Seconds of outage (partial outages counted as the equivalent of a full one) one failure costs.
    pub downtime_seconds: f64,
    /// `per_year` × `downtime_seconds`.
    pub seconds_per_year: f64,
    /// What the downtime consists of: `repair` (nothing recovers it, it waits for the repair), `recovery` (a restore or
    /// an operator), `interruption` (a failover or the detection of a dead member), `capacity` (what is left cannot
    /// carry the load).
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CauseShare {
    pub cause: Cause,
    pub seconds_per_year: f64,
    /// Of the year's downtime.
    pub share: f64,
}

/// A service that the downtime comes from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitShare {
    pub unit: String,
    pub name: String,
    pub seconds_per_year: f64,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Availability {
    /// Expected availability, 0…1.
    pub estimate: f64,
    /// The service does not work even while nothing has failed (a dependency is missing): it is down all the time, and
    /// the failures cannot be told apart.
    pub service_down: bool,
    pub downtime_seconds_per_year: f64,
    /// The target from the requirements (0…1).
    pub target: Option<f64>,
    /// Downtime per year the target allows.
    pub budget_seconds_per_year: Option<f64>,
    /// By what fails, the costliest first.
    pub causes: Vec<CauseShare>,
    /// By service, the costliest first.
    pub units: Vec<UnitShare>,
    /// The costliest single failures.
    pub top: Vec<Contribution>,
    /// Failures looked at.
    pub scenarios: usize,
    /// Some scenarios were left out (the cap on scenarios): the estimate is then optimistic.
    pub truncated: bool,
    /// The assumptions it rests on.
    pub basis: ReliabilitySettings,
}

/// Where the estimate stands against the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The requirements name no target.
    NoTarget,
    /// Comfortably inside the downtime budget.
    Within,
    /// Inside the budget, but past the share (`warnPercent`) that leaves room for the estimate to be wrong.
    Close,
    /// Over the budget.
    Over,
}

impl Availability {
    /// Share of the downtime budget the estimate uses; `None` without a target.
    pub fn budget_used(&self) -> Option<f64> {
        self.budget_seconds_per_year.map(|b| if b > 0.0 { self.downtime_seconds_per_year / b } else { f64::INFINITY })
    }

    pub fn standing(&self) -> Standing {
        match self.budget_used() {
            None => Standing::NoTarget,
            Some(u) if u > 1.0 => Standing::Over,
            Some(u) if u > self.basis.warn_percent / 100.0 => Standing::Close,
            Some(_) => Standing::Within,
        }
    }
}

/// The target of a requirement given as a percentage (`99.95`); a value up to 1 is taken as a fraction.
pub fn target_fraction(slo: f64) -> f64 {
    if slo > 1.0 {
        slo / 100.0
    } else {
        slo
    }
}

/// Seconds a set of overlapping interruptions takes counted as outage: at every moment the share of the requests that
/// fail is the chance that a request meets at least one of the interruptions still going, `1 − Π(1 − share)`.
pub fn outage_seconds(items: &[Interruption]) -> f64 {
    let mut ends: Vec<(f64, f64)> = items.iter().filter(|i| i.share > 0.0 && i.seconds > 0.0 && i.seconds.is_finite()).map(|i| (i.seconds, i.share.clamp(0.0, 1.0))).collect();
    if ends.is_empty() {
        return 0.0;
    }
    ends.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut total = 0.0;
    let mut prev = 0.0;
    for k in 0..ends.len() {
        // until `ends[k].0` the interruptions k.. are all still going
        let ok: f64 = ends[k..].iter().map(|e| 1.0 - e.1).product();
        total += (ends[k].0 - prev) * (1.0 - ok);
        prev = ends[k].0;
    }
    total
}

/// A rate or a repair time from a map keyed by zone type, then by level (`rack`, `site`, `geo`).
fn by_zone(map: &BTreeMap<String, f64>, zone_type: &str, level: ZoneLevel) -> Option<f64> {
    let level_key = match level {
        ZoneLevel::Rack => "rack",
        ZoneLevel::Site => "site",
        ZoneLevel::Geo => "geo",
        ZoneLevel::Group => return None,
    };
    map.get(zone_type).or_else(|| map.get(level_key)).copied()
}

/// What one scenario costs when it happens, in seconds of outage, and what that consists of. `baseline` is the share of
/// the requests the architecture can carry when nothing has failed (at most 1): a design that is too small for its load
/// is the capacity rules' business, and only what a failure takes away on top of it is the failure's cost.
fn downtime(res: &SimResult, repair: f64, baseline: f64, cfg: &ReliabilitySettings) -> (f64, &'static str) {
    if res.state == ServiceState::Down {
        return match (res.recovery.kind, res.recovery.seconds) {
            (RecoveryKind::Never, _) | (RecoveryKind::None, _) | (_, None) => (repair, "repair"),
            (_, Some(s)) => (s, "recovery"),
        };
    }
    let interrupted = outage_seconds(&res.interruptions);
    let short = if cfg.partial_capacity { res.capacity_ratio.map(|r| (baseline - r.min(1.0)).clamp(0.0, 1.0) * repair).unwrap_or(0.0) } else { 0.0 };
    let total = interrupted + short;
    if total <= 0.0 {
        (0.0, "none")
    } else if short > interrupted {
        (total, "capacity")
    } else {
        (total, "interruption")
    }
}

struct Failure {
    scenario: Scenario,
    cause: Cause,
    per_year: f64,
    /// Seconds the failed thing stays failed when nothing recovers it.
    repair: f64,
}

impl Failure {
    /// A failure with a rate and a repair time that mean something (a configured number may be anything).
    fn new(scenario: Scenario, cause: Cause, per_year: f64, repair: f64) -> Option<Failure> {
        (per_year.is_finite() && per_year > 0.0).then(|| Failure { scenario, cause, per_year, repair: if repair.is_finite() { repair.max(0.0) } else { 0.0 } })
    }
}

fn provider_managed(ctx: &Ctx<'_>, m: usize) -> bool {
    let traits = &ctx.cfg.settings.reliability.provider.traits;
    !traits.is_empty() && ctx.ix.traits[m].iter().any(|t| traits.contains(t))
}

/// Availability a provider promises for the node's product.
fn provider_sla(ctx: &Ctx<'_>, m: usize, rel: &ReliabilityDef) -> f64 {
    let cfg = &ctx.cfg.settings.reliability.provider;
    rel.sla.or_else(|| ctx.ix.kind[m].and_then(|k| k.function.as_deref()).and_then(|f| cfg.by_function.get(f).copied())).unwrap_or(cfg.default).clamp(0.0, 1.0)
}

fn failures(ctx: &Ctx<'_>) -> (Vec<Failure>, bool) {
    let ix = &ctx.ix;
    let cfg = &ctx.cfg.settings.reliability;
    let year = cfg.days_per_year * 86_400.0;
    let mut out: Vec<Failure> = vec![];

    // what happens to an instance, to a service a provider runs, and to a service as a whole
    for (m, n) in ix.arch.nodes.iter().enumerate() {
        if ix.has_trait(m, "external") {
            continue;
        }
        let rel = ix.cat.reliability_of(&n.kind, ix.variant[m]);
        if provider_managed(ctx, m) {
            // the provider's SLA, as incidents of a service that is down for a while: its instances, its failover and
            // its upgrades are the provider's, what the architecture does about an outage of the whole service is ours
            let incident = cfg.provider.incident_seconds.max(1.0);
            let per_year = (1.0 - provider_sla(ctx, m, &rel)) * year / incident;
            out.extend(Failure::new(Scenario::Node { node_id: n.id.clone() }, Cause::Provider, per_year, incident));
            continue;
        }
        let per_year = rel.failures_per_year.unwrap_or(cfg.failures.instance) * ix.replicas(m) as f64;
        out.extend(Failure::new(Scenario::Instance { node_id: n.id.clone(), count: 1 }, Cause::Instance, per_year, rel.repair_seconds.unwrap_or(cfg.repair.instance)));
    }
    for unit in &ix.units {
        let members: Vec<usize> = unit.members.iter().copied().filter(|&m| !ix.has_trait(m, "external") && !provider_managed(ctx, m)).collect();
        // one instance is the instance scenario already; the whole service goes down at once only when there are more
        let instances = ix.instances(members.iter().copied().filter(|&m| !ix.is_witness(m)));
        if instances >= 2 {
            let per_year = members.iter().map(|&m| ix.cat.reliability_of(&ix.arch.nodes[m].kind, ix.variant[m]).common_mode_per_year.unwrap_or(cfg.failures.unit)).fold(0.0_f64, f64::max);
            out.extend(Failure::new(Scenario::Unit { unit: unit.key.clone() }, Cause::Unit, per_year, cfg.repair.unit));
        }
    }

    // failure domains, links, isolation, disasters: the scenarios the architecture yields
    let set = scenario_set(ix);
    for sc in set.scenarios {
        match &sc {
            Scenario::Zone { zone_id } => {
                let Some(&zi) = ix.zone_ix.get(zone_id.as_str()) else { continue };
                let z = &ix.arch.zones[zi];
                let level = ix.cat.zone_type(&z.zone_type).map(|t| t.level).unwrap_or(ZoneLevel::Group);
                let Some(mut per_year) = by_zone(&cfg.failures.zone, &z.zone_type, level) else { continue };
                let repair = by_zone(&cfg.repair.zone, &z.zone_type, level).unwrap_or(cfg.repair.instance);
                if level == ZoneLevel::Site {
                    if let Some(f) = z.props.text("tier").and_then(|t| cfg.tier_factor.get(t.trim()).copied()) {
                        per_year *= f;
                    }
                }
                let cause = match level {
                    ZoneLevel::Rack => Cause::Rack,
                    _ => Cause::Site,
                };
                out.extend(Failure::new(sc.clone(), cause, per_year, repair));
            }
            Scenario::Link { .. } => {
                out.extend(Failure::new(sc.clone(), Cause::Link, cfg.failures.link, cfg.repair.link));
            }
            Scenario::Isolate { zone_ids } => {
                let per_year = cfg.failures.isolation * if zone_ids.len() > 1 { cfg.isolation_group_factor } else { 1.0 };
                out.extend(Failure::new(sc.clone(), Cause::Isolation, per_year, cfg.repair.isolation));
            }
            Scenario::Geo { .. } => {
                out.extend(Failure::new(sc.clone(), Cause::Geo, cfg.failures.geo, cfg.repair.geo));
            }
            _ => {}
        }
    }
    (out, set.truncated)
}

/// The service a failure costs most: the one with the biggest share of the interrupted requests, else the root cause
/// of what is down, else the failed node's own.
fn culprit(ctx: &Ctx<'_>, sc: &Scenario, res: &SimResult) -> Option<(String, String)> {
    let ix = &ctx.ix;
    let unit_of_key = |key: &str| ix.unit_ix.get(key).map(|&u| (ix.units[u].key.clone(), ix.units[u].name.clone()));
    if let Some(i) = res.interruptions.iter().filter(|i| i.share > 0.0).max_by(|a, b| (a.share * a.seconds).partial_cmp(&(b.share * b.seconds)).unwrap_or(std::cmp::Ordering::Equal)) {
        if let Some(u) = unit_of_key(&i.unit) {
            return Some(u);
        }
    }
    if let Some(name) = res.down_units.first() {
        if let Some(u) = ix.units.iter().find(|u| &u.name == name) {
            return Some((u.key.clone(), u.name.clone()));
        }
    }
    match sc {
        Scenario::Instance { node_id, .. } | Scenario::Node { node_id } => ix.node_ix.get(node_id.as_str()).map(|&m| (ix.units[ix.unit_of[m]].key.clone(), ix.units[ix.unit_of[m]].name.clone())),
        Scenario::Unit { unit } => unit_of_key(unit),
        _ => None,
    }
}

/// The estimate, or `None` when it is switched off or there is no service to estimate (no entry point and no
/// dependency edge).
pub fn estimate(ctx: &Ctx<'_>) -> Option<Availability> {
    let cfg = &ctx.cfg.settings.reliability;
    let ix = &ctx.ix;
    if !cfg.enabled || cfg.days_per_year <= 0.0 {
        return None;
    }
    let has_entry = (0..ix.arch.nodes.len()).any(|m| ix.has_trait(m, "entry"));
    if ix.arch.nodes.is_empty() || (!has_entry && ix.arch.edges.is_empty()) {
        return None;
    }
    let year = cfg.days_per_year * 86_400.0;
    let target = ix.arch.requirements.availability_slo.map(target_fraction);
    let budget = target.map(|t| ((1.0 - t) * year).max(0.0));

    // a service that does not work even while nothing has failed is down all the time; what fails on top of that
    // changes nothing about it
    let base = ctx.sim(&Scenario::None);
    if base.state == ServiceState::Down {
        return Some(Availability {
            estimate: 0.0,
            service_down: true,
            downtime_seconds_per_year: year,
            target,
            budget_seconds_per_year: budget,
            causes: vec![],
            units: base
                .down_units
                .iter()
                .filter_map(|name| ix.units.iter().find(|u| &u.name == name))
                .map(|u| UnitShare { unit: u.key.clone(), name: u.name.clone(), seconds_per_year: year, share: 1.0 })
                .take(cfg.top.max(1))
                .collect(),
            top: vec![],
            scenarios: 0,
            truncated: false,
            basis: cfg.clone(),
        });
    }
    let baseline = base.capacity_ratio.map(|r| r.min(1.0)).unwrap_or(1.0);
    let (events, truncated) = failures(ctx);

    let mut all: Vec<Contribution> = vec![];
    let mut by_unit: BTreeMap<String, (String, f64)> = BTreeMap::new();
    for f in &events {
        let res = ctx.sim(&f.scenario);
        let (down, basis) = downtime(&res, f.repair, baseline, cfg);
        if down <= 0.0 {
            continue;
        }
        let per_year_seconds = f.per_year * down;
        if let Some((key, name)) = culprit(ctx, &f.scenario, &res) {
            by_unit.entry(key).or_insert((name, 0.0)).1 += per_year_seconds;
        }
        all.push(Contribution {
            scenario: f.scenario.clone(),
            cause: f.cause,
            failed: res.failed.clone(),
            per_year: f.per_year,
            downtime_seconds: down,
            seconds_per_year: per_year_seconds,
            basis: basis.into(),
        });
    }
    let total: f64 = all.iter().map(|c| c.seconds_per_year).sum();

    let mut causes: BTreeMap<Cause, f64> = BTreeMap::new();
    for c in &all {
        *causes.entry(c.cause).or_insert(0.0) += c.seconds_per_year;
    }
    let share = |v: f64| if total > 0.0 { v / total } else { 0.0 };
    let mut causes: Vec<CauseShare> = causes.into_iter().map(|(cause, s)| CauseShare { cause, seconds_per_year: s, share: share(s) }).collect();
    causes.sort_by(|a, b| b.seconds_per_year.partial_cmp(&a.seconds_per_year).unwrap_or(std::cmp::Ordering::Equal).then(a.cause.cmp(&b.cause)));
    let mut units: Vec<UnitShare> = by_unit.into_iter().map(|(unit, (name, s))| UnitShare { unit, name, seconds_per_year: s, share: share(s) }).collect();
    units.sort_by(|a, b| b.seconds_per_year.partial_cmp(&a.seconds_per_year).unwrap_or(std::cmp::Ordering::Equal).then(a.unit.cmp(&b.unit)));
    units.truncate(cfg.top.max(1));
    all.sort_by(|a, b| b.seconds_per_year.partial_cmp(&a.seconds_per_year).unwrap_or(std::cmp::Ordering::Equal).then(a.scenario.key().cmp(&b.scenario.key())));
    let scenarios = events.len();
    all.truncate(cfg.top.max(1));

    Some(Availability {
        estimate: (1.0 - total / year).clamp(0.0, 1.0),
        service_down: false,
        downtime_seconds_per_year: total,
        target,
        budget_seconds_per_year: budget,
        causes,
        units,
        top: all,
        scenarios,
        truncated,
        basis: cfg.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::InterruptionKind;

    fn int(seconds: f64, share: f64) -> Interruption {
        Interruption { unit: "u".into(), kind: InterruptionKind::Failover, seconds, share }
    }

    #[test]
    fn an_interruption_costs_its_length_times_its_share() {
        assert_eq!(outage_seconds(&[]), 0.0);
        assert!((outage_seconds(&[int(35.0, 1.0)]) - 35.0).abs() < 1e-9);
        assert!((outage_seconds(&[int(30.0, 0.125)]) - 3.75).abs() < 1e-9);
        assert_eq!(outage_seconds(&[int(30.0, 0.0), int(0.0, 1.0)]), 0.0);
    }

    #[test]
    fn overlapping_interruptions_are_not_added_twice() {
        // a promotion (all requests, 35 s) and a rerouting (an eighth, 30 s) at the same time: 35 s, not 38.75
        assert!((outage_seconds(&[int(35.0, 1.0), int(30.0, 0.125)]) - 35.0).abs() < 1e-9);
        // two partial ones: the requests that meet at least one of them, 1 - 0.9 * 0.875 = 0.2125 of 30 s
        assert!((outage_seconds(&[int(30.0, 0.1), int(30.0, 0.125)]) - 30.0 * 0.2125).abs() < 1e-9);
        // different lengths: 0..20 s both going, 20..40 s only the second
        let want = 20.0 * (1.0 - 0.5 * 0.5) + 20.0 * 0.5;
        assert!((outage_seconds(&[int(20.0, 0.5), int(40.0, 0.5)]) - want).abs() < 1e-9);
    }

    #[test]
    fn a_target_may_be_given_as_a_percentage_or_a_fraction() {
        assert!((target_fraction(99.95) - 0.9995).abs() < 1e-12);
        assert!((target_fraction(0.9995) - 0.9995).abs() < 1e-12);
        assert_eq!(target_fraction(100.0), 1.0);
    }

    #[test]
    fn zone_rates_come_by_type_first_then_by_level() {
        let mut m = BTreeMap::new();
        m.insert("dc".to_string(), 0.13);
        m.insert("site".to_string(), 0.1);
        m.insert("rack".to_string(), 0.3);
        assert_eq!(by_zone(&m, "dc", ZoneLevel::Site), Some(0.13));
        assert_eq!(by_zone(&m, "az", ZoneLevel::Site), Some(0.1));
        assert_eq!(by_zone(&m, "rack", ZoneLevel::Rack), Some(0.3));
        assert_eq!(by_zone(&m, "region", ZoneLevel::Geo), None);
        assert_eq!(by_zone(&m, "group", ZoneLevel::Group), None);
    }
}
