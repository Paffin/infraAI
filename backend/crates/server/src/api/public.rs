//! Health, readiness, metrics and the data every page needs at start-up.

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

/// Liveness: the process is up.
pub async fn healthz() -> &'static str {
    "ok"
}

/// Readiness: the database answers and the instance is not draining.
pub async fn readyz(State(state): State<AppState>) -> Response {
    if state.shutting_down.load(Ordering::SeqCst) {
        return (StatusCode::SERVICE_UNAVAILABLE, "draining").into_response();
    }
    match sqlx::query("select 1").execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, "ready").into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "database unavailable").into_response(),
    }
}

pub async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let enabled = state.cfg().await.map(|c| c.settings.boolean("observability.metricsEnabled")).unwrap_or(true);
    if !enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(token) = &state.boot.metrics_token {
        let ok = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).map(|v| v == format!("Bearer {token}")).unwrap_or(false);
        if !ok {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    Response::builder().header(header::CONTENT_TYPE, "text/plain; version=0.0.4").body(Body::from(state.metrics.render())).unwrap()
}

/// What the login page needs before anybody is signed in.
pub async fn public_config(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    Ok(Json(json!({
        "appName": cfg.settings.str("app.name"),
        "defaultLanguage": cfg.settings.str("app.defaultLanguage"),
        "languages": cfg.settings.list("app.languages"),
        "theme": cfg.settings.get("ui.theme"),
        "motion": cfg.settings.str("ui.motion"),
        "motionScale": cfg.settings.num("ui.motionScale"),
    })))
}

/// Everything the editor needs: catalog, rules (for rendering findings), templates, limits.
pub async fn bootstrap(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> ApiResult<Response> {
    let cfg = state.cfg().await?;
    let templates: Vec<Value> = cfg
        .templates
        .iter()
        .filter(|t| t.enabled)
        .map(|t| json!({"id": t.id, "name": t.name, "description": t.description, "badge": t.badge, "order": t.order, "nodeCount": t.architecture.nodes.len(), "requirements": t.architecture.requirements}))
        .collect();
    let a = &cfg.engine.assumptions;
    // timings and thresholds of the web UI: the `editor.*` settings, by their short names
    let editor: serde_json::Map<String, Value> = [
        "autosaveMs",
        "evaluateMs",
        "simulateMs",
        "historySteps",
        "saveRetryMs",
        "saveRetryMaxMs",
        "jobPollMs",
        "lodCompactBelow",
        "lodMiniBelow",
        "toastMs",
        "errorToastMs",
        "highlightMs",
        "paletteMaxResults",
        "layoutZonesPerRow",
        "playbackStepMs",
    ]
    .iter()
    .map(|k| (k.to_string(), cfg.settings.get(&format!("editor.{k}"))))
    .collect();
    let body = json!({
        "user": user,
        "rev": cfg.rev,
        "version": env!("CARGO_PKG_VERSION"),
        "engineVersion": engine::evaluate::ENGINE_VERSION,
        "app": {
            "name": cfg.settings.str("app.name"),
            "defaultLanguage": cfg.settings.str("app.defaultLanguage"),
            "languages": cfg.settings.list("app.languages"),
            "theme": cfg.settings.get("ui.theme"),
            "motion": cfg.settings.str("ui.motion"),
            "motionScale": cfg.settings.num("ui.motionScale"),
            "quickPrompts": cfg.settings.get("ui.quickPrompts"),
        },
        "catalog": cfg.catalog,
        "rules": cfg.rules,
        "templates": templates,
        "exportFormats": engine::export::FORMATS.iter().map(|f| json!({"id": f, "group": engine::export::group_of(f)})).collect::<Vec<_>>(),
        "ai": {
            "enabled": cfg.settings.boolean("llm.enabled") && state.llm_key(&cfg).is_some(),
            "configured": state.llm_key(&cfg).is_some(),
            "maxPromptChars": cfg.settings.num("llm.maxPromptChars"),
            "plannerModel": cfg.settings.str("llm.model.planner"),
            "fastModel": cfg.settings.str("llm.model.fast"),
        },
        "editor": editor,
        "limits": {"maxNodes": cfg.settings.num("limits.maxNodes"), "maxEdges": cfg.settings.num("limits.maxEdges"), "maxZones": cfg.settings.num("limits.maxZones"), "waiverMinReasonChars": cfg.settings.num("limits.waiverMinReasonChars"), "waiverMaxDays": cfg.settings.num("limits.waiverMaxDays")},
        "today": crate::versions::today_utc(),
        "engine": {
            "currency": cfg.engine.cost.currency, "symbol": cfg.engine.cost.symbol,
            "lifecycle": cfg.engine.lifecycle,
            "minCapacityDuringFailure": a.min_capacity_during_failure, "headroomPercent": a.headroom_percent,
            "defaultFailureTolerance": a.default_failure_tolerance,
            "toleranceLevels": cfg.engine.tolerance.levels,
            "maxSiteFailures": cfg.engine.scenarios.max_site_failures,
        },
    });
    // conditional GET: the catalog is large and rarely changes
    let bytes = serde_json::to_vec(&body)?;
    let etag = format!("\"{}\"", &crate::crypto::sha256_hex(&bytes)[..24]);
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return Ok(Response::builder().status(StatusCode::NOT_MODIFIED).header(header::ETAG, etag).body(Body::empty()).unwrap());
    }
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "private, no-cache")
        .body(Body::from(bytes))
        .map_err(ApiError::internal)
}

/// The release tables (lifecycle of every product the catalog knows) with today's date: the editor shows support dates and
/// recommended lines from them. Conditional GET: the tables change when someone refreshes them.
pub async fn versions(State(state): State<AppState>, _user: AuthUser, headers: HeaderMap) -> ApiResult<Response> {
    let cfg = state.cfg().await?;
    let body = json!({"today": crate::versions::today_utc(), "tables": cfg.versions});
    let bytes = serde_json::to_vec(&body)?;
    let etag = format!("\"{}\"", &crate::crypto::sha256_hex(&bytes)[..24]);
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return Ok(Response::builder().status(StatusCode::NOT_MODIFIED).header(header::ETAG, etag).body(Body::empty()).unwrap());
    }
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "private, no-cache")
        .body(Body::from(bytes))
        .map_err(ApiError::internal)
}
