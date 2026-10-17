//! Test harness: a fresh PostgreSQL database per test and the real router. No doubles of external services:
//! everything that talks to the model lives in `tests/live.rs` (real OpenRouter) or in pure functions.
#![allow(dead_code)] // shared by several test binaries, each uses a subset

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use server::config::BootConfig;
use server::state::AppState;
use std::net::SocketAddr;
use std::sync::Mutex;
use tower::ServiceExt;

pub fn admin_url() -> String {
    std::env::var("TEST_DATABASE_ADMIN_URL").unwrap_or_else(|_| "postgres://postgres@localhost:5433/postgres".to_string())
}

pub struct TestApp {
    pub router: Router,
    pub state: AppState,
    pub db_name: String,
    cookie: Mutex<Option<String>>,
}

pub fn boot(db_url: String) -> BootConfig {
    std::env::set_var("APP_ENV", "test");
    BootConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: db_url,
        db_max_connections: 5,
        db_connect_timeout_secs: 10,
        production: false,
        app_secret: "test-secret-test-secret-test-secret-123".into(),
        web_dir: "/nonexistent".into(),
        admin_email: Some("admin@test.local".into()),
        admin_password: Some("correct horse battery staple".into()),
        openrouter_api_key: None,
        cookie_secure: false,
        instance_id: "test".into(),
        metrics_token: None,
        metrics_port: None,
        trust_proxy: false,
        trust_proxy_hops: 1,
        shutdown_grace_secs: 1,
        log_json: false,
        seed_demo_project: None,
    }
}

