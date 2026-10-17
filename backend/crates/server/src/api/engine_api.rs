//! Stateless engine endpoints: evaluate, simulate, apply operations, templates, export.
//! None of them touch the database except to read a project's accepted risks, so they scale
//! horizontally with the number of replicas.

use crate::api::projects::engine_waivers;
use crate::api_support::{lang_of, lenient_limits, limits, parse_arch, parse_viewed_arch, run_evaluate, summarize, today, ReportSummary};
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::json::{Json, Path, Query};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::Response;
use engine::ops::{apply_ops, diff, Op};
use engine::sim::{check_scenario, simulate_timeline, Scenario, ScenarioFault};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Write;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateBody {
    pub architecture: Value,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub lang: Option<String>,
}

pub async fn evaluate(State(state): State<AppState>, _user: AuthUser, Json(b): Json<EvaluateBody>) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    // structural problems are part of the report (issues), not a request failure
    let arch = parse_arch(b.architecture)?;
    let n = arch.nodes.len();
    let lim = limits(&cfg);
    if n > lim.max_nodes || arch.edges.len() > lim.max_edges || arch.zones.len() > lim.max_zones {
        return Err(ApiError::bad_request("limits_exceeded", "architecture exceeds the configured size limits"));
    }
    let waivers = match b.project_id {
        Some(id) => engine_waivers(&state, id).await?,
        None => vec![],
    };
    state.metrics.evaluations.with_label_values(&["evaluate"]).inc();
    let report = run_evaluate(cfg.clone(), arch, waivers, lang_of(&cfg, b.lang.as_deref())).await?;
    Ok(Json(serde_json::to_value(report)?))
}

/// What to improve first: the engine's fixes for the open findings, each tried on a copy and ranked by the effect measured
/// (`docs/RECOMMENDATIONS.md`). Stateless like `evaluate`; the project's accepted risks are read for the waivers.
pub async fn recommendations(State(state): State<AppState>, _user: AuthUser, Json(b): Json<EvaluateBody>) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    let arch = parse_viewed_arch(&cfg, b.architecture)?;
    let waivers = match b.project_id {
        Some(id) => engine_waivers(&state, id).await?,
        None => vec![],
    };
    let lang = lang_of(&cfg, b.lang.as_deref());
    state.metrics.evaluations.with_label_values(&["recommend"]).inc();
    let lim = lenient_limits(&cfg);
    let plan = tokio::task::spawn_blocking(move || {
        let ec = cfg.engine_config(&lang);
        engine::recommend::recommend(&arch, &ec, &waivers, &lim)
    })
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(serde_json::to_value(plan)?))
}

#[derive(Deserialize)]
pub struct DiffBody {
    pub from: Value,
    pub to: Value,
    /// Count canvas positions as changes (default: no).
    #[serde(default)]
    pub ui: bool,
}

