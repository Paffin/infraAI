//! Test harness: a fresh PostgreSQL database per test, the real router, a mock OpenRouter.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use server::config::BootConfig;
use server::state::AppState;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
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
        trust_proxy: false,
        shutdown_grace_secs: 1,
        log_json: false,
    }
}

impl TestApp {
    pub async fn new() -> TestApp {
        let db_name = format!("t_{}", uuid::Uuid::new_v4().simple());
        let admin = sqlx::PgPool::connect(&admin_url()).await.expect("test postgres must be reachable (see docs/DEVELOPMENT.md)");
        sqlx::query(&format!("create database {db_name}")).execute(&admin).await.unwrap();
        let base = admin_url();
        let url = base.rsplit_once('/').map(|(a, _)| format!("{a}/{db_name}")).unwrap();
        let state = server::startup::init_state(boot(url)).await.expect("init");
        let router = server::api::router(state.clone()).await;
        TestApp { router, state, db_name, cookie: Mutex::new(None) }
    }

    pub async fn request(&self, method: Method, path: &str, body: Option<Value>, cookie: Option<String>, extra: &[(&str, &str)]) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut b = Request::builder().method(method).uri(path).header(header::HOST, "localhost");
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, c);
        }
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        let req = if let Some(v) = body {
            b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())).unwrap()
        } else {
            b.body(Body::empty()).unwrap()
        };
        let mut req = req;
        req.extensions_mut().insert(ConnectInfo("127.0.0.1:50000".parse::<SocketAddr>().unwrap()));
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
        if let Ok(admin) = sqlx::PgPool::connect(&admin_url()).await {
            let _ = sqlx::query(&format!("drop database if exists {} with (force)", self.db_name)).execute(&admin).await;
        }
    }
}

/// A fake OpenRouter: records requests, answers with queued completions.
#[derive(Clone)]
pub struct MockLlm {
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub answers: Arc<Mutex<VecDeque<Value>>>,
    pub url: String,
}

pub fn completion(content: &str) -> Value {
    json!({"id": "gen-1", "model": "mock/model", "choices": [{"message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
           "usage": {"prompt_tokens": 100, "completion_tokens": 50, "cost": 0.0012}})
}

pub async fn start_mock_llm() -> MockLlm {
    let requests = Arc::new(Mutex::new(vec![]));
    let answers: Arc<Mutex<VecDeque<Value>>> = Arc::new(Mutex::new(VecDeque::new()));
    let (r2, a2) = (requests.clone(), answers.clone());
    let app = Router::new()
        .route(
            "/chat/completions",
            post(move |Json(body): Json<Value>| {
                let (r, a) = (r2.clone(), a2.clone());
                async move {
                    r.lock().unwrap().push(body);
                    let next = a.lock().unwrap().pop_front();
                    match next {
                        Some(v) => (StatusCode::OK, Json(v)),
                        None => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": {"message": "mock exhausted"}}))),
                    }
                }
            }),
        )
        .route("/models", get(|| async { Json(json!({"data": [
            {"id": "mock/model", "name": "Mock", "context_length": 100000, "architecture": {"output_modalities": ["text"]}, "pricing": {"prompt": "0.000001", "completion": "0.000002"}, "supported_parameters": ["structured_outputs", "response_format", "temperature"]},
            {"id": "mock/plain", "name": "Plain", "context_length": 8000, "architecture": {"output_modalities": ["text"]}, "pricing": {"prompt": "0", "completion": "0"}, "supported_parameters": ["temperature"]}
        ]})) }))
        .route("/key", get(|| async { Json(json!({"data": {"label": "sk-or-mock", "limit": 10, "limit_remaining": 9.5, "usage": 0.5}})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    MockLlm { requests, answers, url: format!("http://{addr}") }
}

impl MockLlm {
    pub fn push(&self, content: &str) {
        self.answers.lock().unwrap().push_back(completion(content));
    }
}