/// A directory with the given files (`("assets/app-1.js", "…")`) to serve as the web application; the caller removes it.
pub fn temp_web_dir(files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ia-web-{}", uuid::Uuid::new_v4().simple()));
    for (name, text) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

impl TestApp {
    pub async fn new() -> TestApp {
        TestApp::with_boot(|_| {}).await
    }

    /// The application with its boot configuration changed by `tweak` (another web directory, secure cookies…).
    pub async fn with_boot(tweak: impl FnOnce(&mut BootConfig)) -> TestApp {
        let db_name = format!("t_{}", uuid::Uuid::new_v4().simple());
        let admin = sqlx::PgPool::connect(&admin_url()).await.expect("test postgres must be reachable (see docs/DEVELOPMENT.md)");
        sqlx::query(&format!("create database {db_name}")).execute(&admin).await.unwrap();
        let base = admin_url();
        let url = base.rsplit_once('/').map(|(a, _)| format!("{a}/{db_name}")).unwrap();
        let mut b = boot(url);
        tweak(&mut b);
        let state = server::startup::init_state(b).await.expect("init");
        let router = server::api::router(state.clone()).await;
        TestApp { router, state, db_name, cookie: Mutex::new(None) }
    }

    /// The same application on a pool of `max` connections that gives up after two seconds instead of the production ten.
    /// A handler that holds a connection (a transaction) and asks the pool for a second one waits for itself: with a pool of
    /// one that shows at once as a 500 here, instead of as a hang under load.
    pub async fn with_pool(max: u32) -> TestApp {
        let mut app = TestApp::new().await;
        let small = sqlx::postgres::PgPoolOptions::new()
            .max_connections(max)
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(&app.state.boot.database_url)
            .await
            .expect("a pool of the requested size");
        app.state.pool.close().await;
        app.state.pool = small;
        app.router = server::api::router(app.state.clone()).await;
        app
    }

    /// The same application whose passwords are hashed through a gate of `slots` (the setting `auth.hashConcurrency` is read at start; this is the
    /// way to have a gate of one without starting a server).
    pub async fn with_hash_slots(slots: usize) -> TestApp {
        let mut app = TestApp::new().await;
        app.state.passwords = std::sync::Arc::new(server::crypto::PasswordGate::new(slots, app.state.metrics.auth_password_checks.clone()));
        app.router = server::api::router(app.state.clone()).await;
        app
    }

    pub async fn request(&self, method: Method, path: &str, body: Option<Value>, cookie: Option<String>, extra: &[(&str, &str)]) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut b = Request::builder().method(method).uri(path).header(header::HOST, "localhost");
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, c);
        }
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        let req = if let Some(v) = body { b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())).unwrap() } else { b.body(Body::empty()).unwrap() };
        self.send(req).await
    }

    /// A request as a client that gets it wrong sends it: the body is text as it is (broken JSON, anything), with the content type the caller
    /// names (or none). Answers with the status, the headers and the body (JSON if it is, else the text).
    pub async fn raw(&self, method: Method, path: &str, body: Option<&str>, content_type: Option<&str>, cookie: Option<&str>) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut b = Request::builder().method(method).uri(path).header(header::HOST, "localhost");
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, c);
        }
        if let Some(ct) = content_type {
            b = b.header(header::CONTENT_TYPE, ct);
        }
        let req = b.body(body.map(|t| Body::from(t.to_string())).unwrap_or_else(Body::empty)).unwrap();
        self.send(req).await
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
        self.send_from("127.0.0.1", req).await
    }

    /// The request as it arrives from the TCP peer `peer` (the client, or the reverse proxy in front of the application).
    async fn send_from(&self, peer: &str, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut req = req;
        let addr: SocketAddr = format!("{peer}:50000").parse().unwrap_or_else(|_| format!("[{peer}]:50000").parse().expect("a peer address"));
        req.extensions_mut().insert(ConnectInfo(addr));
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()));
        (status, headers, v)
    }

    pub async fn login_as(&self, email: &str, password: &str) -> (StatusCode, Option<String>, Value) {
        let (s, h, v) = self.request(Method::POST, "/api/auth/login", Some(json!({"email": email, "password": password})), None, &[]).await;
        let cookie = h.get(header::SET_COOKIE).and_then(|c| c.to_str().ok()).map(|c| c.split(';').next().unwrap().to_string());
        (s, cookie, v)
    }

    pub async fn login_admin(&self) -> String {
        let (s, c, v) = self.login_as("admin@test.local", "correct horse battery staple").await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let c = c.expect("session cookie");
        *self.cookie.lock().unwrap() = Some(c.clone());
        c
    }

    pub async fn get(&self, cookie: &str, path: &str) -> (StatusCode, Value) {
        let (s, _, v) = self.request(Method::GET, path, None, Some(cookie.into()), &[]).await;
        (s, v)
    }
    pub async fn post(&self, cookie: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let (s, _, v) = self.request(Method::POST, path, Some(body), Some(cookie.into()), &[]).await;
        (s, v)
    }
    pub async fn put(&self, cookie: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let (s, _, v) = self.request(Method::PUT, path, Some(body), Some(cookie.into()), &[]).await;
        (s, v)
    }
    pub async fn patch(&self, cookie: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let (s, _, v) = self.request(Method::PATCH, path, Some(body), Some(cookie.into()), &[]).await;
        (s, v)
    }
    pub async fn delete(&self, cookie: &str, path: &str) -> (StatusCode, Value) {
        let (s, _, v) = self.request(Method::DELETE, path, None, Some(cookie.into()), &[]).await;
        (s, v)
    }

    /// A sign-in as it arrives from `peer` (an IP address) with extra request headers (`X-Forwarded-For` …): status, session cookie, body.
    pub async fn login_from(&self, peer: &str, extra: &[(&str, &str)], email: &str, password: &str) -> (StatusCode, Option<String>, Value) {
        let mut b = Request::builder().method(Method::POST).uri("/api/auth/login").header(header::HOST, "localhost").header(header::CONTENT_TYPE, "application/json");
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        let req = b.body(Body::from(json!({"email": email, "password": password}).to_string())).unwrap();
        let (s, h, v) = self.send_from(peer, req).await;
        let cookie = h.get(header::SET_COOKIE).and_then(|c| c.to_str().ok()).map(|c| c.split(';').next().unwrap().to_string());
        (s, cookie, v)
    }

    /// A second application instance over the same database, as a second replica would be: its own caches, nothing shared but PostgreSQL.
    /// It does not own the database (`cleanup` of the first one drops it).
    pub async fn second_instance(&self) -> TestApp {
        let mut b = (*self.state.boot).clone();
        b.instance_id = "test-2".into();
        let state = server::startup::init_state(b).await.expect("the second instance starts");
        let router = server::api::router(state.clone()).await;
        TestApp { router, state, db_name: String::new(), cookie: Mutex::new(None) }
    }

    /// One sample of `/metrics` as written without the `instance` label (`infra_auth_logins_total{outcome="failed"}`); 0 while there is none.
    pub async fn metric(&self, sample: &str) -> f64 {
        let (_, _, v) = self.request(Method::GET, "/metrics", None, None, &[]).await;
        let text = v.as_str().unwrap_or_default().to_string();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let Some((name, value)) = line.rsplit_once(' ') else { continue };
            let mut bare = name.to_string();
            if let Some(start) = bare.find("instance=\"") {
                let end = start + "instance=\"".len() + bare[start + "instance=\"".len()..].find('"').unwrap() + 1;
                bare.replace_range(start..end, "");
                bare = bare.replace("{,", "{").replace(",,", ",").replace(",}", "}").replace("{}", "");
            }
            if bare == sample {
                return value.parse().unwrap_or(0.0);
            }
        }
        0.0
    }

    pub async fn create_user(&self, admin_cookie: &str, email: &str, role: &str) -> String {
        let (s, v) = self.post(admin_cookie, "/api/admin/users", json!({"email": email, "role": role, "password": "another long password 1"})).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let (s, c, v) = self.login_as(email, "another long password 1").await;
        assert_eq!(s, StatusCode::OK, "{v}");
        c.unwrap()
    }

    pub async fn first_project(&self, cookie: &str) -> String {
        let (_, v) = self.get(cookie, "/api/projects").await;
        v["projects"][0]["id"].as_str().expect("demo project").to_string()
    }

    pub async fn cleanup(self) {
        self.state.pool.close().await;
        if self.db_name.is_empty() {
            return; // a second instance over the database of another TestApp: that one drops it
        }
        if let Ok(admin) = sqlx::PgPool::connect(&admin_url()).await {
            let _ = sqlx::query(&format!("drop database if exists {} with (force)", self.db_name)).execute(&admin).await;
        }
    }
}
