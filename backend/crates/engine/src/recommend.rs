//! What to improve first: the engine's own fixes for the open findings, each one *tried* – applied to a copy, evaluated again –
//! and ranked by what it measurably does: the critical findings it closes, the score it adds, the availability it gives, the
//! money it costs. A fix that would not make the design better never reaches the list, so nothing here is a guess about an effect.
//!
//! The fixes are the ones the rules already produce (`Finding::fixes`); this module only measures and orders them. Pure and
//! deterministic like the rest of the engine.

use crate::config::{EngineConfig, Waiver};
use crate::evaluate::{evaluate, Finding, Report, Status};
use crate::model::{Architecture, ValidationLimits};
use crate::ops::{apply_ops, Op};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// A number before and after a change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Change<T> {
    pub before: T,
    pub after: T,
}

/// What a change does to the design, measured by evaluating it again.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub score: Change<f64>,
    /// Open critical findings.
    pub critical: Change<usize>,
    /// Open risk findings.
    pub risk: Change<usize>,
    /// Expected availability (0…1), when the estimate exists on both sides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<Change<f64>>,
    /// Expected downtime a year, seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downtime_seconds: Option<Change<f64>>,
    /// Monthly cost estimate.
    pub cost: Change<f64>,
    /// Open critical / risk findings (fingerprints) that are gone.
    pub resolved: Vec<String>,
    /// Open critical / risk findings (fingerprints) that came in.
    pub introduced: Vec<String>,
}

/// One change worth making: a fix of the engine for a finding, and what it does.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    /// The fix's id.
    pub id: String,
    /// The finding it answers (fingerprint) and its rule.
    pub finding_id: String,
    pub rule_id: String,
    /// Severity of that finding.
    pub status: Status,
    /// The fix's kind (key into the rule's `fixLabels`).
    pub kind: String,
    /// Placeholders of the label: the finding's variables with the fix's own.
    pub vars: Map<String, Value>,
    pub ops: Vec<Op>,
    pub effect: Effect,
}

/// Several fixes applied one after another, each kept only when the result was better: what "apply them all" does.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    /// The fixes that were kept.
    pub ids: Vec<String>,
    pub ops: Vec<Op>,
    pub effect: Effect,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub items: Vec<Recommendation>,
    pub bundle: Option<Bundle>,
    /// Open critical / risk findings (fingerprints) that no fix of the engine closes: they are for a person. Said only when every fix
    /// was tried (not when `truncated`, or when `recommendations.kinds` keeps some from being tried).
    pub manual: Vec<String>,
    /// Fixes that were tried.
    pub considered: usize,
    /// There were more fixes than `recommendations.maxCandidates`: the rest were not tried.
    pub truncated: bool,
}

fn open(r: &Report) -> impl Iterator<Item = &Finding> {
    r.findings.iter().filter(|f| f.accepted.is_none() && matches!(f.status, Status::Critical | Status::Risk))
}

fn count(r: &Report, st: Status) -> usize {
    open(r).filter(|f| f.status == st).count()
}

/// Is `next` a better design than `cur`: fewer critical findings, or as many and a higher score.
pub fn improves(next: &Report, cur: &Report) -> bool {
    let (n, c) = (count(next, Status::Critical), count(cur, Status::Critical));
    n < c || (n == c && next.score.value > cur.score.value)
}

/// What going from `before` to `after` does.
pub fn effect(before: &Report, after: &Report) -> Effect {
    let ids = |r: &Report| open(r).map(|f| f.id.clone()).collect::<BTreeSet<_>>();
    let (b, a) = (ids(before), ids(after));
    let pair = |x: Option<&crate::reliability::Availability>, y: Option<&crate::reliability::Availability>| match (x, y) {
        (Some(x), Some(y)) if !x.service_down && !y.service_down => {
            Some((Change { before: x.estimate, after: y.estimate }, Change { before: x.downtime_seconds_per_year, after: y.downtime_seconds_per_year }))
        }
        _ => None,
    };
    let av = pair(before.availability.as_ref(), after.availability.as_ref());
    Effect {
        score: Change { before: before.score.value, after: after.score.value },
        critical: Change { before: count(before, Status::Critical), after: count(after, Status::Critical) },
        risk: Change { before: count(before, Status::Risk), after: count(after, Status::Risk) },
        availability: av.map(|x| x.0),
        downtime_seconds: av.map(|x| x.1),
        cost: Change { before: before.cost.total, after: after.cost.total },
        resolved: b.difference(&a).cloned().collect(),
        introduced: a.difference(&b).cloned().collect(),
    }
}

