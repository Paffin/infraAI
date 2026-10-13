//! The gate of a pipeline: a design is judged against thresholds (which open findings fail it, how low the score may go, what is new
//! against an earlier run) and the verdict is written for a person (text, Markdown, workflow annotations) or for a tool (JSON, SARIF).
//!
//! Pure like the rest of the engine: files, clocks, colours and exit codes belong to the program that calls it (`infra-architect check`).

use crate::config::{EngineConfig, Rule, Waiver};
use crate::evaluate::{Finding, Fix, Report, Status};
use crate::model::Architecture;
use crate::recommend::{Effect, Plan, Recommendation};
use crate::text::interpolate_l;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub mod github;
pub mod md;
pub mod plain;
pub mod sarif;
mod words;

pub use words::say;

/// Which open findings fail the check: the mildest status that does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailOn {
    Critical,
    Risk,
    NoData,
    Never,
}

impl FailOn {
    pub fn parse(s: &str) -> Option<FailOn> {
        match s.trim().to_ascii_lowercase().as_str() {
            "critical" => Some(FailOn::Critical),
            "risk" => Some(FailOn::Risk),
            "no-data" | "no_data" | "nodata" => Some(FailOn::NoData),
            "never" | "none" => Some(FailOn::Never),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FailOn::Critical => "critical",
            FailOn::Risk => "risk",
            FailOn::NoData => "no-data",
            FailOn::Never => "never",
        }
    }

    /// The mildest status that fails the check (`None`: findings never fail it).
    pub fn threshold(self) -> Option<Status> {
        match self {
            FailOn::Critical => Some(Status::Critical),
            FailOn::Risk => Some(Status::Risk),
            FailOn::NoData => Some(Status::NoData),
            FailOn::Never => None,
        }
    }
}

/// What an earlier run found: the open findings, and how severe each one was. A finding the baseline already had (as bad or worse) is
/// not new, so a team can adopt the gate on a design with a history and fail only on what a change adds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Baseline {
    pub open: BTreeMap<String, Status>,
}

/// Open (not accepted) critical, risk and no-data findings, the worst first.
pub fn open_findings(report: &Report) -> Vec<&Finding> {
    let mut v: Vec<&Finding> = report.findings.iter().filter(|f| f.accepted.is_none() && matches!(f.status, Status::Critical | Status::Risk | Status::NoData)).collect();
    v.sort_by(|a, b| b.status.cmp(&a.status).then_with(|| a.rule_id.cmp(&b.rule_id)).then_with(|| a.id.cmp(&b.id)));
    v
}

impl Baseline {
    pub fn from_report(r: &Report) -> Baseline {
        Baseline { open: open_findings(r).into_iter().map(|f| (f.id.clone(), f.status)).collect() }
    }

    /// Read a baseline out of a file written by an earlier run: the JSON document of `check --format json` (the entry for `file`, or the only
    /// one) or a bare report of the engine (`POST /evaluate`). A design the baseline has no entry for is new as a whole: it has no history,
    /// so none of its findings is known.
    pub fn from_json(v: &Value, file: &str) -> Result<Baseline, String> {
        let report = if v.get("findings").is_some() {
            v
        } else if let Some(results) = v.get("results").and_then(Value::as_array) {
            let hit = results.iter().find(|r| r.get("file").and_then(Value::as_str) == Some(file)).or(if results.len() == 1 { results.first() } else { None });
            match hit.and_then(|r| r.get("report")) {
                Some(r) => r,
                None => return Ok(Baseline::default()),
            }
        } else {
            return Err("it is neither a report of the engine nor the JSON output of `check`".into());
        };
        let findings = report.get("findings").and_then(Value::as_array).ok_or("the report has no findings")?;
        let mut open = BTreeMap::new();
        for f in findings {
            let accepted = f.get("accepted").map(|a| !a.is_null()).unwrap_or(false);
            let status = f.get("status").and_then(|s| serde_json::from_value::<Status>(s.clone()).ok());
            let id = f.get("id").and_then(Value::as_str);
            if let (false, Some(status), Some(id)) = (accepted, status, id) {
                if matches!(status, Status::Critical | Status::Risk | Status::NoData) {
                    open.insert(id.to_string(), status);
                }
            }
        }
        Ok(Baseline { open })
    }

