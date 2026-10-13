//! Live tests: the real router, a real PostgreSQL and the **real OpenRouter** – no doubles.
//!
//! They cost a few cents and take minutes, so they are `#[ignore]`d by default:
//!
//! ```text
//! OPENROUTER_API_KEY=sk-or-… cargo test -p server --test live -- --ignored --nocapture
//! ```
//!
//! `LIVE_LLM_MODEL` (default `anthropic/claude-haiku-5.5`) picks the model. Behind a proxy that injects the
//! OpenRouter credentials the key may be omitted. With `LIVE_LLM_RECORD=<dir>` the raw model answers are
//! written there – that is how the fixtures of the pure pipeline tests (`tests/fixtures/llm`) were captured.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn model() -> String {
    std::env::var("LIVE_LLM_MODEL").unwrap_or_else(|_| "anthropic/claude-haiku-5.5".into())
}

fn api_key() -> String {
    std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| "sk-or-v1-supplied-by-the-proxy".into())
}

async fn configure(app: &TestApp, c: &str) {
    let m = model();
    let (s, v) = app
        .put(
            c,
            "/api/admin/settings",
            json!({"changes": {"llm.apiKey": api_key(), "llm.model.planner": m, "llm.model.fast": m, "llm.model.fallbacks": [], "llm.retries": 1, "llm.timeoutSeconds": 240, "llm.logPrompts": true}}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

async fn run_job(app: &TestApp, c: &str, pid: &str, kind: &str, prompt: &str) -> Value {
    let (s, j) = app.post(c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": kind, "prompt": prompt, "lang": "en"})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let id = j["jobId"].as_str().unwrap().to_string();
    let started = Instant::now();
    loop {
        let (_, job) = app.get(c, &format!("/api/ai/jobs/{id}")).await;
        if matches!(job["status"].as_str(), Some("succeeded") | Some("failed") | Some("cancelled")) {
            return job;
        }
        assert!(started.elapsed() < Duration::from_secs(420), "job did not finish in time: {job}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Keep the raw answers of a run (for the fixtures of the pure pipeline tests).
fn record(name: &str, kind: &str, prompt: &str, job: &Value) {
    let Ok(dir) = std::env::var("LIVE_LLM_RECORD") else { return };
    let answers: Vec<&str> = job["steps"].as_array().unwrap().iter().filter(|s| s["code"] == "model_answer").filter_map(|s| s["detail"]["text"].as_str()).collect();
    let doc = json!({"kind": kind, "model": model(), "capturedAt": chrono::Utc::now().to_rfc3339(), "prompt": prompt, "answers": answers});
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(format!("{dir}/{name}.json"), serde_json::to_string_pretty(&doc).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "calls the real OpenRouter (see the module docs)"]
async fn live_connection_test_and_model_catalog() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    configure(&app, &c).await;
    let (s, v) = app.post(&c, "/api/admin/llm/test", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["models"]["planner"]["found"], true, "the configured model is in OpenRouter's catalog: {v}");
    let (_, m) = app.get(&c, "/api/admin/llm/models").await;
    assert!(m["models"].as_array().unwrap().len() > 50, "the real catalog is large");
    assert!(m["models"].as_array().unwrap().iter().any(|x| x["id"] == model().as_str()));
    app.cleanup().await;
}

#[tokio::test]
#[ignore = "calls the real OpenRouter (see the module docs)"]
async fn live_chat_answers_and_the_call_is_accounted() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    configure(&app, &c).await;
    let pid = app.first_project(&c).await;
    let prompt = "In one or two sentences: which finding hurts the resilience score most and why?";
    let job = run_job(&app, &c, &pid, "chat", prompt).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    let answer = job["result"]["answer"].as_str().unwrap();
    assert!(answer.len() > 20, "{answer}");
    record("chat", "chat", prompt, &job);

    // the call is in the usage ledger with tokens, latency and (OpenRouter-reported) cost
    let (calls,): (i64,) =
        sqlx::query_as("select count(*) from llm_calls where status = 'ok' and purpose = 'chat' and prompt_tokens > 0 and completion_tokens > 0").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(calls, 1);
    let (_, u) = app.get(&c, "/api/admin/llm/usage?days=1").await;
    assert!(u["monthSpendUsd"].as_f64().unwrap() > 0.0, "{u}");
    // the exchange is in the chat history
    let (_, chat) = app.get(&c, &format!("/api/projects/{pid}/chat")).await;
    assert!(chat["messages"].as_array().unwrap().iter().any(|m| m["role"] == "assistant" && m["content"] == answer));
    app.cleanup().await;
}

#[tokio::test]
#[ignore = "calls the real OpenRouter (see the module docs)"]
async fn live_patch_is_previewed_applied_and_versioned() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    configure(&app, &c).await;
    let pid = app.first_project(&c).await;
    let prompt = "Raise the number of application instances (node app-dc1) to 40.";
    let job = run_job(&app, &c, &pid, "patch", prompt).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    record("patch", "patch", prompt, &job);
    let codes: Vec<&str> = job["steps"].as_array().unwrap().iter().filter_map(|s| s["code"].as_str()).collect();
    assert!(codes.contains(&"calling_model") && codes.contains(&"evaluating"), "{codes:?}");
    let proposal = job["result"]["proposalId"].as_str().expect("the model proposed operations").to_string();

    let (s, pr) = app.get(&c, &format!("/api/proposals/{proposal}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((pr["status"].as_str(), pr["mode"].as_str()), (Some("pending"), Some("ops")));
    assert!(pr["preview"]["after"]["score"].is_number() && pr["preview"]["before"]["score"].is_number(), "before/after scores are computed by the engine: {}", pr["preview"]);
    let (_, before) = app.get(&c, &format!("/api/projects/{pid}")).await;
    let app_dc1 = |v: &Value| v["architecture"]["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "app-dc1").unwrap()["props"]["replicas"].clone();
    assert_ne!(app_dc1(&before), json!(40), "nothing changes before the user applies");

    let (s, v) = app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({"expectedRev": before["rev"]})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, after) = app.get(&c, &format!("/api/projects/{pid}")).await;
    assert_eq!(app_dc1(&after), json!(40), "the change the model proposed is the change that was applied");
    let (_, vers) = app.get(&c, &format!("/api/projects/{pid}/versions")).await;
    assert!(vers["versions"].as_array().unwrap().iter().any(|x| x["message"].as_str().unwrap_or("").starts_with("AI:")), "a rollback version is kept");
    let (s, _) = app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "a proposal is applied once");
    app.cleanup().await;
}

#[tokio::test]
#[ignore = "calls the real OpenRouter (see the module docs)"]
async fn live_generate_produces_a_valid_architecture_the_engine_has_checked() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    configure(&app, &c).await;
    let (s, p) = app.post(&c, "/api/projects", json!({"name": "generated"})).await;
    assert_eq!(s, StatusCode::OK);
    let pid = p["id"].as_str().unwrap().to_string();
    let prompt = "A small web shop in two data centers: a stateless app (4 instances per site) behind a load balancer, PostgreSQL with a replica in the second site, Redis cache. 300 rps, RTO 15 minutes, RPO 5 minutes.";
    let job = run_job(&app, &c, &pid, "generate", prompt).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    record("generate", "generate", prompt, &job);
    let proposal = job["result"]["proposalId"].as_str().unwrap();
    let (_, pr) = app.get(&c, &format!("/api/proposals/{proposal}")).await;
    assert_eq!(pr["mode"], "replace");
    let nodes = pr["architecture"]["nodes"].as_array().unwrap();
    assert!(nodes.len() >= 5, "{} nodes", nodes.len());
    assert!(nodes.iter().all(|n| n["proposed"] == true));
    assert!(pr["architecture"]["zones"].as_array().unwrap().len() >= 2);
    // only catalog kinds, and the engine evaluated the result
    let (_, boot) = app.get(&c, "/api/bootstrap").await;
    let kinds: Vec<&str> = boot["catalog"]["kinds"].as_array().unwrap().iter().map(|k| k["id"].as_str().unwrap()).collect();
    assert!(nodes.iter().all(|n| kinds.contains(&n["kind"].as_str().unwrap())));
    assert!(pr["preview"]["after"]["score"].is_number());
    // props the model filled in survive (constrained decoding must not empty the free-form maps)
    assert!(nodes.iter().filter(|n| n["props"].as_object().map(|o| !o.is_empty()).unwrap_or(false)).count() * 2 >= nodes.len(), "most nodes carry properties");
    app.cleanup().await;
}

/// The release tables refreshed from the real endoflife.date (no key needed, only a network):
/// `cargo test -p server --test live -- --ignored release_tables`
#[tokio::test]
#[ignore]
async fn release_tables_refresh_from_the_real_source() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let (s, v) = app.post(&c, "/api/admin/versions/sync", json!({"ids": ["postgresql", "kafka", "ubuntu"]})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["failed"], 0, "{v}");
    assert_eq!(v["ok"], 3);
    let (_, list) = app.get(&c, "/api/admin/config/catalog.versions").await;
    let pg = list["items"].as_array().unwrap().iter().find(|i| i["id"] == "postgresql").unwrap()["data"].clone();
    assert_eq!(pg["asOf"], server::versions::today_utc());
    assert!(pg["cycles"].as_array().unwrap().len() >= 5, "{pg}");
    // the whole thing, as the periodic refresh does it
    let (s, v) = app.post(&c, "/api/admin/versions/sync", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["ok"].as_i64().unwrap() >= 100, "{v}");
    let (_, st) = app.get(&c, "/api/admin/versions/status").await;
    assert_eq!(st["last"]["by"], "manual");
    assert!(st["stale"].as_array().unwrap().is_empty(), "{st}");
    app.cleanup().await;
}
