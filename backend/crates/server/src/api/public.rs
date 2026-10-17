//! Health, readiness, metrics and the data every page needs at start-up.

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::json::Json;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
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

/// Does the `Authorization` header carry exactly `Bearer <token>`? The token is compared in constant time (`subtle`), so the time of a refusal does not say how
/// many leading characters of a guess were right; a different length is refused at once, which tells nothing that the length of the header does not.
/// The scheme is matched as Prometheus writes it (`Bearer`), nothing else is accepted.
pub fn bearer_matches(header: Option<&str>, token: &str) -> bool {
    use subtle::ConstantTimeEq;
    header.and_then(|v| v.strip_prefix("Bearer ")).map(|given| bool::from(given.as_bytes().ct_eq(token.as_bytes()))).unwrap_or(false)
}

/// `GET /metrics`: Prometheus text. Served on the main port, or (with `METRICS_PORT`) on a listener of its own by [`crate::api::metrics_router`], under the same
/// rules: off with `observability.metricsEnabled = false` (404), and with `METRICS_TOKEN` set a bearer token is required (401).
pub async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let enabled = state.cfg().await.map(|c| c.settings.boolean("observability.metricsEnabled")).unwrap_or(true);
    if !enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(token) = &state.boot.metrics_token {
        if !bearer_matches(headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()), token) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    Response::builder().header(header::CONTENT_TYPE, "text/plain; version=0.0.4").body(Body::from(state.metrics.render())).unwrap()
}

/// `GET /metrics` on the main port once `METRICS_PORT` moved it elsewhere. An explicit answer and not a missing route: the single-page application is the
/// fallback of this router, and a missing route would answer `200 text/html`, the application's index page, which looks like a success to a scraper and
/// like a hole to anyone who looks.
pub async fn metrics_moved() -> ApiError {
    ApiError::not_found("the metrics are not served on this port: Prometheus scrapes the metrics port (METRICS_PORT)")
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
/// The bootstrap body: everything that is built per request, and the catalog as the text it already is.
#[derive(serde::Serialize)]
struct WithCatalog<'a> {
    #[serde(flatten)]
    rest: &'a Value,
    catalog: &'a serde_json::value::RawValue,
}

pub async fn bootstrap(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> ApiResult<Response> {
    let cfg = state.cfg().await?;
    let templates: Vec<Value> = cfg
        .templates
        .iter()
        .filter(|t| t.enabled)
        .map(|t| json!({"id": t.id, "name": t.name, "description": t.description, "badge": t.badge, "tags": t.tags, "order": t.order, "nodeCount": t.architecture.nodes.len(), "requirements": t.architecture.requirements}))
        .collect();
    let a = &cfg.engine.assumptions;
    // timings and thresholds of the web UI: the `editor.*` settings, by their short names
    let editor: serde_json::Map<String, Value> = [
        "autosaveMs",
        "evaluateMs",
        "simulateMs",
        "recommendMs",
        "historySteps",
        "coalesceMs",
        "saveRetryMs",
        "saveRetryMaxMs",
        "unsavedKeepHours",
        "jobPollMs",
        "lodCompactBelow",
        "lodMiniBelow",
        "toastMs",
        "errorToastMs",
        "undoToastMs",
        "highlightMs",
        "paletteMaxResults",
        "layoutZonesPerRow",
        "quietEdgesSites",
        "playbackStepMs",
        "failureLeadMs",
        "flowSpeed",
        "flowSpacing",
        "flowParticlesMax",
        "rightPanelWidth",
        "rightPanelMinWidth",
        "rightPanelMaxWidth",
        "floatPanelWidth",
        "impactShown",
        "pickerShown",
        "librarySearchShown",
        "galleryTagsShown",
        "adminListShown",
    ]
    .iter()
    .map(|k| (k.to_string(), cfg.settings.get(&format!("editor.{k}"))))
    .collect();
    let body = json!({
        "user": user,
        "rev": cfg.rev,
        "version": env!("CARGO_PKG_VERSION"),
        "engineVersion": engine::evaluate::ENGINE_VERSION,
        // what the "About" window says: the license and where the source is (from the crate manifest, one place for them)
        "about": {"license": env!("CARGO_PKG_LICENSE"), "repository": env!("CARGO_PKG_REPOSITORY"), "homepage": env!("CARGO_PKG_HOMEPAGE")},
        "app": {
            "name": cfg.settings.str("app.name"),
            "defaultLanguage": cfg.settings.str("app.defaultLanguage"),
            "languages": cfg.settings.list("app.languages"),
            "theme": cfg.settings.get("ui.theme"),
            "motion": cfg.settings.str("ui.motion"),
            "motionScale": cfg.settings.num("ui.motionScale"),
            "onboarding": cfg.settings.boolean("ui.onboarding"),
            "helpUrl": cfg.settings.str("ui.helpUrl"),
            "quickPrompts": cfg.settings.get("ui.quickPrompts"),
        },
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
            "availability": {"enabled": cfg.engine.reliability.enabled, "presets": cfg.engine.reliability.presets, "warnPercent": cfg.engine.reliability.warn_percent, "daysPerYear": cfg.engine.reliability.days_per_year},
            "recommendations": {"enabled": cfg.engine.recommendations.enabled, "show": cfg.engine.recommendations.show},
        },
    });
    // conditional GET: the catalog is megabytes and changes only with the configuration, whose revision (`rev`) is in the rest of the body, so the tag is
    // made from the rest; the catalog is written from the text made when the configuration was loaded, and only when it has to be sent
    let etag = format!("\"{}\"", &crate::crypto::sha256_hex(&serde_json::to_vec(&body)?)[..24]);
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return Ok(Response::builder().status(StatusCode::NOT_MODIFIED).header(header::ETAG, etag).body(Body::empty()).unwrap());
    }
    let bytes = serde_json::to_vec(&WithCatalog { rest: &body, catalog: &cfg.catalog_json })?;
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

