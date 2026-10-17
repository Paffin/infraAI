//! Helpers shared by API handlers.

use crate::config_store::Loaded;
use crate::error::{ApiError, ApiResult};
use engine::config::Waiver;
use engine::evaluate::{evaluate, Report, Status};
use engine::model::{validate, Architecture, IssueLevel, ValidationLimits};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn limits(cfg: &Loaded) -> ValidationLimits {
    ValidationLimits { max_nodes: cfg.settings.num("limits.maxNodes") as usize, max_edges: cfg.settings.num("limits.maxEdges") as usize, max_zones: cfg.settings.num("limits.maxZones") as usize }
}

pub fn parse_arch(v: Value) -> ApiResult<Architecture> {
    serde_json::from_value(v).map_err(|e| ApiError::bad_request("invalid_architecture", format!("architecture does not match the schema: {e}")))
}

/// Parse + structurally validate (referential integrity, catalog conformity, limits).
pub fn parse_valid_arch(cfg: &Loaded, v: Value) -> ApiResult<Architecture> {
    let mut a = parse_arch(v)?;
    // `proposed` only marks nodes of a pending preview; it is never part of a stored architecture
    for n in a.nodes.iter_mut() {
        n.proposed = false;
    }
    let issues = validate(&a, &cfg.catalog, &limits(cfg));
    let errors: Vec<_> = issues.into_iter().filter(|i| i.level == IssueLevel::Error).collect();
    if !errors.is_empty() {
        return Err(ApiError::bad_request("invalid_architecture", errors.iter().take(5).map(|e| e.message.clone()).collect::<Vec<_>>().join("; ")).with_details(json!({"issues": errors})));
    }
    Ok(a)
}

/// Run the (CPU-bound) evaluation off the async runtime.
pub async fn run_evaluate(cfg: Arc<Loaded>, arch: Architecture, waivers: Vec<Waiver>, lang: String) -> ApiResult<Report> {
    tokio::task::spawn_blocking(move || {
        let ec = cfg.engine_config(&lang);
        evaluate(&arch, &ec, &waivers)
    })
    .await
    .map_err(ApiError::internal)
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

/// The summary kept with a version: the architecture evaluated with the project's accepted risks.
pub async fn evaluated_summary(cfg: &Arc<Loaded>, arch: Architecture, waivers: Vec<Waiver>, lang: String) -> ApiResult<Value> {
    let report = run_evaluate(cfg.clone(), arch, waivers, lang).await?;
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