    fn knows(&self, f: &Finding) -> bool {
        self.open.get(&f.id).is_some_and(|was| *was >= f.status)
    }
}

/// The thresholds of one check.
#[derive(Debug, Clone)]
pub struct Gate {
    pub fail_on: FailOn,
    /// Fail when the score is lower.
    pub min_score: Option<f64>,
    pub baseline: Option<Baseline>,
}

impl Default for Gate {
    fn default() -> Self {
        Gate { fail_on: FailOn::Critical, min_score: None, baseline: None }
    }
}

/// Why a design did not pass.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reason {
    /// Open findings at or above the threshold (only the new ones when there is a baseline).
    #[serde(rename_all = "camelCase")]
    Findings { fail_on: FailOn, count: usize, ids: Vec<String>, against_baseline: bool },
    /// The score is under the minimum.
    Score { value: f64, min: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub passed: bool,
    pub fail_on: FailOn,
    pub min_score: Option<f64>,
    pub reasons: Vec<Reason>,
    /// Fingerprints of the findings that fail the check.
    pub blocking: Vec<String>,
    /// Findings at or above the threshold that the baseline already had: they do not fail it.
    pub known: usize,
}

/// Judge a report.
pub fn judge(report: &Report, gate: &Gate) -> Verdict {
    let mut reasons = vec![];
    let mut blocking = vec![];
    let mut known = 0;
    if let Some(threshold) = gate.fail_on.threshold() {
        for f in open_findings(report).into_iter().filter(|f| f.status >= threshold) {
            if gate.baseline.as_ref().is_some_and(|b| b.knows(f)) {
                known += 1;
            } else {
                blocking.push(f.id.clone());
            }
        }
        if !blocking.is_empty() {
            reasons.push(Reason::Findings { fail_on: gate.fail_on, count: blocking.len(), ids: blocking.clone(), against_baseline: gate.baseline.is_some() });
        }
    }
    if let Some(min) = gate.min_score {
        if report.score.value < min {
            reasons.push(Reason::Score { value: report.score.value, min });
        }
    }
    Verdict { passed: reasons.is_empty(), fail_on: gate.fail_on, min_score: gate.min_score, reasons, blocking, known }
}

/// A risk accepted in a file kept next to the design (what the editor's "accept risk" does in the database): the finding stays visible and
/// stops failing the check. `expires` (`YYYY-MM-DD`) ends the acceptance, so a risk cannot be accepted for good by forgetting about it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedRisk {
    pub fingerprint: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub expires: Option<String>,
}

/// The acceptances in force on `today` (`YYYY-MM-DD`; none expires when it says nothing) and the fingerprints whose acceptance has expired.
/// A date that is not a date is an error: an acceptance must not silently last forever because of a typo.
pub fn waivers_in_force(list: &[AcceptedRisk], today: &str) -> Result<(Vec<Waiver>, Vec<String>), String> {
    let now = crate::lifecycle::parse_date(today);
    let (mut live, mut expired) = (vec![], vec![]);
    for a in list {
        let ends = match a.expires.as_deref() {
            None => None,
            Some(e) => Some(crate::lifecycle::parse_date(e).ok_or_else(|| format!("the acceptance of '{}' expires on '{e}', which is not a date (YYYY-MM-DD)", a.fingerprint))?),
        };
        if matches!((ends, now), (Some(end), Some(now)) if end < now) {
            expired.push(a.fingerprint.clone());
        } else {
            live.push(Waiver { fingerprint: a.fingerprint.clone(), reason: a.reason.clone(), by: a.by.clone(), at: a.at.clone() });
        }
    }
    Ok((live, expired))
}

/// One checked design, with everything a report about it needs.
pub struct Run<'a> {
    /// The path the design was read from, as the user gave it.
    pub file: &'a str,
    pub arch: &'a Architecture,
    pub cfg: &'a EngineConfig,
    pub report: &'a Report,
    pub plan: Option<&'a Plan>,
    pub verdict: &'a Verdict,
    pub lang: &'a str,
    /// Fingerprints whose acceptance has expired (they are open again).
    pub expired: &'a [String],
    /// The line of an element (node, edge or zone) in the file the design was read from.
    pub line: &'a dyn Fn(&str) -> Option<u32>,
}

