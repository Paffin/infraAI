//! Helpers shared by API handlers.

use crate::config_store::Loaded;
use crate::error::{ApiError, ApiResult};
use crate::heavy::Held;
use engine::config::Waiver;
use engine::evaluate::{evaluate, Report, Status};
use engine::model::{validate, Architecture, IssueLevel, ValidationLimits};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// The limits for what is written: saved, created, imported, applied, proposed. A number out of its range (`prop_out_of_range`,
/// `requirement_out_of_range`) is an error there: it is refused before it is stored, instead of failing every later request about the project.
pub fn limits(cfg: &Loaded) -> ValidationLimits {
    ValidationLimits {
        max_nodes: cfg.settings.num("limits.maxNodes") as usize,
        max_edges: cfg.settings.num("limits.maxEdges") as usize,
        max_zones: cfg.settings.num("limits.maxZones") as usize,
        numbers: cfg.engine.limits,
        lenient_ranges: false,
    }
}

/// The limits for a design that is only looked at (evaluated, simulated, recommended, exported, versioned, the base of a set of operations): a
/// number out of its range is a warning in the report. The engine holds an instance count at its limit; any other number it reads as it is, so a
/// figure computed from one may come out without a value (`null`). A project stored before the ranges were enforced still opens, and a value
/// that is still being typed is no reason to refuse the request that asks what it would do.
pub fn lenient_limits(cfg: &Loaded) -> ValidationLimits {
    limits(cfg).lenient()
}

/// Refuse a design that is bigger than the configured size (`400 limits_exceeded`: `limits.maxNodes`, `maxEdges`, `maxZones`), without judging anything else
/// about it. The paths that only look at a design (`/evaluate`, `/diff`) use this; the ones that write also run the whole of [`validate`].
pub fn ensure_size(cfg: &Loaded, a: &Architecture) -> ApiResult<()> {
    let lim = limits(cfg);
    if a.nodes.len() > lim.max_nodes || a.edges.len() > lim.max_edges || a.zones.len() > lim.max_zones {
        return Err(ApiError::bad_request("limits_exceeded", "architecture exceeds the configured size limits").with_details(json!({
            "nodes": a.nodes.len(), "maxNodes": lim.max_nodes, "edges": a.edges.len(), "maxEdges": lim.max_edges, "zones": a.zones.len(), "maxZones": lim.max_zones,
        })));
    }
    Ok(())
}

/// Which of the caps on text a field answers to: three settings, because a name, a long text and an identifier are three different things to bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextField {
    /// A project's or a user's name: `limits.maxNameChars`.
    Name,
    /// A description, a version message, the reason for an accepted risk: `limits.maxTextChars`.
    LongText,
    /// The fingerprint of a finding in a request: `limits.maxIdChars`.
    Id,
}

impl TextField {
    pub fn setting(self) -> &'static str {
        match self {
            TextField::Name => "limits.maxNameChars",
            TextField::LongText => "limits.maxTextChars",
            TextField::Id => "limits.maxIdChars",
        }
    }
}

/// `value` when it is no longer than the cap of its kind (counted in characters, read from the settings on every call: a change takes effect at once), else
/// `400 field_too_long` with `details {field, max, actual}`. A text that came in without a bound is stored, listed, echoed to everybody who opens the page
/// and sent to the model: it is bounded where it comes in.
pub fn text<'a>(cfg: &Loaded, kind: TextField, field: &'static str, value: &'a str) -> ApiResult<&'a str> {
    let max = cfg.settings.num(kind.setting()) as usize;
    let actual = value.chars().count();
    if actual > max {
        return Err(ApiError::bad_request("field_too_long", format!("{field} is longer than {max} characters")).with_details(json!({"field": field, "max": max, "actual": actual})));
    }
    Ok(value)
}

pub fn parse_arch(v: Value) -> ApiResult<Architecture> {
    serde_json::from_value(v).map_err(|e| ApiError::bad_request("invalid_architecture", format!("architecture does not match the schema: {e}")))
}

