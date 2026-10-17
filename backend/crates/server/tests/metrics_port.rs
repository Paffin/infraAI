//! `METRICS_PORT`: the metrics on a listener of their own, and the main port saying 404. Real listeners on real sockets, the real router over a real database;
//! the listener is the one `main.rs` starts (`api::bind_metrics` and `api::serve_metrics`).
//!
//! The web directory of these tests holds an `index.html`: the single-page application is the fallback of the main router, and with no web directory a
//! missing route would be a 404 by accident and the test would pass without the feature.
mod common;

use axum::http::{header, Method, StatusCode};
use common::*;
use serde_json::{json, Value};

const INDEX: &str = "<!doctype html><title>the application</title><div id=\"app\"></div>";

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// What the metrics listener answered: status, content type, body.
async fn get(base: &str, path: &str, authorization: Option<&str>) -> (u16, String, String) {
    let mut req = client().get(format!("{base}{path}"));
    if let Some(a) = authorization {
        req = req.header("authorization", a);
    }
    let res = req.send().await.expect("the metrics listener answers");
    let status = res.status().as_u16();
    let content_type = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    (status, content_type, res.text().await.unwrap())
}

/// The application with `METRICS_PORT` set (to 0: the operating system picks the port, and the listener says which) and its metrics listener running.
async fn with_metrics_port(web: &std::path::Path, token: Option<&str>) -> (TestApp, String) {
    let app = TestApp::with_boot(|b| {
        b.web_dir = web.to_path_buf();
        b.metrics_port = Some(0);
        b.metrics_token = token.map(String::from);
    })
    .await;
    let listener = server::api::bind_metrics(&app.state.boot).await.expect("the port can be bound").expect("METRICS_PORT is set");
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(server::api::serve_metrics(listener, app.state.clone(), std::future::pending()));
    (app, base)
}

async fn metrics_on_main(app: &TestApp, authorization: Option<&str>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let extra: Vec<(&str, &str)> = authorization.map(|a| ("authorization", a)).into_iter().collect();
    app.request(Method::GET, "/metrics", None, None, &extra).await
}

