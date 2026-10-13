//! HTTP routing, middleware (security headers, metrics, CSRF, body limit) and static files.

pub mod admin;
pub mod ai;
pub mod engine_api;
pub mod placement;
pub mod projects;
pub mod public;

use crate::auth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{DefaultBodyLimit, MatchedPath, Request, State};
use axum::http::{header, HeaderValue};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, post, put};
use axum::Router;
use std::time::Instant;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

async fn api_not_found() -> ApiError {
    ApiError::not_found("no such endpoint")
}

async fn security_headers(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    let set = |h: &mut axum::http::HeaderMap, k: &'static str, v: &'static str| {
        if !h.contains_key(k) {
            h.insert(k, HeaderValue::from_static(v));
        }
    };
    set(h, "x-content-type-options", "nosniff");
    set(h, "x-frame-options", "DENY");
    set(h, "referrer-policy", "same-origin");
    set(h, "permissions-policy", "camera=(), microphone=(), geolocation=()");
    set(h, "cross-origin-opener-policy", "same-origin");
    // the SPA needs inline styles (Vue Flow) but never inline scripts or foreign origins
    set(h, "content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'");
    if path.starts_with("/assets/") {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
    } else if path.starts_with("/api/") {
        if !h.contains_key(header::CACHE_CONTROL) {
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        }
    } else if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    res
}

async fn track(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let route = req.extensions().get::<MatchedPath>().map(|m| m.as_str().to_string()).unwrap_or_else(|| "unmatched".to_string());
    let t = Instant::now();
    let res = next.run(req).await;
    let status = res.status().as_u16().to_string();
    state.metrics.http_requests.with_label_values(&[&method, &route, &status]).inc();
    state.metrics.http_duration.with_label_values(&[&method, &route]).observe(t.elapsed().as_secs_f64());
    res
}

pub async fn router(state: AppState) -> Router {
    let cfg = state.cfg().await.ok();
    let body_limit = cfg.as_ref().map(|c| c.settings.num("limits.maxBodyMb") as usize).unwrap_or(4).max(1) * 1024 * 1024;

    let api = Router::new()
        .route("/public/config", get(public::public_config))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/password", post(auth::change_password))
        .route("/bootstrap", get(public::bootstrap))
        .route("/projects", get(projects::list).post(projects::create))
        .route("/projects/{id}", get(projects::get).patch(projects::patch).delete(projects::delete))
        .route("/projects/{id}/draft", put(projects::save_draft))
        .route("/projects/{id}/versions", get(projects::list_versions).post(projects::create_version))
        .route("/projects/{id}/versions/{n}", get(projects::get_version))
        .route("/projects/{id}/versions/{n}/restore", post(projects::restore_version))
        .route("/projects/{id}/diff", get(projects::diff))
        .route("/projects/{id}/waivers", post(projects::create_waiver))
        .route("/projects/{id}/waivers/{wid}", delete(projects::revoke_waiver))
        .route("/projects/{id}/export/{format}", get(engine_api::export))
        .route("/projects/{id}/ai/jobs", post(ai::create_job))
        .route("/projects/{id}/chat", get(ai::chat_history).delete(ai::clear_chat))
        .route("/ai/jobs/{id}", get(ai::get_job))
        .route("/ai/jobs/{id}/cancel", post(ai::cancel_job))
        .route("/proposals/{id}", get(ai::get_proposal))
        .route("/proposals/{id}/apply", post(ai::apply_proposal))
        .route("/proposals/{id}/reject", post(ai::reject_proposal))
        .route("/evaluate", post(engine_api::evaluate))
        .route("/diff", post(engine_api::diff_architectures))
        .route("/simulate", post(engine_api::simulate_scenario))
        .route("/ops/apply", post(engine_api::apply))
        .route("/templates/{id}", get(engine_api::get_template))
        .route("/placement/options", get(placement::options))
        .route("/placement/preview", post(placement::preview))
        .route("/placement/compare", post(placement::compare))
        .route("/admin/settings", get(admin::get_settings).put(admin::put_settings))
        .route("/admin/llm/models", get(admin::llm_models))
        .route("/admin/llm/test", post(admin::llm_test))
        .route("/admin/llm/usage", get(admin::llm_usage))
        .route("/versions", get(public::versions))
        .route("/admin/versions/sync", post(admin::versions_sync))
        .route("/admin/versions/status", get(admin::versions_status))
        .route("/admin/config", get(admin::config_kinds))
        .route("/admin/config-export", get(admin::config_export))
        .route("/admin/config-import", post(admin::config_import))
        .route("/admin/config/{kind}", get(admin::config_list).post(admin::config_create))
        .route("/admin/config/{kind}/{id}", put(admin::config_put).delete(admin::config_delete))
        .route("/admin/config/{kind}/{id}/reset", post(admin::config_reset))
        .route("/admin/users", get(admin::users_list).post(admin::users_create))
        .route("/admin/users/{id}", axum::routing::patch(admin::users_patch))
        .route("/admin/users/{id}/reset-password", post(admin::users_reset_password))
        .route("/admin/audit", get(admin::audit_list))
        .route("/admin/system", get(admin::system))
        .fallback(api_not_found)
        .layer(middleware::from_fn_with_state(state.clone(), auth::csrf_guard));

    let web = state.boot.web_dir.clone();
    let index = web.join("index.html");
    let static_files = ServeDir::new(&web).append_index_html_on_directories(true).fallback(ServeFile::new(index));

    Router::new()
        .route("/healthz", get(public::healthz))
        .route("/readyz", get(public::readyz))
        .route("/metrics", get(public::metrics))
        .nest("/api", api)
        .fallback_service(static_files)
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(middleware::from_fn_with_state(state.clone(), track))
        .layer(middleware::from_fn(security_headers))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