/// Refuse `a` with `400 invalid_architecture` (the issues in `details.issues`) when it has errors under `lim`.
pub fn ensure_valid(cfg: &Loaded, a: &Architecture, lim: &ValidationLimits) -> ApiResult<()> {
    let issues = validate(a, &cfg.catalog, lim);
    let errors: Vec<_> = issues.into_iter().filter(|i| i.level == IssueLevel::Error).collect();
    if !errors.is_empty() {
        return Err(ApiError::bad_request("invalid_architecture", errors.iter().take(5).map(|e| e.message.clone()).collect::<Vec<_>>().join("; ")).with_details(json!({"issues": errors})));
    }
    Ok(())
}

fn parse_checked(cfg: &Loaded, v: Value, lim: &ValidationLimits) -> ApiResult<Architecture> {
    let mut a = parse_arch(v)?;
    // `proposed` only marks nodes of a pending preview; it is never part of a stored architecture
    for n in a.nodes.iter_mut() {
        n.proposed = false;
    }
    ensure_valid(cfg, &a, lim)?;
    Ok(a)
}

/// Parse + validate what is going to be written (referential integrity, catalog conformity, limits, numbers in their ranges).
pub fn parse_valid_arch(cfg: &Loaded, v: Value) -> ApiResult<Architecture> {
    parse_checked(cfg, v, &limits(cfg))
}

/// Parse + validate a design that is only looked at: the structure must be sound, a number out of range is left to the report as a warning.
pub fn parse_viewed_arch(cfg: &Loaded, v: Value) -> ApiResult<Architecture> {
    parse_checked(cfg, v, &lenient_limits(cfg))
}

/// Run CPU-bound work on the blocking pool, off the async runtime, **with the slot of the request moved in** (`heavy`): the slot is given back when the work
/// ends, not when the request does. A client that aborts drops the future that waits here; the closure on its blocking thread does not stop, and until it
/// has finished the processor is not free, so the slot is not either.
pub async fn blocking<T: Send + 'static>(hold: &Held, work: impl FnOnce() -> T + Send + 'static) -> ApiResult<T> {
    let hold = hold.clone();
    tokio::task::spawn_blocking(move || {
        let _slot = hold;
        work()
    })
    .await
    .map_err(ApiError::internal)
}

/// Run the (CPU-bound) evaluation off the async runtime.
pub async fn run_evaluate(cfg: Arc<Loaded>, arch: Architecture, waivers: Vec<Waiver>, lang: String) -> ApiResult<Report> {
    run_evaluate_held(cfg, arch, waivers, lang, &Held::default()).await
}

/// [`run_evaluate`] for a request that holds a slot of the heavy-request gate: the slot goes with the work.
pub async fn run_evaluate_held(cfg: Arc<Loaded>, arch: Architecture, waivers: Vec<Waiver>, lang: String, hold: &Held) -> ApiResult<Report> {
    blocking(hold, move || {
        let ec = cfg.engine_config(&lang);
        evaluate(&arch, &ec, &waivers)
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub score: f64,
    pub coverage: f64,
    pub counts: engine::evaluate::Counts,
    pub objectives: Vec<(String, Status)>,
    pub cost_total: f64,
    /// Fingerprints of open critical / risk findings (used to compute introduced/resolved).
    pub open: Vec<String>,
}

pub fn summarize(r: &Report) -> ReportSummary {
    ReportSummary {
        score: r.score.value,
        coverage: r.score.coverage,
        counts: r.counts.clone(),
        objectives: r.objectives.iter().map(|o| (o.id.clone(), o.status)).collect(),
        cost_total: r.cost.total,
        open: r.findings.iter().filter(|f| f.accepted.is_none() && matches!(f.status, Status::Critical | Status::Risk)).map(|f| f.id.clone()).collect(),
    }
}

/// The summary kept with a version: the architecture evaluated with the project's accepted risks, by a request that holds a slot of the heavy-request gate.
pub async fn evaluated_summary_held(cfg: &Arc<Loaded>, arch: Architecture, waivers: Vec<Waiver>, lang: String, hold: &Held) -> ApiResult<Value> {
    let report = run_evaluate_held(cfg.clone(), arch, waivers, lang, hold).await?;
    Ok(serde_json::to_value(summarize(&report))?)
}

pub fn lang_of(cfg: &Loaded, requested: Option<&str>) -> String {
    let langs = cfg.settings.list("app.languages");
    match requested {
        Some(l) if langs.iter().any(|x| x == l) => l.to_string(),
        _ => cfg.settings.str("app.defaultLanguage"),
    }
}

pub fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}