#[tokio::test]
async fn with_a_metrics_port_the_scrape_is_there_and_the_main_port_answers_404() {
    let web = temp_web_dir(&[("index.html", INDEX)]);
    let (app, base) = with_metrics_port(&web, None).await;

    // the application is served from this directory: a path that is no route is its index page, 200 text/html. So the 404 below is a route that says no
    let (s, h, v) = app.request(Method::GET, "/metrics/", None, None, &[]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(h[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"), "{h:?}");
    assert!(v.as_str().unwrap().contains("the application"));

    // some traffic, so that the request counter has series
    let (s, _, _) = app.request(Method::GET, "/healthz", None, None, &[]).await;
    assert_eq!(s, StatusCode::OK);

    // the main port: 404, in the one JSON shape of the API, not the index page and not the metrics
    let (s, h, v) = metrics_on_main(&app, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    assert!(!h[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"), "the application's index page is not an answer to /metrics: {h:?}");
    assert_eq!(v["error"]["code"], "not_found", "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("METRICS_PORT"), "the answer says where the metrics went: {v}");
    assert!(!v.to_string().contains("infra_http_requests_total"));
    // a scraper that sends its token to the old address gets the same: a token does not bring the route back
    let (s, _, _) = metrics_on_main(&app, Some("Bearer anything")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // the metrics port: 200, Prometheus text, with the series of the request that was just made
    let (s, ct, body) = get(&base, "/metrics", None).await;
    assert_eq!(s, 200, "{body}");
    assert!(ct.starts_with("text/plain"), "{ct}");
    assert!(body.contains("infra_http_requests_total"), "{body}");
    assert!(body.contains("route=\"/healthz\""), "the scrape shows what the main port served");
    // no token is configured: a header that is sent is not looked at
    assert_eq!(get(&base, "/metrics", Some("Bearer whatever")).await.0, 200);

    // and nothing but /metrics: not the application, not the API, not the probes
    for path in ["/", "/healthz", "/readyz", "/api/bootstrap", "/api/public/config", "/index.html", "/metrics/", "/metricsX"] {
        let (s, ct, body) = get(&base, path, None).await;
        assert_eq!(s, 404, "{path}: {body}");
        assert!(!ct.starts_with("text/html") && !body.contains("the application"), "{path} is the application's page on the metrics port");
    }
    let res = client().post(format!("{base}/metrics")).send().await.unwrap();
    assert_eq!(res.status().as_u16(), 405, "a scrape is a GET");

    app.cleanup().await;
    let _ = std::fs::remove_dir_all(web);
}

#[tokio::test]
async fn without_a_metrics_port_the_main_port_serves_them_as_before() {
    let web = temp_web_dir(&[("index.html", INDEX)]);
    let app = TestApp::with_boot(|b| b.web_dir = web.clone()).await;
    assert!(server::api::bind_metrics(&app.state.boot).await.unwrap().is_none(), "no variable, no second listener");
    app.request(Method::GET, "/healthz", None, None, &[]).await;
    let (s, h, v) = metrics_on_main(&app, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/plain"), "{h:?}");
    assert!(v.as_str().unwrap().contains("infra_http_requests_total"), "{v}");
    app.cleanup().await;
    let _ = std::fs::remove_dir_all(web);
}

#[tokio::test]
async fn the_bearer_token_is_required_on_the_listener_that_serves_the_metrics() {
    let token = "scrape-token-0123456789";
    let web = temp_web_dir(&[("index.html", INDEX)]);
    let wrong = [
        None,
        Some("Bearer wrong"),
        Some("Bearer scrape-token-012345678"),
        Some("Bearer scrape-token-01234567890"),
        Some("Basic scrape-token-0123456789"),
        Some("scrape-token-0123456789"),
        Some("Bearer "),
    ];

    // on the metrics port ...
    let (app, base) = with_metrics_port(&web, Some(token)).await;
    for auth in wrong {
        let (s, _, body) = get(&base, "/metrics", auth).await;
        assert_eq!(s, 401, "{auth:?}");
        assert!(!body.contains("infra_"), "a refusal shows no metrics");
    }
    assert_eq!(get(&base, "/metrics", Some("Bearer scrape-token-0123456789")).await.0, 200);
    app.cleanup().await;

    // ... and on the main port when there is no metrics port
    let app = TestApp::with_boot(|b| {
        b.web_dir = web.clone();
        b.metrics_token = Some(token.into());
    })
    .await;
    for auth in wrong {
        let (s, _, v) = metrics_on_main(&app, auth).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{auth:?}: {v}");
    }
    let (s, _, v) = metrics_on_main(&app, Some("Bearer scrape-token-0123456789")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v.as_str().unwrap().contains("infra_"));
    app.cleanup().await;
    let _ = std::fs::remove_dir_all(web);
}

#[tokio::test]
async fn the_setting_that_switches_the_metrics_off_does_it_on_either_port() {
    let web = temp_web_dir(&[("index.html", INDEX)]);

    let (app, base) = with_metrics_port(&web, None).await;
    let c = app.login_admin().await;
    assert_eq!(get(&base, "/metrics", None).await.0, 200);
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"observability.metricsEnabled": false}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(get(&base, "/metrics", None).await.0, 404, "off on the metrics port");
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"observability.metricsEnabled": true}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(get(&base, "/metrics", None).await.0, 200, "and on again");
    app.cleanup().await;

    let app = TestApp::with_boot(|b| b.web_dir = web.clone()).await;
    let c = app.login_admin().await;
    assert_eq!(metrics_on_main(&app, None).await.0, StatusCode::OK);
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"observability.metricsEnabled": false}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(metrics_on_main(&app, None).await.0, StatusCode::NOT_FOUND, "off on the main port");
    app.cleanup().await;
    let _ = std::fs::remove_dir_all(web);
}

#[tokio::test]
async fn a_metrics_port_that_is_taken_is_an_error_for_the_caller_not_a_silent_fallback() {
    let web = temp_web_dir(&[("index.html", INDEX)]);
    let (app, _) = with_metrics_port(&web, None).await;
    // somebody else holds a port; asking for the same address again must fail (main.rs turns the error into "the server does not start")
    let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut boot = (*app.state.boot).clone();
    boot.metrics_port = Some(held.local_addr().unwrap().port());
    let err = server::api::bind_metrics(&boot).await.expect_err("the port is in use");
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    app.cleanup().await;
    let _ = std::fs::remove_dir_all(web);
}