impl Run<'_> {
    /// The name of the design: its own, or the file's.
    pub fn name(&self) -> String {
        match self.arch.meta.name.as_deref().map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => std::path::Path::new(self.file).file_stem().and_then(|s| s.to_str()).unwrap_or(self.file).to_string(),
        }
    }

    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.cfg.rules.iter().find(|r| r.id == id)
    }

    pub fn title(&self, rule_id: &str) -> String {
        self.rule(rule_id).map(|r| r.title.get(self.lang)).filter(|t| !t.is_empty()).unwrap_or_else(|| rule_id.to_string())
    }

    /// What is wrong, in the words of the rule.
    pub fn message(&self, f: &Finding) -> String {
        match self.rule(&f.rule_id) {
            Some(r) => crate::evaluate::render_message(r, f, self.lang),
            None => f.rule_id.clone(),
        }
    }

    /// The label of a fix: the rule's own wording of that kind of fix, with the variables of the finding and the fix.
    pub fn fix_label(&self, f: &Finding, fix: &Fix) -> String {
        let mut vars = f.vars.clone();
        vars.extend(fix.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.label_of(&f.rule_id, &fix.kind, &vars)
    }

    /// The label of a recommendation.
    pub fn recommendation_label(&self, r: &Recommendation) -> String {
        self.label_of(&r.rule_id, &r.kind, &r.vars)
    }

    fn label_of(&self, rule_id: &str, kind: &str, vars: &Map<String, Value>) -> String {
        let vars: BTreeMap<String, Value> = vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        match self.rule(rule_id).and_then(|r| r.fix_labels.get(kind)) {
            Some(tpl) => tpl.render(self.lang, &vars),
            None => kind.to_string(),
        }
    }

    /// `id «name»` of every element the finding is about (nodes, then edges, then zones), with where each lies in the file.
    pub fn subjects(&self, f: &Finding) -> Vec<Subject> {
        let mut out = vec![];
        for id in &f.subjects.nodes {
            let name = self.arch.nodes.iter().find(|n| &n.id == id).map(|n| n.name.clone());
            out.push(Subject { kind: "node", id: id.clone(), name, line: (self.line)(id) });
        }
        for id in &f.subjects.edges {
            let name = self.arch.edges.iter().find(|e| &e.id == id).map(|e| format!("{} → {}", self.node_name(&e.source), self.node_name(&e.target)));
            out.push(Subject { kind: "edge", id: id.clone(), name, line: (self.line)(id) });
        }
        for id in &f.subjects.zones {
            let name = self.arch.zones.iter().find(|z| &z.id == id).map(|z| z.name.clone());
            out.push(Subject { kind: "zone", id: id.clone(), name, line: (self.line)(id) });
        }
        out
    }

    fn node_name(&self, id: &str) -> String {
        self.arch.nodes.iter().find(|n| n.id == id).map(|n| n.name.clone()).unwrap_or_else(|| id.to_string())
    }

    /// What to fill in to turn a "no data" finding into an answer: `rtoSeconds`, `lagSeconds (edge e1)`.
    pub fn needs(&self, f: &Finding) -> Vec<String> {
        f.needs
            .iter()
            .map(|n| match (n.target.as_str(), n.id.as_deref()) {
                ("requirements", _) => format!("requirements.{}", n.prop),
                (target, Some(id)) => format!("{target} {id}: {}", n.prop),
                (_, None) => n.prop.clone(),
            })
            .collect()
    }

    /// `say` for this run's language.
    pub fn say(&self, key: &str, vars: &[(&str, Value)]) -> String {
        say(self.lang, key, vars)
    }

    /// The figures of an effect that moved, as the editor's chips write them: findings, score, availability with its nines, money a month.
    /// A figure that did not change is not listed; a finding the change brings in is.
    pub fn effect_chips(&self, e: &Effect) -> Vec<String> {
        use serde_json::json;
        let mut out = vec![];
        if e.critical.before != e.critical.after {
            out.push(self.say("chip.critical", &[("from", json!(e.critical.before)), ("to", json!(e.critical.after))]));
        }
        if e.risk.before != e.risk.after {
            out.push(self.say("chip.risk", &[("from", json!(e.risk.before)), ("to", json!(e.risk.after))]));
        }
        let (s0, s1) = (e.score.before.round(), e.score.after.round());
        if s0 != s1 {
            out.push(self.say("chip.score", &[("from", json!(s0)), ("to", json!(s1))]));
        }
        if let Some(a) = &e.availability {
            let (a0, a1) = (crate::text::format_availability(a.before), crate::text::format_availability(a.after));
            if a0 != a1 {
                out.push(self.say("chip.availability", &[("from", json!(a0)), ("to", json!(a1))]));
            }
        }
        let cost = e.cost.after - e.cost.before;
        if cost.abs() >= 0.5 {
            let money = format!("{}{} {}", if cost < 0.0 { '−' } else { '+' }, group_thousands(cost.abs(), self.lang), self.report.cost.symbol);
            out.push(self.say("chip.cost", &[("v", json!(money)), ("unit", json!(self.say("per_month", &[])))]));
        }
        if !e.introduced.is_empty() {
            out.push(self.say("chip.introduced", &[("n", json!(e.introduced.len()))]));
        }
        out
    }

    /// A reason the check failed, in words.
    pub fn reason_text(&self, r: &Reason) -> String {
        use serde_json::json;
        match r {
            Reason::Findings { fail_on, count, against_baseline, .. } => {
                let level = self.say(
                    match fail_on {
                        FailOn::Risk => "level.risk",
                        FailOn::NoData => "level.no_data",
                        _ => "level.critical",
                    },
                    &[],
                );
                let key = if *against_baseline { "reason.findings.new" } else { "reason.findings" };
                self.say(key, &[("level", json!(level)), ("n", json!(count)), ("fail_on", json!(fail_on.name()))])
            }
            Reason::Score { value, min } => self.say("reason.score", &[("value", json!(value.round())), ("min", json!(min))]),
        }
    }

    /// The word for a status.
    pub fn status_word(&self, s: Status) -> String {
        self.say(
            match s {
                Status::Critical => "status.critical",
                Status::Risk => "status.risk",
                _ => "status.no_data",
            },
            &[],
        )
    }

    /// The headline of the numbers: score, confidence, findings by level, availability against its target, money a month.
    pub fn figures(&self) -> Vec<String> {
        let r = self.report;
        // the findings that are listed below (the report's own counts are per rule)
        let open = open_findings(r);
        let count = |s: Status| open.iter().filter(|f| f.status == s).count();
        let mut out = vec![
            format!("{} {}/100", self.say("score", &[]), r.score.value.round()),
            format!("{} {}%", self.say("confidence", &[]), (r.score.coverage * 100.0).round()),
            format!("{} {}", self.say("critical", &[]), count(Status::Critical)),
            format!("{} {}", self.say("risk", &[]), count(Status::Risk)),
            format!("{} {}", self.say("no_data", &[]), count(Status::NoData)),
        ];
        if r.counts.accepted > 0 {
            out.push(format!("{} {}", self.say("accepted", &[]), r.counts.accepted));
        }
        if let Some(a) = &r.availability {
            let mut s = format!("{} ≈ {}", self.say("availability", &[]), crate::text::format_availability(a.estimate));
            if let Some(t) = a.target {
                s.push_str(&format!(" ({} {})", self.say("target", &[]), crate::text::format_availability(t)));
            }
            out.push(s);
        }
        if r.cost.total > 0.0 {
            out.push(format!("{} {} {} {}", self.say("cost", &[]), group_thousands(r.cost.total, self.lang), r.cost.symbol, self.say("per_month", &[])));
        }
        out
    }
}

/// One element a finding is about.
#[derive(Debug, Clone)]
pub struct Subject {
    pub kind: &'static str,
    pub id: String,
    pub name: Option<String>,
    pub line: Option<u32>,
}

impl Subject {
    /// `gslb «GSLB + CDN»`.
    pub fn label(&self) -> String {
        match &self.name {
            Some(n) if n != &self.id => format!("{} «{}»", self.id, n),
            _ => self.id.clone(),
        }
    }
}

/// Numbers with thousands separators, the way the language writes them (`727 900`, `727,900`).
pub fn group_thousands(n: f64, lang: &str) -> String {
    let digits = format!("{}", n.abs().round() as u64);
    let sep = if lang == "ru" { '\u{a0}' } else { ',' };
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    if n < 0.0 {
        out.insert(0, '−');
    }
    out
}

/// A text with `{placeholders}` and plural forms, in `lang`.
pub(crate) fn fill(tpl: &str, vars: &[(&str, Value)], lang: &str) -> String {
    let map: BTreeMap<String, Value> = vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    interpolate_l(tpl, &map, lang)
}
