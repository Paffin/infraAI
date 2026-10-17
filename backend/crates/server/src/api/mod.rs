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
use axum::http::{header, HeaderValue, Method, StatusCode};
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

/// A path that exists but not for this method. axum adds the `Allow` header to a `405` itself.
async fn api_method_not_allowed() -> ApiError {
    ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "this path does not take that method (the Allow header lists the ones it takes)")
}

async fn security_headers(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    let found = matches!(res.status().as_u16(), 200 | 206 | 304);
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
        // built files carry a hash of their content in the name, so the file that is there never changes; one that is not there must not be remembered
        // for a year as if it were (a tab opened before a deploy asks for a chunk the new build no longer has)
        if found {
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
        } else {
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        }
    } else if path.starts_with("/api/") {
        if !h.contains_key(header::CACHE_CONTROL) {
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        }
    } else if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    res
}

/// The `method` label of the request metrics. A fixed set, because a label that is whatever the client sent is a series per value on a public
/// endpoint: HTTP takes any token as a method, and every new one creates fifteen series (twelve buckets, the sum, the count and the counter) that are never
/// freed (measured: twenty invented methods grew the registry from 752 to 1,052 series and a scrape from 87 KB to 120 KB). Everything else is `other`.
pub fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::HEAD => "HEAD",
        Method::OPTIONS => "OPTIONS",
        _ => "other",
    }
}

async fn track(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = method_label(req.method());
    let route = req.extensions().get::<MatchedPath>().map(|m| m.as_str().to_string()).unwrap_or_else(|| "unmatched".to_string());
    let t = Instant::now();
    let res = next.run(req).await;
    let status = res.status().as_u16().to_string();
    state.metrics.http_requests.with_label_values(&[method, &route, &status]).inc();
    state.metrics.http_duration.with_label_values(&[method, &route]).observe(t.elapsed().as_secs_f64());
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
        .route("/recommendations", post(engine_api::recommendations))
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
        .method_not_allowed_fallback(api_method_not_allowed)
        .layer(middleware::from_fn_with_state(state.clone(), auth::csrf_guard));

    let web = state.boot.web_dir.clone();
    let index = web.join("index.html");
    // the built files have a place of their own and no fallback: a file that is not there is a 404, not the index page of the application
    let assets = ServeDir::new(web.join("assets"));
    let static_files = ServeDir::new(&web).append_index_html_on_directories(true).fallback(ServeFile::new(index));

    // with METRICS_PORT the metrics are on another listener (`metrics_router`) and this port says so: the route stays, as an explicit 404, because the
    // single-page application below is the fallback and would answer 200 with its index page
    let metrics = if state.boot.metrics_port.is_some() { get(public::metrics_moved) } else { get(public::metrics) };

    Router::new()
        .route("/healthz", get(public::healthz))
        .route("/readyz", get(public::readyz))
        .route("/metrics", metrics)
        .nest("/api", api)
        .nest_service("/assets", assets)
        .fallback_service(static_files)
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(middleware::from_fn_with_state(state.clone(), track))
        .layer(middleware::from_fn(security_headers))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// The router of the metrics listener (`METRICS_PORT`): `GET /metrics` under the rules of [`public::metrics`] and nothing else, so that a port which is not
/// published (`deploy/k8s`: the Service has no such port, the NetworkPolicy admits the monitoring namespace only) serves no part of the application.
pub fn metrics_router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(public::metrics))
        .fallback(|| async { ApiError::not_found("this port serves /metrics and nothing else") })
        .layer(middleware::from_fn_with_state(state.clone(), track))
        .layer(middleware::from_fn(security_headers))
        .layer(CompressionLayer::new())
        .with_state(state)
}

/// Binds the metrics listener on `HOST` and `METRICS_PORT`, or returns `None` when the variable is not set. An address that cannot be bound is an error
/// the caller must not ignore: an operator who asked for `/metrics` off the public port must not get it silently on neither.
pub async fn bind_metrics(boot: &crate::config::BootConfig) -> std::io::Result<Option<tokio::net::TcpListener>> {
    let Some(port) = boot.metrics_port else { return Ok(None) };
    tokio::net::TcpListener::bind(std::net::SocketAddr::new(boot.bind.ip(), port)).await.map(Some)
}

/// Serves [`metrics_router`] on `listener` until `shutdown` completes (then the requests in flight finish).
pub async fn serve_metrics(listener: tokio::net::TcpListener, state: AppState, shutdown: impl std::future::Future<Output = ()> + Send + 'static) -> std::io::Result<()> {
    axum::serve(listener, metrics_router(state).into_make_service()).with_graceful_shutdown(shutdown).await
}