#[cfg(test)]
mod tests {
    use super::bearer_matches;

    #[test]
    fn bearer_matches_only_the_exact_token() {
        let token = "s3cret-Token_0123456789";
        // the one thing that passes
        assert!(bearer_matches(Some("Bearer s3cret-Token_0123456789"), token));
        // a strict prefix of the token, the token and one more character, a near miss in the last or the first character, other case
        assert!(!bearer_matches(Some("Bearer s3cret-Token_012345678"), token));
        assert!(!bearer_matches(Some("Bearer s3cret-Token_01234567890"), token));
        assert!(!bearer_matches(Some("Bearer s3cret-Token_012345678X"), token));
        assert!(!bearer_matches(Some("Bearer X3cret-Token_0123456789"), token));
        assert!(!bearer_matches(Some("Bearer S3CRET-TOKEN_0123456789"), token));
        // no header, nothing after the scheme, another scheme, the bare token, a doubled space, surrounding space
        assert!(!bearer_matches(None, token));
        assert!(!bearer_matches(Some("Bearer "), token));
        assert!(!bearer_matches(Some("Bearer"), token));
        assert!(!bearer_matches(Some(""), token));
        assert!(!bearer_matches(Some("Basic s3cret-Token_0123456789"), token));
        assert!(!bearer_matches(Some("s3cret-Token_0123456789"), token));
        assert!(!bearer_matches(Some("Bearer  s3cret-Token_0123456789"), token));
        assert!(!bearer_matches(Some("Bearer s3cret-Token_0123456789 "), token));
    }

    #[test]
    fn bearer_matches_judges_bytes_so_non_ascii_near_misses_do_not_pass_or_panic() {
        // "é" is two bytes in UTF-8: a guess that is as long in bytes but not in characters, or cut in the middle of a character, is a different token
        let token = "tökén-ключ";
        assert!(bearer_matches(Some("Bearer tökén-ключ"), token));
        assert!(!bearer_matches(Some("Bearer tokén-ключ"), token));
        assert!(!bearer_matches(Some("Bearer tökén-ключ!"), token));
        assert!(!bearer_matches(Some("Bearer tökén-клю"), token));
        // the same visible text in another normalisation form is not the same token
        assert!(!bearer_matches(Some("Bearer to\u{308}ke\u{301}n-ключ"), token));
        // an empty guess against a token
        assert!(!bearer_matches(Some("Bearer "), "x"));
    }
}