/// Order of importance: critical findings closed, score gained, availability gained, the least money, then the id.
fn rank(a: &Recommendation, b: &Recommendation) -> std::cmp::Ordering {
    let crit = |r: &Recommendation| r.effect.critical.before as i64 - r.effect.critical.after as i64;
    let gain = |r: &Recommendation| r.effect.score.after - r.effect.score.before;
    let avail = |r: &Recommendation| r.effect.availability.map(|c| c.after - c.before).unwrap_or(0.0);
    let cost = |r: &Recommendation| r.effect.cost.after - r.effect.cost.before;
    let cmp = |x: f64, y: f64| x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
    crit(b).cmp(&crit(a)).then(cmp(gain(b), gain(a))).then(cmp(avail(b), avail(a))).then(cmp(cost(a), cost(b))).then(a.id.cmp(&b.id))
}

/// Try the fixes of the open findings of `arch` (whose report is `base`) and say which are worth making, in what order.
pub fn recommend_from(arch: &Architecture, base: &Report, cfg: &EngineConfig, waivers: &[Waiver], limits: &ValidationLimits) -> Plan {
    let set = &cfg.settings.recommendations;
    let mut plan = Plan::default();
    if !set.enabled {
        return plan;
    }
    let allowed = |kind: &str| set.kinds.is_empty() || set.kinds.iter().any(|k| k == kind);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut candidates: Vec<(&Finding, &crate::evaluate::Fix)> = vec![];
    for f in open(base) {
        for fix in f.fixes.iter().filter(|x| !x.ops.is_empty() && allowed(&x.kind)) {
            // the same fix is not offered for two findings
            if seen.insert(fix.id.clone()) {
                candidates.push((f, fix));
            }
        }
    }
    plan.truncated = candidates.len() > set.max_candidates;
    candidates.truncate(set.max_candidates);
    let tried: BTreeSet<&str> = candidates.iter().map(|(_, fix)| fix.id.as_str()).collect();

    let mut helped: BTreeSet<String> = BTreeSet::new();
    for (f, fix) in candidates {
        plan.considered += 1;
        let Ok(out) = apply_ops(arch, &fix.ops, &cfg.catalog, limits) else { continue };
        let next = evaluate(&out.architecture, cfg, waivers);
        if !improves(&next, base) {
            continue;
        }
        let eff = effect(base, &next);
        helped.extend(eff.resolved.iter().cloned());
        let mut vars = f.vars.clone();
        vars.extend(fix.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
        plan.items.push(Recommendation { id: fix.id.clone(), finding_id: f.id.clone(), rule_id: f.rule_id.clone(), status: f.status, kind: fix.kind.clone(), vars, ops: fix.ops.clone(), effect: eff });
    }
    plan.items.sort_by(rank);
    // For a person: every fix of the engine was tried and none made things better. When some were not (past the limit, or a kind that
    // may not be recommended) nobody can say what is left, so nothing is claimed.
    let everything_tried = open(base).all(|f| f.fixes.iter().filter(|x| !x.ops.is_empty()).all(|x| tried.contains(x.id.as_str())));
    if everything_tried {
        plan.manual = open(base).filter(|f| !helped.contains(&f.id)).map(|f| f.id.clone()).collect();
    }

    // all of them, in that order, each kept only when the result is better than what the ones before it made
    if plan.items.len() >= 2 {
        let mut cur = arch.clone();
        let mut cur_report: Option<Report> = None;
        let mut ids = vec![];
        let mut ops: Vec<Op> = vec![];
        for item in &plan.items {
            let Ok(out) = apply_ops(&cur, &item.ops, &cfg.catalog, limits) else { continue };
            let next = evaluate(&out.architecture, cfg, waivers);
            if improves(&next, cur_report.as_ref().unwrap_or(base)) {
                cur = out.architecture;
                cur_report = Some(next);
                ids.push(item.id.clone());
                ops.extend(item.ops.iter().cloned());
            }
        }
        if let (Some(r), true) = (cur_report, ids.len() >= 2) {
            plan.bundle = Some(Bundle { ids, ops, effect: effect(base, &r) });
        }
    }
    plan
}

/// [`recommend_from`] for a design whose report is not at hand.
pub fn recommend(arch: &Architecture, cfg: &EngineConfig, waivers: &[Waiver], limits: &ValidationLimits) -> Plan {
    // switched off: not even the design's own check is run
    if !cfg.settings.recommendations.enabled {
        return Plan::default();
    }
    let base = evaluate(arch, cfg, waivers);
    recommend_from(arch, &base, cfg, waivers, limits)
}
