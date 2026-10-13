//! Rule orchestration: runs every enabled rule, applies waivers (accepted risks), computes the
//! score, the objective tiles and the standard scenario summary.
//!
//! Result statuses are deliberately *not* boolean: `critical`, `risk`, `no_data`, `ok`, `na`.
//! A rule whose premises cannot be verified reports `no_data` instead of a false green.

use crate::checks;
use crate::config::{CheckType, EngineConfig, Severity, Waiver};
use crate::cost::{self, CostReport};
use crate::graph::Index;
use crate::model::{validate, Architecture, Issue, ValidationLimits};
use crate::ops::Op;
use crate::sim::{simulate, standard_scenarios, RecoveryKind, Scenario, ServiceState, SimResult, UnitLoad};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

pub const ENGINE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Rule does not apply to this architecture.
    Na,
    Ok,
    NoData,
    Risk,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Subjects {
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub edges: Vec<String>,
    #[serde(default)]
    pub zones: Vec<String>,
}

/// A value the user can supply to turn a `no_data` finding into a verified one: the UI offers to open the
/// exact field. `target` is `requirements`, `node`, `edge` or `zone`; `prop` is the property key (for
/// requirements: the field name, e.g. `rtoSeconds`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Need {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub prop: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fix {
    pub id: String,
    /// Key into the rule's `fixLabels`.
    pub kind: String,
    #[serde(default)]
    pub vars: Map<String, Value>,
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    pub reason: String,
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Stable fingerprint (`ruleId:key`) – waivers are matched on it.
    pub id: String,
    pub rule_id: String,
    pub category: String,
    pub status: Status,
    pub subjects: Subjects,
    /// Placeholder values for the rule's `message` template.
    pub vars: Map<String, Value>,
    pub fixes: Vec<Fix>,
    /// What to fill in to verify a `no_data` finding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<Need>,
    /// Scenario that demonstrates the finding (the UI can replay it on the canvas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<Scenario>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<Accepted>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleResult {
    pub rule_id: String,
    pub category: String,
    pub status: Status,
    pub findings: usize,
    pub accepted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub critical: usize,
    pub risk: usize,
    pub no_data: usize,
    pub ok: usize,
    pub na: usize,
    /// Findings the user accepted explicitly (not counted above).
    pub accepted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Objective {
    /// `rto`, `rpo`, `site_loss`, `spof`, `capacity`, `budget`.
    pub id: String,
    pub status: Status,
    #[serde(default)]
    pub required: Option<Value>,
    #[serde(default)]
    pub actual: Option<Value>,
    /// `declared` or `assumed` when the actual value rests on an assumption.
    #[serde(default)]
    pub assumed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioSummary {
    pub scenario: Scenario,
    pub failed: Vec<String>,
    pub state: ServiceState,
    pub recovery_kind: RecoveryKind,
    pub recovery_seconds: Option<f64>,
    pub rpo_seconds: Option<f64>,
    pub rpo_assumed: bool,
    pub capacity_ratio: Option<f64>,
    pub split_brain: bool,
    pub data_lost: bool,
    pub down_units: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreItem {
    pub rule_id: String,
    pub status: Status,
    pub penalty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub value: f64,
    pub penalty: f64,
    /// Share of applicable rules that could be verified with data (0..1).
    pub coverage: f64,
    pub breakdown: Vec<ScoreItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub engine_version: u32,
    pub issues: Vec<Issue>,
    pub counts: Counts,
    pub rules: Vec<RuleResult>,
    pub findings: Vec<Finding>,
    pub objectives: Vec<Objective>,
    pub scenarios: Vec<ScenarioSummary>,
    pub baseline_loads: Vec<UnitLoad>,
    /// Per-service capacity plan (empty without a declared peak load).
    pub capacity: Vec<CapacityRow>,
    pub score: Score,
    pub cost: CostReport,
    /// Units with their display names (group key → name) for the UI.
    pub units: BTreeMap<String, String>,
}

/// Capacity plan of one logical service: what it carries, what it can carry, what the failure scenarios
/// leave of it and how many instances the configured headroom needs. Load scales linearly with the peak
/// RPS, so the UI can answer "what if the load grows" without another evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapacityRow {
    pub unit: String,
    pub name: String,
    pub kind: String,
    pub nodes: Vec<String>,
    /// Instances that serve load (arbiters excluded).
    pub instances: u32,
    pub per_replica_rps: Option<f64>,
    /// Load the unit carries at the declared peak.
    pub load_rps: f64,
    /// What the unit can carry now; `None` = no capacity model (set vCPU or RPS per instance).
    pub capacity_rps: Option<f64>,
    /// Capacity needed to keep the configured headroom at peak plus growth.
    pub target_rps: f64,
    pub instances_needed: Option<u32>,
    /// The failure scenario that leaves the unit the least capacity relative to its load.
    pub worst: Option<CapacityWorst>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapacityWorst {
    pub scenario: Scenario,
    pub failed: Vec<String>,
    /// Load the unit has to carry in that scenario.
    pub load_rps: f64,
    pub capacity_rps: f64,
}

/// Evaluation context shared by all checks (memoises simulations).
pub struct Ctx<'a> {
    pub ix: Index<'a>,
    pub cfg: &'a EngineConfig,
    cache: RefCell<HashMap<String, Rc<SimResult>>>,
}

impl<'a> Ctx<'a> {
    pub fn new(arch: &'a Architecture, cfg: &'a EngineConfig) -> Self {
        Ctx { ix: Index::build(arch, &cfg.catalog, &cfg.settings), cfg, cache: RefCell::new(HashMap::new()) }
    }
    pub fn sim(&self, sc: &Scenario) -> Rc<SimResult> {
        let key = sc.key();
        if let Some(r) = self.cache.borrow().get(&key) {
            return r.clone();
        }
        let r = Rc::new(simulate(&self.ix, sc));
        self.cache.borrow_mut().insert(key, r.clone());
        r
    }
}

/// Evaluate an architecture. Pure and deterministic: identical input → identical output.
pub fn evaluate(arch: &Architecture, cfg: &EngineConfig, waivers: &[Waiver]) -> Report {
    let ctx = Ctx::new(arch, cfg);
    let issues = validate(arch, &cfg.catalog, &ValidationLimits::default());

    let mut findings: Vec<Finding> = Vec::new();
    let mut rules: Vec<RuleResult> = Vec::new();
    let waiver_map: HashMap<&str, &Waiver> = waivers.iter().map(|w| (w.fingerprint.as_str(), w)).collect();

    for rule in cfg.rules.iter().filter(|r| r.enabled) {
        let out = checks::run(&ctx, rule);
        if !out.applicable {
            rules.push(RuleResult { rule_id: rule.id.clone(), category: rule.category.clone(), status: Status::Na, findings: 0, accepted: 0 });
            continue;
        }
        let mut worst = Status::Ok;
        let mut n = 0;
        let mut acc = 0;
        for raw in out.findings {
            let id = format!("{}:{}", rule.id, raw.key);
            let accepted = waiver_map.get(id.as_str()).map(|w| Accepted { reason: w.reason.clone(), by: w.by.clone(), at: w.at.clone() });
            // NoData findings cannot be "accepted away": there is nothing to accept.
            let accepted = if raw.status == Status::NoData { None } else { accepted };
            if accepted.is_some() {
                acc += 1;
            } else {
                worst = worst.max(raw.status);
            }
            n += 1;
            findings.push(Finding {
                id,
                rule_id: rule.id.clone(),
                category: rule.category.clone(),
                status: raw.status,
                subjects: raw.subjects,
                vars: raw.vars,
                fixes: raw.fixes,
                needs: raw.needs,
                scenario: raw.scenario,
                accepted,
            });
        }
        rules.push(RuleResult { rule_id: rule.id.clone(), category: rule.category.clone(), status: worst, findings: n, accepted: acc });
    }

    // stable order: severity desc, then rule id, then fingerprint
    findings.sort_by(|a, b| b.accepted.is_none().cmp(&a.accepted.is_none()).then(b.status.cmp(&a.status)).then(a.rule_id.cmp(&b.rule_id)).then(a.id.cmp(&b.id)));

    let mut counts = Counts::default();
    for r in &rules {
        match r.status {
            Status::Critical => counts.critical += 1,
            Status::Risk => counts.risk += 1,
            Status::NoData => counts.no_data += 1,
            Status::Ok => counts.ok += 1,
            Status::Na => counts.na += 1,
        }
    }
    counts.accepted = findings.iter().filter(|f| f.accepted.is_some()).count();

    let score = compute_score(cfg, &rules);
    let scenarios = scenario_summaries(&ctx);
    let baseline = ctx.sim(&Scenario::None);
    let objectives = objectives(&ctx, &findings);
    let units = ctx.ix.units.iter().map(|u| (u.key.clone(), u.name.clone())).collect();
    let cost = cost::estimate(&ctx.ix);
    let capacity = capacity_rows(&ctx);

    Report { engine_version: ENGINE_VERSION, issues, counts, rules, findings, objectives, scenarios, baseline_loads: baseline.unit_loads.clone(), capacity, score, cost, units }
}

fn capacity_rows(ctx: &Ctx<'_>) -> Vec<CapacityRow> {
    let ix = &ctx.ix;
    let baseline = ctx.sim(&Scenario::None);
    if baseline.unit_loads.is_empty() {
        return vec![];
    }
    let req = &ix.arch.requirements;
    let growth = req.growth_percent.unwrap_or(0.0) / 100.0;
    let headroom = ctx.cfg.settings.assumptions.headroom_percent / 100.0;
    let model_traits = checks::capacity_model_traits(ctx.cfg.rules.iter().find(|r| r.check == CheckType::Capacity && r.enabled));
    let scenarios: Vec<_> = standard_scenarios(ix).into_iter().filter(|sc| !matches!(sc, Scenario::Instance { .. } | Scenario::Node { .. } | Scenario::None)).collect();
    let results: Vec<_> = scenarios.iter().map(|sc| ctx.sim(sc)).collect();
    let mut rows = vec![];
    for l in &baseline.unit_loads {
        let Some(&u) = ix.unit_ix.get(&l.unit) else { continue };
        let unit = &ix.units[u];
        // a source of load without a capacity model that needs none (external users) is not part of the plan
        if l.capacity_rps.is_none() && !model_traits.iter().any(|t| unit.traits.contains(t)) {
            continue;
        }
        let serving: Vec<usize> = unit.members.iter().copied().filter(|&m| !ix.is_witness(m)).collect();
        let instances: u32 = serving.iter().map(|&m| ix.replicas(m)).sum();
        let per = serving.iter().find_map(|&m| ix.per_replica_rps(m));
        let avg = match l.capacity_rps {
            Some(c) if instances > 0 => Some(c / instances as f64),
            _ => per,
        };
        let target = l.load_rps * (1.0 + growth) / (1.0 - headroom).max(0.05);
        let needed = avg.filter(|a| *a > 0.0).map(|a| (target / a).ceil().max(1.0) as u32);
        let mut worst: Option<(f64, CapacityWorst)> = None;
        if l.capacity_rps.is_some() {
            for (sc, r) in scenarios.iter().zip(&results) {
                if r.state == ServiceState::Down {
                    continue;
                }
                let Some(ul) = r.unit_loads.iter().find(|x| x.unit == l.unit) else { continue };
                let Some(cap) = ul.capacity_rps else { continue };
                let ratio = if ul.load_rps > 0.0 { cap / ul.load_rps } else { f64::MAX };
                if worst.as_ref().map(|(w, _)| ratio < *w).unwrap_or(true) {
                    worst = Some((ratio, CapacityWorst { scenario: sc.clone(), failed: r.failed.clone(), load_rps: ul.load_rps, capacity_rps: cap }));
                }
            }
        }
        rows.push(CapacityRow {
            unit: unit.key.clone(),
            name: unit.name.clone(),
            kind: unit.kind.clone(),
            nodes: unit.members.iter().map(|&m| ix.arch.nodes[m].id.clone()).collect(),
            instances,
            per_replica_rps: avg,
            load_rps: l.load_rps,
            capacity_rps: l.capacity_rps,
            target_rps: target,
            instances_needed: needed,
            worst: worst.map(|(_, w)| w),
        });
    }
    rows
}

fn compute_score(cfg: &EngineConfig, rules: &[RuleResult]) -> Score {
    let sc = &cfg.settings.scoring;
    let mut breakdown = Vec::new();
    let mut penalty = 0.0;
    let mut nd_total = 0.0;
    for r in rules {
        let rule = cfg.rules.iter().find(|x| x.id == r.rule_id);
        let base = match r.status {
            Status::Critical => sc.critical,
            Status::Risk => sc.risk,
            Status::NoData => sc.no_data,
            _ => 0.0,
        };
        if base == 0.0 {
            continue;
        }
        let p = rule.and_then(|x| x.weight).unwrap_or(base);
        if r.status == Status::NoData {
            nd_total += p;
            continue;
        }
        penalty += p;
        breakdown.push(ScoreItem { rule_id: r.rule_id.clone(), status: r.status, penalty: p });
    }
    for r in rules.iter().filter(|r| r.status == Status::NoData) {
        breakdown.push(ScoreItem { rule_id: r.rule_id.clone(), status: r.status, penalty: sc.no_data });
    }
    penalty += nd_total.min(sc.no_data_cap);
    let applicable = rules.iter().filter(|r| r.status != Status::Na).count();
    let verified = rules.iter().filter(|r| !matches!(r.status, Status::Na | Status::NoData)).count();
    let coverage = if applicable == 0 { 1.0 } else { verified as f64 / applicable as f64 };
    Score { value: (100.0 - penalty).clamp(0.0, 100.0).round(), penalty, coverage: (coverage * 1000.0).round() / 1000.0, breakdown }
}

fn scenario_summaries(ctx: &Ctx<'_>) -> Vec<ScenarioSummary> {
    standard_scenarios(&ctx.ix)
        .into_iter()
        .filter(|sc| !matches!(sc, Scenario::Instance { .. } | Scenario::Node { .. }))
        .map(|sc| {
            let r = ctx.sim(&sc);
            summarize(&r)
        })
        .collect()
}

pub fn summarize(r: &SimResult) -> ScenarioSummary {
    ScenarioSummary {
        scenario: r.scenario.clone(),
        failed: r.failed.clone(),
        state: r.state,
        recovery_kind: r.recovery.kind,
        recovery_seconds: r.recovery.seconds,
        rpo_seconds: r.rpo_seconds,
        rpo_assumed: r.rpo_assumed,
        capacity_ratio: r.capacity_ratio,
        split_brain: !r.split_brain.is_empty(),
        data_lost: !r.data_lost.is_empty(),
        down_units: r.down_units.clone(),
    }
}

/// Objective tiles (RTO / RPO / site loss / SPOF / capacity / budget) derived from the same
/// scenarios the rules use – the tiles can never disagree with the findings.
fn objectives(ctx: &Ctx<'_>, findings: &[Finding]) -> Vec<Objective> {
    let req = &ctx.ix.arch.requirements;
    let site_sc = checks::site_scenarios(ctx);
    let mut out = Vec::new();

    // RTO
    let mut worst_rto: Option<f64> = None;
    let mut never = false;
    for sc in &site_sc {
        let r = ctx.sim(sc);
        match (r.recovery.kind, r.recovery.seconds) {
            (RecoveryKind::Never, _) => never = true,
            (_, Some(s)) => worst_rto = Some(worst_rto.map_or(s, |w: f64| w.max(s))),
            _ => {}
        }
    }
    let rto_status = match (req.rto_seconds, worst_rto, never) {
        (None, _, _) => Status::NoData,
        (Some(_), _, true) => Status::Critical,
        (Some(req), Some(w), _) => {
            if w > req {
                Status::Critical
            } else {
                Status::Ok
            }
        }
        (Some(_), None, _) => {
            if site_sc.is_empty() {
                Status::NoData
            } else {
                Status::Ok
            }
        }
    };
    out.push(Objective {
        id: "rto".into(),
        status: rto_status,
        required: req.rto_seconds.map(|v| json!(v)),
        actual: if never { Some(json!("never")) } else { worst_rto.map(|v| json!(v)) },
        assumed: false,
    });

    // RPO (units excluded by the RPO rule, e.g. queues covered by their own rule, are ignored)
    let excl: Vec<String> = ctx
        .cfg
        .rules
        .iter()
        .find(|r| r.check == CheckType::Rpo && r.enabled)
        .and_then(|r| r.params.get("excludeTraits"))
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut worst_rpo: Option<f64> = Some(0.0);
    let mut rpo_assumed = false;
    let mut data_lost = false;
    for sc in &site_sc {
        let r = ctx.sim(sc);
        data_lost |= !checks::lost_units(ctx, &r.data_lost, &excl).is_empty();
        let (v, a) = crate::sim::rpo_filtered(&r.rpo_items, &excl);
        match (v, worst_rpo) {
            (None, _) => worst_rpo = None,
            (Some(v), Some(w)) => worst_rpo = Some(w.max(v)),
            _ => {}
        }
        rpo_assumed |= a;
    }
    let rpo_status = match (req.rpo_seconds, worst_rpo) {
        _ if data_lost => Status::Critical,
        (None, _) => Status::NoData,
        (Some(_), None) => Status::NoData,
        (Some(r), Some(w)) => {
            if w > r {
                Status::Critical
            } else if rpo_assumed && w > 0.0 {
                Status::NoData
            } else {
                Status::Ok
            }
        }
    };
    out.push(Objective {
        id: "rpo".into(),
        status: rpo_status,
        required: req.rpo_seconds.map(|v| json!(v)),
        actual: if data_lost { Some(json!("lost")) } else { worst_rpo.map(|v| json!(v)) },
        assumed: rpo_assumed,
    });

    // Site loss
    let mut survived = true;
    for sc in &site_sc {
        let r = ctx.sim(sc);
        if r.state == ServiceState::Down && r.recovery.kind == RecoveryKind::Never {
            survived = false;
        }
    }
    out.push(Objective {
        id: "site_loss".into(),
        status: if site_sc.is_empty() {
            Status::NoData
        } else if survived {
            Status::Ok
        } else {
            Status::Critical
        },
        required: Some(json!(checks::tolerance(ctx))),
        actual: Some(json!(survived)),
        assumed: false,
    });

    // SPOF
    let spof = findings.iter().filter(|f| f.rule_id_check_is(ctx, CheckType::Spof) && f.accepted.is_none()).count();
    out.push(Objective { id: "spof".into(), status: if spof == 0 { Status::Ok } else { Status::Critical }, required: Some(json!(0)), actual: Some(json!(spof)), assumed: false });

    // Capacity under failure
    let worst_cap = site_sc.iter().filter_map(|sc| ctx.sim(sc).capacity_ratio).fold(None, |a: Option<f64>, v| Some(a.map_or(v, |x| x.min(v))));
    let min_cap = ctx.cfg.settings.assumptions.min_capacity_during_failure;
    out.push(Objective {
        id: "capacity".into(),
        status: match (req.rps_peak, worst_cap) {
            (None, _) | (_, None) => Status::NoData,
            (_, Some(w)) => {
                if w >= min_cap {
                    Status::Ok
                } else {
                    Status::Risk
                }
            }
        },
        required: Some(json!(min_cap)),
        actual: worst_cap.map(|v| json!((v * 1000.0).round() / 1000.0)),
        assumed: false,
    });
    out
}

impl Finding {
    fn rule_id_check_is(&self, ctx: &Ctx<'_>, t: CheckType) -> bool {
        ctx.cfg.rules.iter().any(|r| r.id == self.rule_id && r.check == t)
    }
}

/// Convenience used by tests and the server: severity → status.
pub fn status_of(sev: Severity) -> Status {
    match sev {
        Severity::Critical => Status::Critical,
        Severity::Risk => Status::Risk,
    }
}

/// Render the human text of a finding in `lang` using the rule's templates.
pub fn render_message(rule: &crate::config::Rule, f: &Finding, lang: &str) -> String {
    use crate::text::LText;
    let vars: BTreeMap<String, Value> = f.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let tpl: &LText = if f.id.ends_with(":lost") && f.status != Status::NoData {
        rule.data_lost_message.as_ref().unwrap_or(&rule.message)
    } else if f.status == Status::NoData {
        if f.id.ends_with(":req") {
            rule.requirement_message.as_ref().or(rule.no_data_message.as_ref()).unwrap_or(&rule.message)
        } else {
            rule.no_data_message.as_ref().unwrap_or(&rule.message)
        }
    } else {
        &rule.message
    };
    tpl.render(lang, &vars)
}