/// Structural difference between two architectures given in the request: the design against an imported
/// snapshot of the real environment (drift), or any two files. Nothing is stored.
pub async fn diff_architectures(_user: AuthUser, Json(b): Json<DiffBody>) -> ApiResult<Json<Value>> {
    let from = parse_arch(b.from)?;
    let to = parse_arch(b.to)?;
    Ok(Json(serde_json::to_value(diff(&from, &to, b.ui))?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateBody {
    pub architecture: Value,
    pub scenario: Scenario,
}

pub async fn simulate_scenario(State(state): State<AppState>, _user: AuthUser, Json(b): Json<SimulateBody>) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    let arch = parse_viewed_arch(&cfg, b.architecture)?;
    state.metrics.evaluations.with_label_values(&["simulate"]).inc();
    let result = tokio::task::spawn_blocking(move || {
        let ec = cfg.engine_config("en");
        let ix = engine::graph::Index::build(&arch, &ec.catalog, &ec.settings);
        // a failure of something the design does not have takes nothing away: the simulator would answer "up". For a scenario that a person (or a
        // link, or a stale tab) brings that is a wrong answer, so it is refused, naming what is not there
        check_scenario(&ix, &b.scenario).map(|()| simulate_timeline(&ix, &b.scenario))
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(scenario_error)?;
    Ok(Json(serde_json::to_value(result)?))
}

fn scenario_error(fault: ScenarioFault) -> ApiError {
    match fault {
        ScenarioFault::Unknown(refs) => {
            let names: Vec<String> = refs.iter().map(|r| format!("{} '{}'", r.kind, r.id)).collect();
            ApiError::bad_request("unknown_scenario_reference", format!("the scenario names what the design does not have: {}", names.join(", "))).with_details(json!({"refs": refs}))
        }
        ScenarioFault::Invalid { reason, message } => ApiError::bad_request("invalid_scenario", message).with_details(json!({"reason": reason})),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyBody {
    pub architecture: Value,
    pub ops: Vec<Op>,
    /// Also evaluate before/after and return the diff.
    #[serde(default)]
    pub preview: bool,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub lang: Option<String>,
}

pub struct Preview {
    pub outcome: engine::ops::ApplyOutcome,
    pub diff: engine::ops::ArchDiff,
    pub before: ReportSummary,
    pub after: ReportSummary,
    pub introduced: Vec<String>,
    pub resolved: Vec<String>,
    pub after_report: engine::evaluate::Report,
}

/// Apply operations to a copy and evaluate the result (shared by the API and AI proposals).
pub async fn preview_ops(cfg: std::sync::Arc<crate::config_store::Loaded>, base: engine::model::Architecture, ops: Vec<Op>, waivers: Vec<engine::config::Waiver>, lang: String) -> ApiResult<Preview> {
    let lim = limits(&cfg);
    let ec = cfg.engine_config(&lang);
    let (base2, ec2, ops2) = (base.clone(), ec.clone(), ops.clone());
    let outcome = tokio::task::spawn_blocking(move || apply_ops(&base2, &ops2, &ec2.catalog, &lim))
        .await
        .map_err(ApiError::internal)?
        .map_err(|e| ApiError::bad_request("ops_invalid", e.to_string()).with_details(json!({"issues": e.issues})))?;
    let before = run_evaluate(cfg.clone(), base.clone(), waivers.clone(), lang.clone()).await?;
    let after = run_evaluate(cfg.clone(), outcome.architecture.clone(), waivers, lang).await?;
    let (sb, sa) = (summarize(&before), summarize(&after));
    let before_open: HashSet<&String> = sb.open.iter().collect();
    let after_open: HashSet<&String> = sa.open.iter().collect();
    let introduced = sa.open.iter().filter(|x| !before_open.contains(x)).cloned().collect();
    let resolved = sb.open.iter().filter(|x| !after_open.contains(x)).cloned().collect();
    let d = diff(&base, &outcome.architecture, false);
    Ok(Preview { outcome, diff: d, before: sb, after: sa, introduced, resolved, after_report: after })
}

pub async fn apply(State(state): State<AppState>, user: AuthUser, Json(b): Json<ApplyBody>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    // the base is looked at, so that operations which put right a number stored out of range can be applied; what they make of it is written
    let base = parse_viewed_arch(&cfg, b.architecture)?;
    let lang = lang_of(&cfg, b.lang.as_deref());
    if !b.preview {
        let lim = limits(&cfg);
        let ec = cfg.engine_config(&lang);
        let outcome = apply_ops(&base, &b.ops, &ec.catalog, &lim).map_err(|e| ApiError::bad_request("ops_invalid", e.to_string()).with_details(json!({"issues": e.issues})))?;
        return Ok(Json(serde_json::to_value(outcome)?));
    }
    let waivers = match b.project_id {
        Some(id) => engine_waivers(&state, id).await?,
        None => vec![],
    };
    let p = preview_ops(cfg, base, b.ops, waivers, lang).await?;
    Ok(Json(json!({
        "architecture": p.outcome.architecture, "cascadedEdges": p.outcome.cascaded_edges, "renamed": p.outcome.renamed,
        "diff": p.diff, "before": p.before, "after": p.after, "introduced": p.introduced, "resolved": p.resolved,
    })))
}

pub async fn get_template(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    let t = cfg.templates.iter().find(|t| t.id == id && t.enabled).ok_or_else(|| ApiError::not_found("template not found"))?;
    Ok(Json(serde_json::to_value(t)?))
}

#[derive(Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub download: Option<u8>,
    /// `draft` (default) or a version number.
    #[serde(default)]
    pub version: Option<String>,
}

pub async fn export(State(state): State<AppState>, _user: AuthUser, Path((id, format)): Path<(Uuid, String)>, Query(q): Query<ExportQuery>) -> ApiResult<Response> {
    let cfg = state.cfg().await?;
    if !engine::export::FORMATS.contains(&format.as_str()) {
        return Err(ApiError::bad_request("unknown_format", format!("format must be one of: {}", engine::export::FORMATS.join(", "))));
    }
    let which = q.version.clone().unwrap_or_else(|| "draft".into());
    let (name, arch_v): (String, Value) = if which == "draft" {
        sqlx::query_as("select name, draft from projects where id = $1 and deleted_at is null").bind(id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("project not found"))?
    } else {
        let n: i32 = which.parse().map_err(|_| ApiError::bad_request("invalid_version", "version must be 'draft' or a number"))?;
        let (pn,): (String,) =
            sqlx::query_as("select name from projects where id = $1 and deleted_at is null").bind(id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("project not found"))?;
        let (a,): (Value,) = sqlx::query_as("select architecture from versions where project_id = $1 and n = $2")
            .bind(id)
            .bind(n)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("version not found"))?;
        (pn, a)
    };
    let mut arch = parse_arch(arch_v)?;
    if arch.meta.name.is_none() {
        arch.meta.name = Some(name.clone());
    }
    let lang = lang_of(&cfg, q.lang.as_deref());
    let waivers = engine_waivers(&state, id).await?;
    let cfg2 = cfg.clone();
    let l2 = lang.clone();
    let fmt = format.clone();
    let export = tokio::task::spawn_blocking(move || {
        let ec = cfg2.engine_config(&l2);
        let report = engine::evaluate::evaluate(&arch, &ec, &waivers);
        engine::export::generate(&fmt, &arch, &ec, &report, &waivers, &l2, &today())
    })
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::bad_request("unknown_format", "unknown format"))?;
    let files = export.files;

    let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>().trim_matches('-').to_string();
    let slug = if slug.is_empty() { "architecture".to_string() } else { slug };
    if q.download.unwrap_or(0) == 0 {
        return json_response(json!({"format": format, "files": files, "report": export.report}));
    }
    if files.len() == 1 {
        let f = &files[0];
        let ct = if f.path.ends_with(".json") { "application/json" } else { "text/plain; charset=utf-8" };
        return Response::builder()
            .header(header::CONTENT_TYPE, ct)
            .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}-{}\"", slug, f.path.rsplit('/').next().unwrap_or("file")))
            .body(Body::from(f.content.clone()))
            .map_err(ApiError::internal);
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for f in &files {
            zip.start_file(f.path.clone(), opts).map_err(ApiError::internal)?;
            zip.write_all(f.content.as_bytes()).map_err(ApiError::internal)?;
        }
        zip.finish().map_err(ApiError::internal)?;
    }
    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}-{}.zip\"", slug, format))
        .body(Body::from(buf.into_inner()))
        .map_err(ApiError::internal)
}

fn json_response(v: Value) -> ApiResult<Response> {
    Response::builder().status(StatusCode::OK).header(header::CONTENT_TYPE, "application/json").body(Body::from(serde_json::to_vec(&v)?)).map_err(ApiError::internal)
}
