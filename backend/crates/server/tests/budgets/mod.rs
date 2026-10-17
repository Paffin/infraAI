//! Budgets: what the server will read from other hosts, accept from a client and do at once. Included by `api.rs` (`mod budgets;`): a directory with a
//! `mod.rs` is not a test binary of its own, so these share the one that is already built.
//!
//! The sockets in this file misbehave on purpose (a source that redirects somewhere it must not, a provider that answers for ever): no real service can
//! be asked to, and what the server does then is the thing under test. Everything else is the real router over a real PostgreSQL.

use crate::common::*;
use crate::{set, wait_job};
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A listener on 127.0.0.1 that counts the requests it gets and answers each from `answer(path)`; `None` = answer for ever, chunked, without a length.
struct Hostile {
    port: u16,
    hits: Arc<AtomicUsize>,
}

impl Hostile {
    async fn listen(answer: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Hostile {
        Hostile::listen_after(Duration::ZERO, answer).await
    }

    /// The same, but each answer is given `delay` after the request has been read: a source that is slow.
    async fn listen_after(delay: Duration, answer: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Hostile {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let (counted, answer) = (hits.clone(), Arc::new(answer));
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let (counted, answer) = (counted.clone(), answer.clone());
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut buf = [0u8; 2048];
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut buf).await {
                            Ok(n) if n > 0 => head.extend_from_slice(&buf[..n]),
                            _ => return,
                        }
                    }
                    counted.fetch_add(1, Ordering::SeqCst);
                    let path = String::from_utf8_lossy(&head).lines().next().and_then(|l| l.split_whitespace().nth(1).map(String::from)).unwrap_or_default();
                    tokio::time::sleep(delay).await;
                    match answer(&path) {
                        Some(text) => {
                            let _ = sock.write_all(text.as_bytes()).await;
                        }
                        None => {
                            if sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n").await.is_ok() {
                                let piece = [b'x'; 1024];
                                while sock.write_all(b"400\r\n").await.is_ok() && sock.write_all(&piece).await.is_ok() && sock.write_all(b"\r\n").await.is_ok() {}
                            }
                        }
                    }
                    let _ = sock.shutdown().await;
                });
            }
        });
        Hostile { port, hits }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

fn reply(status: &str, headers: &str, body: &str) -> Option<String> {
    Some(format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len()))
}

async fn setting(app: &TestApp, c: &str, key: &str) -> Value {
    let (_, v) = app.get(c, "/api/admin/settings").await;
    v["settings"].as_array().unwrap().iter().find(|s| s["key"] == key).unwrap_or_else(|| panic!("no setting {key}"))["value"].clone()
}

/// Every URL-like setting refuses what cannot work, with the key named and nothing stored; what can work is accepted (a local address, an internal mirror:
/// closed networks point these settings at private addresses, so an address is never refused for being one).
#[tokio::test]
async fn url_settings_are_validated() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;

    // the third column is the code of the refusal: the interface says it in the language of the person, and the English `reason` stays for scripts
    let refused: [(&str, Value, &str); 9] = [
        ("llm.baseUrl", json!("file:///etc"), "not_http"),
        ("llm.baseUrl", json!("http://169.254.169.254@evil.example"), "has_credentials"),
        ("llm.baseUrl", json!("not a url"), "not_http"),
        ("ui.helpUrl", json!("javascript:alert(document.domain)"), "not_http"),
        ("app.publicUrl", json!("https://architect.example.com/app"), "not_origin"),
        ("llm.appReferer", json!("https://x.example\r\nX-Evil: 1"), "not_single_line"),
        ("llm.appTitle", json!("a\nb"), "not_single_line"),
        ("versions.sourceBases", json!(["https://endoflife.date/api/", "ftp://mirror.internal/"]), "not_http"),
        ("app.name", json!("x\u{0}y"), "not_single_line"),
    ];
    for (key, bad, code) in refused {
        let before = setting(&app, &c, key).await;
        let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {key: bad}})).await;
        assert_eq!((s, v["error"]["code"].as_str(), v["error"]["details"]["key"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_setting"), Some(key)), "{key} = {bad}: {v}");
        assert!(v["error"]["details"]["reason"].as_str().is_some_and(|r| !r.is_empty()), "the reason is said: {v}");
        assert_eq!(v["error"]["details"]["reasonCode"], code, "{key} = {bad}: {v}");
        assert_eq!(setting(&app, &c, key).await, before, "{key}: a refused value is not stored");
    }
    let (_, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"versions.sourceBases": ["https://endoflife.date/api/", "ftp://mirror.internal/"]}})).await;
    assert_eq!(v["error"]["details"]["params"]["value"], "ftp://mirror.internal/", "the entry that is wrong, for the interface to name: {v}");
    let (_, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"auth.sessionTtlHours": 0}})).await;
    assert_eq!((v["error"]["details"]["reasonCode"].as_str(), v["error"]["details"]["params"]["min"].as_f64()), (Some("below_min"), Some(1.0)), "a bound carries the number: {v}");

    // one bad value in a set of changes keeps the good ones out too: the request is all or nothing
    let name = setting(&app, &c, "app.name").await;
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"app.name": "Half saved", "llm.baseUrl": "file:///etc"}})).await;
    assert_eq!((s, v["error"]["details"]["key"].as_str()), (StatusCode::BAD_REQUEST, Some("llm.baseUrl")), "{v}");
    assert_eq!(setting(&app, &c, "app.name").await, name);

    // the same values through a configuration import are refused before anything is written, all of them named
    let (s, v) = app.post(&c, "/api/admin/config-import", json!({"settings": {"ui.helpUrl": "javascript:alert(1)", "llm.baseUrl": "openrouter.ai/api/v1"}, "dryRun": true})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let problems = v["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 2, "{v}");
    assert!(problems.iter().any(|p| p.as_str().unwrap().starts_with("ui.helpUrl:")) && problems.iter().any(|p| p.as_str().unwrap().starts_with("llm.baseUrl:")), "{v}");

    // the controls: values that work are accepted, and are stored in the form the code relies on
    let accepted = json!({
        "llm.baseUrl": "http://127.0.0.1:9",
        "ui.helpUrl": "https://docs.example.com/guide?lang=ru#start",
        "app.publicUrl": "HTTPS://Architect.Example.com:443/",
        "llm.appReferer": "",
        "llm.appTitle": "Инфра Архитектор",
        "versions.sourceBases": ["http://127.0.0.1:9/api", "https://mirror.internal/eol/"],
    });
    set(&app, &c, accepted).await;
    assert_eq!(setting(&app, &c, "llm.baseUrl").await, "http://127.0.0.1:9");
    assert_eq!(setting(&app, &c, "app.publicUrl").await, "https://architect.example.com", "stored as a browser writes the Origin: lower case, no default port, no slash");
    assert_eq!(setting(&app, &c, "versions.sourceBases").await, json!(["http://127.0.0.1:9/api/", "https://mirror.internal/eol/"]), "a base is a directory");
    assert_eq!(setting(&app, &c, "llm.appReferer").await, "", "no Referer is a way to send none");
    app.cleanup().await;
}

/// The sync through the API: an allowed source that redirects to an address outside `versions.sourceBases` is not followed. The second listener stands for
/// an internal service the allowlist was written to keep the server away from.
#[tokio::test]
async fn the_release_table_sync_does_not_follow_a_redirect_out_of_the_bases() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let internal = Hostile::listen(|_| reply("200 OK", "Content-Type: application/json\r\n", "[]")).await;
    let to = internal.url("/internal/metadata");
    let rows = include_str!("../fixtures/endoflife/postgresql.json").to_string();
    let source = Hostile::listen(move |path| match path {
        "/api/redirecting.json" => reply("302 Found", &format!("Location: {to}\r\n"), ""),
        "/api/moved.json" => reply("301 Moved Permanently", "Location: /api/postgresql.json\r\n", ""),
        "/api/postgresql.json" => reply("200 OK", "Content-Type: application/json\r\n", &rows),
        _ => reply("404 Not Found", "", ""),
    })
    .await;
    set(&app, &c, json!({"versions.sourceBases": [source.url("/api/")]})).await;

    // a table of the administrator's own, whose source answers with a redirect to the internal service
    let (_, list) = app.get(&c, "/api/admin/config/catalog.versions").await;
    let template = list["items"].as_array().unwrap().iter().find(|i| i["id"] == "kafka").unwrap()["data"].clone();
    for (id, path) in [("trap", "/api/redirecting.json"), ("moved", "/api/moved.json")] {
        let mut t = template.clone();
        t["id"] = json!(id);
        t["source"] = json!(source.url(path));
        let (s, v) = app.post(&c, "/api/admin/config/catalog.versions", json!({"id": id, "data": t})).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }

    let (s, v) = app.post(&c, "/api/admin/versions/sync", json!({"ids": ["trap"]})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["failed"], 1, "{v}");
    assert!(v["results"][0]["error"].as_str().unwrap().contains("versions.sourceBases"), "{v}");
    assert_eq!(internal.hits(), 0, "the internal service was never asked");
    assert_eq!(source.hits(), 1);

    // the control: a redirect that stays under the base is followed, and the table is replaced by what it led to
    let (s, v) = app.post(&c, "/api/admin/versions/sync", json!({"ids": ["moved"]})).await;
    assert_eq!((s, v["failed"].as_i64(), v["results"][0]["ok"].as_bool()), (StatusCode::OK, Some(0), Some(true)), "{v}");
    assert_eq!(source.hits(), 3, "the redirect and the table");
    app.cleanup().await;
}

/// A model provider (or something in its place) that answers for ever: the call fails with its reason, at the cap, instead of filling the server's memory
/// until the request's timeout. `outbound.maxAnswerKb` is the cap and it takes effect on the next request.
#[tokio::test]
async fn an_endless_answer_from_the_model_provider_is_cut_off_at_the_cap() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let provider = Hostile::listen(|_| None).await;
    set(&app, &c, json!({"llm.apiKey": "sk-or-v1-test-key", "llm.retries": 0, "llm.baseUrl": provider.url(""), "outbound.maxAnswerKb": 64, "llm.timeoutSeconds": 60, "llm.catalogTimeoutSeconds": 60}))
        .await;

    let started = Instant::now();
    let (s, v) = app.get(&c, "/api/admin/llm/models?refresh=1").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::SERVICE_UNAVAILABLE, Some("llm_unreachable")), "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("too large"), "{v}");
    assert!(started.elapsed() < Duration::from_secs(5), "the cap stopped it, not the 60 s timeout: {:?}", started.elapsed());

    let pid = app.first_project(&c).await;
    let started = Instant::now();
    let (s, j) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "chat", "prompt": "how resilient is this?"})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let job = wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;
    assert_eq!((job["status"].as_str(), job["errorCode"].as_str()), (Some("failed"), Some("llm_error")), "{job}");
    assert!(job["error"].as_str().unwrap().contains("too large"), "{job}");
    assert!(started.elapsed() < Duration::from_secs(8), "{:?}", started.elapsed());
    app.cleanup().await;
}

/// The key check says "ok" only about an answer it has read. A 2xx answer past `outbound.maxAnswerKb` (or one that is not JSON) was once turned into `ok: true`
/// with no information about the key: a statement the check could not back.
#[tokio::test]
async fn an_unreadable_answer_to_the_key_check_is_not_a_good_key() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let provider = Hostile::listen(|_| None).await;
    set(&app, &c, json!({"llm.apiKey": "sk-or-v1-test-key", "llm.baseUrl": provider.url(""), "outbound.maxAnswerKb": 64, "llm.catalogTimeoutSeconds": 60})).await;
    let started = Instant::now();
    let (s, v) = app.post(&c, "/api/admin/llm/test", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["ok"], false, "an answer past the cap says nothing good about the key: {v}");
    assert!(v["key"].is_null(), "{v}");
    assert!(v["message"].as_str().unwrap().contains("too large"), "the reason is the one it failed for: {v}");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());

    // not JSON: the same
    let html = Hostile::listen(|_| reply("200 OK", "Content-Type: text/html\r\n", "<html>a captive portal</html>")).await;
    set(&app, &c, json!({"llm.baseUrl": html.url("")})).await;
    let (_, v) = app.post(&c, "/api/admin/llm/test", json!({})).await;
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["message"].as_str().unwrap().contains("not JSON"), "{v}");

    // an error status with a body nobody can read is not "the key was rejected" either
    let broken = Hostile::listen(|_| reply("502 Bad Gateway", "Content-Type: text/html\r\n", "<html>bad gateway</html>")).await;
    set(&app, &c, json!({"llm.baseUrl": broken.url("")})).await;
    let (_, v) = app.post(&c, "/api/admin/llm/test", json!({})).await;
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["message"].as_str().unwrap().contains("502"), "says what happened, not that the key is bad: {v}");

    // the control: a readable answer is a checked key
    let good = Hostile::listen(|path| match path {
        "/key" => reply("200 OK", "Content-Type: application/json\r\n", r#"{"data": {"label": "ci", "limit": null, "usage": 0.5, "is_free_tier": false}}"#),
        _ => reply("404 Not Found", "Content-Type: application/json\r\n", "{}"),
    })
    .await;
    set(&app, &c, json!({"llm.baseUrl": good.url("")})).await;
    let (_, v) = app.post(&c, "/api/admin/llm/test", json!({})).await;
    assert_eq!((v["ok"].as_bool(), v["key"]["label"].as_str()), (Some(true), Some("ci")), "{v}");
    app.cleanup().await;
}

// ------------------------------------------------------------------------------------------------------------------------------------------
// Input budgets: what a client may send, and what the list of projects carries back
// ------------------------------------------------------------------------------------------------------------------------------------------

fn too_long(reply: (StatusCode, Value), code: &str, field: &str, max: usize, actual: usize) {
    let (status, v) = reply;
    assert_eq!((status, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some(code)), "{field}: {v}");
    assert_eq!(v["error"]["details"]["field"], field, "{v}");
    assert_eq!(v["error"]["details"]["max"], max, "{field}: the bound is said: {v}");
    assert_eq!(v["error"]["details"]["actual"], actual, "{v}");
}

async fn project_count(app: &TestApp) -> i64 {
    sqlx::query_scalar("select count(*) from projects").fetch_one(&app.state.pool).await.unwrap()
}

/// Every text a person can write has a bound, in characters, from a setting that is read on every request: the project name, the description, a version
/// message, the reason for an accepted risk, the id of a finding, the name of a user, the request to the model. One character past the bound is refused
/// with the bound in `details.max` (the codes that already existed, `invalid_name` and `prompt_too_long`, keep their code); exactly the bound is accepted.
#[tokio::test]
async fn oversized_text_fields_are_rejected() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    // the floors of the three settings: a test with a bound of 20 characters is a test of the setting, not of the default
    set(&app, &c, json!({"limits.maxNameChars": 20, "limits.maxTextChars": 100, "limits.maxIdChars": 16})).await;
    let text = |n: usize| "x".repeat(n);
    let demo = app.first_project(&c).await; // before the projects below: the list begins with the newest
    let before = project_count(&app).await;

    // project name: 20 characters of two bytes each are accepted (it is characters that are counted), 21 are not, and neither is a blank one
    too_long(app.post(&c, "/api/projects", json!({"name": text(21)})).await, "invalid_name", "name", 20, 21);
    too_long(app.post(&c, "/api/projects", json!({"name": "   "})).await, "invalid_name", "name", 20, 0);
    // description: on create and on patch
    too_long(app.post(&c, "/api/projects", json!({"name": "ok", "description": text(101)})).await, "field_too_long", "description", 100, 101);
    assert_eq!(project_count(&app).await, before, "a refused create stored nothing");
    let (s, v) = app.post(&c, "/api/projects", json!({"name": "ж".repeat(20), "description": "ж".repeat(100)})).await;
    assert_eq!(s, StatusCode::OK, "the bound itself is accepted, counted in characters: {v}");
    let pid = v["id"].as_str().unwrap().to_string();
    let path = format!("/api/projects/{pid}");
    too_long(app.patch(&c, &path, json!({"name": text(21)})).await, "invalid_name", "name", 20, 21);
    too_long(app.patch(&c, &path, json!({"description": text(101)})).await, "field_too_long", "description", 100, 101);
    let (s, v) = app.patch(&c, &path, json!({"name": text(20), "description": text(100)})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, p) = app.get(&c, &path).await;
    assert_eq!((p["project"]["name"].as_str(), p["project"]["description"].as_str().map(str::len)), (Some(text(20).as_str()), Some(100)), "{p}");

    // the message of a version
    too_long(app.post(&c, &format!("{path}/versions"), json!({"message": text(101)})).await, "field_too_long", "message", 100, 101);
    let (s, v) = app.post(&c, &format!("{path}/versions"), json!({"message": text(100)})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // the reason for an accepted risk, and the fingerprint it names: refused before the engine is started to look for the finding
    let (_, d) = app.get(&c, &format!("/api/projects/{demo}")).await;
    let (s, rep) = app.post(&c, "/api/evaluate", json!({"architecture": d["architecture"], "projectId": demo})).await;
    assert_eq!(s, StatusCode::OK, "{rep}");
    let fp = rep["findings"].as_array().unwrap().iter().find(|f| f["ruleId"] == "DR-KFK-01").unwrap()["id"].as_str().unwrap().to_string();
    let waivers = format!("/api/projects/{demo}/waivers");
    too_long(app.post(&c, &waivers, json!({"fingerprint": "F".repeat(17), "reason": text(40)})).await, "field_too_long", "fingerprint", 16, 17);
    set(&app, &c, json!({"limits.maxIdChars": 100})).await;
    too_long(app.post(&c, &waivers, json!({"fingerprint": fp, "reason": text(101)})).await, "field_too_long", "reason", 100, 101);
    let (s, v) = app.post(&c, &waivers, json!({"fingerprint": fp, "reason": "short"})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("reason_too_short")), "the minimum is still there: {v}");
    let (s, v) = app.post(&c, &waivers, json!({"fingerprint": fp, "reason": text(100)})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // a user's name: on create and on patch
    let new_user = |name: String| json!({"email": "named@test.local", "role": "viewer", "password": "another long password 1", "name": name});
    too_long(app.post(&c, "/api/admin/users", new_user(text(21))).await, "field_too_long", "name", 20, 21);
    let (s, v) = app.post(&c, "/api/admin/users", new_user(text(20))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let uid = v["id"].as_str().unwrap().to_string();
    too_long(app.patch(&c, &format!("/api/admin/users/{uid}"), json!({"name": text(21)})).await, "field_too_long", "name", 20, 21);

    // the request to the model keeps its code and now says its bound; the id of the finding to fix is bounded like any other
    set(&app, &c, json!({"llm.apiKey": "sk-or-v1-test-key", "llm.retries": 0, "llm.baseUrl": "http://127.0.0.1:9", "llm.maxPromptChars": 100, "limits.maxIdChars": 16})).await;
    let jobs = format!("/api/projects/{demo}/ai/jobs");
    too_long(app.post(&c, &jobs, json!({"kind": "chat", "prompt": text(101)})).await, "prompt_too_long", "prompt", 100, 101);
    too_long(app.post(&c, &jobs, json!({"kind": "fix", "findingId": "F".repeat(17)})).await, "field_too_long", "findingId", 16, 17);
    let (s, j) = app.post(&c, &jobs, json!({"kind": "fix", "findingId": "F".repeat(16)})).await;
    assert_eq!(s, StatusCode::OK, "the bound itself is accepted: {j}");
    wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;

    // a bound is read on every request: raise it and the next request is judged by the new one, with no restart
    too_long(app.patch(&c, &path, json!({"description": text(150)})).await, "field_too_long", "description", 100, 150);
    set(&app, &c, json!({"limits.maxTextChars": 200})).await;
    let (s, v) = app.patch(&c, &path, json!({"description": text(150)})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.cleanup().await;
}

/// One project whose description weighs megabytes used to make the list that every user loads weigh megabytes (a 3 MB description: a 3 MB list). The
/// rows here are made in SQL, like the legacy ones the cap does not reach (the API refuses a description this long now).
#[tokio::test]
async fn list_stays_small_with_long_descriptions() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = crate::demo_architecture(&app, &c).await;
    let mut ids = vec![];
    for n in 0..3 {
        ids.push(crate::new_project(&app, &c, &format!("long {n}"), &arch).await);
    }
    sqlx::query("update projects set description = repeat('x', 3000000) where name like 'long %'").execute(&app.state.pool).await.unwrap();
    let (stored,): (i64,) = sqlx::query_as("select min(length(description))::bigint from projects where name like 'long %'").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(stored, 3_000_000, "the rows really hold 3 MB each");

    let (s, list) = app.get(&c, "/api/projects").await;
    assert_eq!(s, StatusCode::OK);
    let bytes = serde_json::to_string(&list).unwrap().len();
    assert!(bytes < 50_000, "the list of {} projects weighs {bytes} bytes", list["projects"].as_array().unwrap().len());
    let long: Vec<&Value> = list["projects"].as_array().unwrap().iter().filter(|p| p["name"].as_str().unwrap().starts_with("long ")).collect();
    assert_eq!(long.len(), 3);
    for p in &long {
        assert_eq!(p["description"].as_str().unwrap().len(), 280, "a card carries limits.listDescriptionChars characters");
    }
    // the whole of it is where it always was
    let (_, one) = app.get(&c, &format!("/api/projects/{}", ids[0])).await;
    assert_eq!(one["project"]["description"].as_str().unwrap().len(), 3_000_000);
    // the bound is a setting
    set(&app, &c, json!({"limits.listDescriptionChars": 10})).await;
    let (_, list) = app.get(&c, "/api/projects").await;
    assert!(list["projects"].as_array().unwrap().iter().filter(|p| p["name"].as_str().unwrap().starts_with("long ")).all(|p| p["description"].as_str().unwrap().len() == 10));
    // and a short description is whole, however the bound is set
    let (_, short) = app.post(&c, "/api/projects", json!({"name": "short", "description": "short one"})).await;
    let (_, list) = app.get(&c, "/api/projects").await;
    assert_eq!(list["projects"].as_array().unwrap().iter().find(|p| p["id"] == short["id"]).unwrap()["description"], "short one");
    app.cleanup().await;
}

/// The list of projects cuts a description with `left(description, n)`. That counts characters in a UTF8 database and bytes in `SQL_ASCII`, where a cut
/// inside a Cyrillic letter cannot be sent to the client at all (500 on `GET /api/projects` on a cluster made under the `C` locale). The requirement is
/// documented (docs/DEPLOYMENT.md) and an installation that does not meet it is told at start (`db::warn_unless_utf8`). Two real databases, one of each.
#[tokio::test]
async fn a_database_that_is_not_utf8_is_named() {
    let admin = sqlx::PgPool::connect(&admin_url()).await.expect("test postgres must be reachable");
    let (utf8, ascii) = (format!("t_{}", uuid::Uuid::new_v4().simple()), format!("t_{}", uuid::Uuid::new_v4().simple()));
    sqlx::query(&format!("create database {utf8} encoding 'UTF8' template template0")).execute(&admin).await.unwrap();
    sqlx::query(&format!("create database {ascii} encoding 'SQL_ASCII' template template0")).execute(&admin).await.unwrap();
    let connect = |name: &str| {
        let url = admin_url().rsplit_once('/').map(|(a, _)| format!("{a}/{name}")).unwrap();
        async move { sqlx::PgPool::connect(&url).await.unwrap() }
    };
    let (good, bad) = (connect(&utf8).await, connect(&ascii).await);

    assert_eq!(server::db::encoding(&good).await.unwrap(), "UTF8");
    assert_eq!(server::db::encoding(&bad).await.unwrap(), "SQL_ASCII");
    assert_eq!(server::db::warn_unless_utf8(&good).await, None, "nothing to say about a UTF8 database");
    assert_eq!(server::db::warn_unless_utf8(&bad).await.as_deref(), Some("SQL_ASCII"), "the other one is named");

    // why it matters: the same cut, in the same words, is a string in one and an error in the other
    let cut = "select left('привет', 3)";
    assert_eq!(sqlx::query_scalar::<_, String>(cut).fetch_one(&good).await.unwrap(), "при");
    let failed = sqlx::query_scalar::<_, String>(cut).fetch_one(&bad).await.expect_err("a cut inside a letter is not sent");
    assert!(failed.to_string().contains("invalid byte sequence"), "{failed}");

    good.close().await;
    bad.close().await;
    for name in [utf8, ascii] {
        sqlx::query(&format!("drop database {name} with (force)")).execute(&admin).await.unwrap();
    }
}

fn ids_of(page: &Value) -> Vec<String> {
    page["projects"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap().to_string()).collect()
}

/// A list that stops at 500 is a list in which the 501st project does not exist. Now every project is reachable: pages by `limit` and `cursor`, newest change
/// first, ties by id, and the page boundary neither repeats nor skips a row (11 of the rows below share one `updated_at` across it).
#[tokio::test]
async fn projects_list_pages_through_more_than_500() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    // the demo project plus 644 rows = 645; rows 495..=505 share one time, and the boundary of the first page of 500 falls inside that group
    sqlx::query(
        "insert into projects (name, owner_id, draft, head_version, created_at, updated_at)
         select 'bulk ' || g, (select id from users limit 1), '{\"nodes\": [], \"edges\": [], \"zones\": []}'::jsonb, 1, now(),
                (now() - interval '1 hour') - ((case when g between 495 and 505 then 500 else g end) || ' seconds')::interval
         from generate_series(1, 644) g",
    )
    .execute(&app.state.pool)
    .await
    .unwrap();
    let expected: Vec<String> = sqlx::query_scalar::<_, uuid::Uuid>("select id from projects where deleted_at is null order by updated_at desc, id desc")
        .fetch_all(&app.state.pool)
        .await
        .unwrap()
        .iter()
        .map(|u| u.to_string())
        .collect();
    assert_eq!(expected.len(), 645);

    // without parameters: what it always answered, 500, and now it says there is more
    let (s, first) = app.get(&c, "/api/projects").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(first["projects"].as_array().unwrap().len(), 500);
    let next = first["next"].as_str().expect("there is a next page").to_string();
    let thumbs = |page: &Value| page["projects"].as_array().unwrap().iter().filter(|p| !p["thumb"].is_null()).count();
    assert_eq!(thumbs(&first), 60, "the first page carries limits.listThumbnails thumbnails");
    let (s, second) = app.get(&c, &format!("/api/projects?cursor={next}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(second["projects"].as_array().unwrap().len(), 145);
    assert!(second["next"].is_null(), "the last page says so");
    assert_eq!(thumbs(&second), 0, "a later page is not 'the most recently changed': no thumbnails (the card loads its own)");
    let mut all = ids_of(&first);
    all.extend(ids_of(&second));
    assert_eq!(all, expected, "two pages are the 645 projects, once each, in the order of the list");

    // pages of 100: seven of them, the same sequence
    let mut got = vec![];
    let mut sizes = vec![];
    let mut cursor: Option<String> = None;
    loop {
        let path = match &cursor {
            Some(c) => format!("/api/projects?limit=100&cursor={c}"),
            None => "/api/projects?limit=100".to_string(),
        };
        let (s, page) = app.get(&c, &path).await;
        assert_eq!(s, StatusCode::OK, "{page}");
        sizes.push(page["projects"].as_array().unwrap().len());
        got.extend(ids_of(&page));
        match page["next"].as_str() {
            Some(n) => cursor = Some(n.to_string()),
            None => break,
        }
    }
    assert_eq!(sizes, [100, 100, 100, 100, 100, 100, 45]);
    assert_eq!(got, expected);

    // a limit is clamped, not refused
    let (_, one) = app.get(&c, "/api/projects?limit=0").await;
    assert_eq!(one["projects"].as_array().unwrap().len(), 1);
    let (_, one) = app.get(&c, "/api/projects?limit=-5").await;
    assert_eq!(one["projects"].as_array().unwrap().len(), 1);
    let (_, big) = app.get(&c, "/api/projects?limit=100000").await;
    assert_eq!(big["projects"].as_array().unwrap().len(), 500, "limits.listPageMax is the most");
    let (s, v) = app.get(&c, "/api/projects?limit=lots").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_query")), "{v}");
    // a cursor that was not given out
    for bad in ["garbage", "bm90LWEtY3Vyc29y", "MjAyNi0xMC0xNlQxMjowMDowMC4wMDAwMDBafG5vdC1hLXV1aWQ"] {
        let (s, v) = app.get(&c, &format!("/api/projects?cursor={bad}")).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_cursor")), "{bad}: {v}");
    }
    // the position survives the row it names being deleted meanwhile
    let last_of_first = ids_of(&first).last().unwrap().clone();
    sqlx::query("update projects set deleted_at = now() where id = $1::uuid").bind(&last_of_first).execute(&app.state.pool).await.unwrap();
    let (_, after) = app.get(&c, &format!("/api/projects?cursor={next}")).await;
    assert_eq!(ids_of(&after), ids_of(&second), "the next page is the same without the row the cursor stood on");

    // the size of a page is a setting
    set(&app, &c, json!({"limits.listPageMax": 100})).await;
    let (_, page) = app.get(&c, "/api/projects").await;
    assert_eq!(page["projects"].as_array().unwrap().len(), 100);
    assert!(page["next"].is_string());
    let (_, page) = app.get(&c, "/api/projects?limit=500").await;
    assert_eq!(page["projects"].as_array().unwrap().len(), 100);
    app.cleanup().await;
}

/// `/diff` takes two designs from the request: it used to take them of any size and compare them on the async runtime. Past the size limits it is `400
/// limits_exceeded` (naming the numbers); within them a snapshot with a kind the catalog does not know still compares (drift is the point of the endpoint).
#[tokio::test]
async fn diff_respects_size_limits() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let design = crate::demo_architecture(&app, &c).await;
    let (nodes, edges, zones) = (design["nodes"].as_array().unwrap().len(), design["edges"].as_array().unwrap().len(), design["zones"].as_array().unwrap().len());
    assert!(nodes > 10 && edges > 10 && zones > 1, "the demo is bigger than the floors of the limits: {nodes} {edges} {zones}");
    for (setting, floor) in [("limits.maxNodes", 10), ("limits.maxEdges", 10), ("limits.maxZones", 1)] {
        set(&app, &c, json!({setting: floor})).await;
        for side in ["from", "to"] {
            let body = if side == "from" { json!({"from": design, "to": {}}) } else { json!({"from": {}, "to": design}) };
            let (s, v) = app.post(&c, "/api/diff", body).await;
            assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("limits_exceeded")), "{setting} {side}: {v}");
        }
        let (s, v) = app.post(&c, "/api/evaluate", json!({"architecture": design})).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("limits_exceeded")), "the same check as /evaluate's: {v}");
        assert_eq!(v["error"]["details"]["nodes"], nodes, "{v}");
        set(&app, &c, json!({setting: match setting { "limits.maxNodes" => 500, "limits.maxEdges" => 2000, _ => 100 }})).await;
    }
    let (s, v) = app.post(&c, "/api/diff", json!({"from": design, "to": design})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // not validated further: an environment snapshot may hold what the catalog does not know
    let mut snapshot = design.clone();
    snapshot["nodes"][0]["kind"] = json!("a-kind-only-the-real-environment-has");
    let (s, d) = app.post(&c, "/api/diff", json!({"from": design, "to": snapshot})).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert!(d["nodes"]["changed"].as_array().unwrap().iter().any(|n| n["changes"].as_array().unwrap().iter().any(|x| x["path"] == "kind")), "{d}");
    app.cleanup().await;
}

/// One `/ops/apply` request takes `limits.maxOpsPerRequest` operations at most; the base of the operations is judged by the size limits as before.
#[tokio::test]
async fn ops_apply_takes_a_bounded_number_of_operations() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let design = crate::demo_architecture(&app, &c).await;
    let op = |n: i64| json!({"op": "update_node", "id": "app-dc1", "set": {"props.replicas": n}});
    set(&app, &c, json!({"limits.maxOpsPerRequest": 1})).await;
    let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": design, "ops": [op(3), op(4)]})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("ops_too_many")), "{v}");
    assert_eq!((v["error"]["details"]["max"].as_i64(), v["error"]["details"]["actual"].as_i64()), (Some(1), Some(2)), "{v}");
    let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": design, "ops": [op(3)]})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["architecture"]["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "app-dc1").unwrap()["props"]["replicas"], 3);
    let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": design, "ops": [op(3)], "preview": true})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    set(&app, &c, json!({"limits.maxOpsPerRequest": 10})).await;
    let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": design, "ops": [op(3), op(4)]})).await;
    assert_eq!(s, StatusCode::OK, "raised, the next request is judged by the new bound: {v}");
    app.cleanup().await;
}

// ------------------------------------------------------------------------------------------------------------------------------------------
// Runtime budgets: how much engine work one user (and one replica) may have running, how long a request may take, how a slow client is treated
// ------------------------------------------------------------------------------------------------------------------------------------------

use server::heavy::HEAVY_ROUTES;

async fn user_id_of(app: &TestApp, cookie: &str) -> uuid::Uuid {
    let (_, me) = app.get(cookie, "/api/auth/me").await;
    me["user"]["id"].as_str().unwrap().parse().unwrap()
}

/// What a request for each route of `HEAVY_ROUTES` looks like. A route in the list without a request here fails the test: the list and the test are one thing.
struct Heavy {
    project: String,
    design: Value,
    fingerprint: String,
}

impl Heavy {
    async fn new(app: &TestApp, c: &str) -> Heavy {
        let design = crate::demo_architecture(app, c).await;
        let project = crate::new_project(app, c, "heavy", &design).await;
        let (s, report) = app.post(c, "/api/evaluate", json!({"architecture": design})).await;
        assert_eq!(s, StatusCode::OK, "{report}");
        let fingerprint = report["findings"].as_array().unwrap().iter().find(|f| f["ruleId"] == "DR-KFK-01").unwrap()["id"].as_str().unwrap().to_string();
        Heavy { project, design, fingerprint }
    }

    /// The request for `route`, valid at this moment (a restore needs the revision as it is now, an applied proposal the draft it was made for).
    async fn request(&self, app: &TestApp, c: &str, method: &str, route: &str) -> (axum::http::Method, String, Option<Value>) {
        let (id, d) = (&self.project, &self.design);
        let (rev, _) = crate::project_rev(app, c, id).await;
        let post = axum::http::Method::POST;
        match (method, route) {
            ("POST", "/api/evaluate") | ("POST", "/api/recommendations") => (post, route.into(), Some(json!({"architecture": d}))),
            ("POST", "/api/simulate") => (post, route.into(), Some(json!({"architecture": d, "scenario": {"type": "zone", "zoneId": "dc1"}}))),
            ("POST", "/api/diff") => (post, route.into(), Some(json!({"from": d, "to": d}))),
            ("POST", "/api/ops/apply") => (post, route.into(), Some(json!({"architecture": d, "ops": [{"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 3}}], "preview": true}))),
            ("POST", "/api/placement/preview") => (post, route.into(), Some(json!({"pattern": "n-sites", "sites": 3}))),
            ("POST", "/api/placement/compare") => (post, route.into(), Some(json!({"base": {"pattern": "n-sites", "sites": 3}, "variants": [{"sites": 2}, {"sites": 3}]}))),
            ("GET", "/api/projects/{id}/export/{format}") => (axum::http::Method::GET, format!("/api/projects/{id}/export/terraform"), None),
            ("POST", "/api/projects") => (post, route.into(), Some(json!({"name": "created by the gate test", "architecture": d}))),
            ("POST", "/api/projects/{id}/versions") => (post, format!("/api/projects/{id}/versions"), Some(json!({"message": "by the gate test"}))),
            ("POST", "/api/projects/{id}/versions/{n}/restore") => (post, format!("/api/projects/{id}/versions/1/restore"), Some(json!({"expectedRev": rev}))),
            ("POST", "/api/projects/{id}/waivers") => {
                (post, format!("/api/projects/{id}/waivers"), Some(json!({"fingerprint": self.fingerprint, "reason": "accepted by the gate test, which has a reason"})))
            }
            ("POST", "/api/proposals/{id}/apply") => {
                let proposal = crate::pending_proposal(app, id, vec![crate::replicas_of("app-dc1", 4)]).await;
                (post, format!("/api/proposals/{proposal}/apply"), Some(json!({})))
            }
            _ => panic!("{method} {route} is a heavy route and this test has no request for it: add one"),
        }
    }
}

async fn send(app: &TestApp, c: &str, (method, path, body): (axum::http::Method, String, Option<Value>)) -> (StatusCode, axum::http::HeaderMap, Value) {
    app.request(method, &path, body, Some(c.to_string()), &[]).await
}

/// Engine work that one user has running is limited, and so is the replica's: past the limit a heavy request is `429 rate_limited` with `Retry-After`, before the
/// body is read and before the engine starts; when a slot is free the same request is answered. The slots are the real gate's (`app.state.heavy`), taken the way a
/// request takes one, so the test does not depend on how long an evaluation takes: it holds the slots instead of racing two requests.
#[tokio::test]
async fn heavy_endpoints_are_rate_limited() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let me = user_id_of(&app, &c).await;
    let heavy = Heavy::new(&app, &c).await;
    assert!(HEAVY_ROUTES.len() >= 9, "{} routes are gated; before there were 2 limits (sign-in, AI requests) on 59 routes", HEAVY_ROUTES.len());
    set(&app, &c, json!({"limits.heavyRetryAfterSeconds": 3})).await;
    let (per_user, gate) = (3, app.state.heavy.clone());

    // all of a user's slots are taken: every heavy route says so
    let taken: Vec<_> = (0..per_user).map(|_| gate.try_acquire(me, per_user, 0).expect("the slots are free")).collect();
    let refused_before = app.metric("infra_heavy_rejected_total{scope=\"user\"}").await;
    for (method, route) in HEAVY_ROUTES {
        let (s, h, v) = send(&app, &c, heavy.request(&app, &c, method, route).await).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("rate_limited")), "{method} {route}: {v}");
        assert_eq!(h.get("retry-after").and_then(|x| x.to_str().ok()), Some("3"), "{method} {route}: Retry-After is the setting");
        assert_eq!((v["error"]["details"]["scope"].as_str(), v["error"]["details"]["limit"].as_i64()), (Some("user"), Some(3)), "{v}");
    }
    assert_eq!(app.metric("infra_heavy_rejected_total{scope=\"user\"}").await - refused_before, HEAVY_ROUTES.len() as f64, "each refusal is counted, by the limit that said no");
    // a HEAD is answered by the handler of the GET, with its body thrown away: it must not be a way round
    let (s, _, _) = app.request(axum::http::Method::HEAD, &format!("/api/projects/{}/export/terraform", heavy.project), None, Some(c.clone()), &[]).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "HEAD of an export while the slots are taken");
    assert_eq!(gate.in_flight_of(me), per_user, "a refused request takes no slot");

    // not everything is gated: the list of projects is a query, not engine work; somebody else's slots are their own; a request with no session is still 401
    let (s, _) = app.get(&c, "/api/projects").await;
    assert_eq!(s, StatusCode::OK);
    let other = app.create_user(&c, "other@test.local", "editor").await;
    let (s, _, v) = send(&app, &other, heavy.request(&app, &c, "POST", "/api/evaluate").await).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _, v) = app.request(axum::http::Method::POST, "/api/evaluate", Some(json!({"architecture": heavy.design})), None, &[]).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("unauthorized")), "{v}");

    // one slot free: each of the same requests is let in, answered, and gives its slot back
    let mut taken = taken;
    taken.pop();
    for (method, route) in HEAVY_ROUTES {
        let (s, _, v) = send(&app, &c, heavy.request(&app, &c, method, route).await).await;
        assert_eq!(s, StatusCode::OK, "{method} {route} with a slot free: {v}");
        assert_eq!(gate.in_flight_of(me), per_user - 1, "{method} {route}: the slot of the request is given back when it is answered");
    }
    drop(taken);
    assert_eq!(gate.in_flight(), 0);
    assert_eq!(app.metric("infra_heavy_in_flight").await, 0.0);

    // the replica has a limit of its own, per processor, whoever's the requests are; the user's limit off
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    set(&app, &c, json!({"limits.heavyConcurrentPerUser": 0, "limits.heavyConcurrentPerCore": 1})).await;
    let strangers: Vec<_> = (0..cores).map(|_| gate.try_acquire(uuid::Uuid::new_v4(), 0, 0).unwrap()).collect();
    let (s, _, v) = send(&app, &c, heavy.request(&app, &c, "POST", "/api/evaluate").await).await;
    assert_eq!((s, v["error"]["details"]["scope"].as_str(), v["error"]["details"]["limit"].as_u64()), (StatusCode::TOO_MANY_REQUESTS, Some("replica"), Some(cores as u64)), "{v}");
    drop(strangers);
    let (s, _, _) = send(&app, &c, heavy.request(&app, &c, "POST", "/api/evaluate").await).await;
    assert_eq!(s, StatusCode::OK);
    // 0 is no limit at all
    set(&app, &c, json!({"limits.heavyConcurrentPerCore": 0})).await;
    let many: Vec<_> = (0..20).map(|_| gate.try_acquire(me, 0, 0).unwrap()).collect();
    let (s, _, _) = send(&app, &c, heavy.request(&app, &c, "POST", "/api/evaluate").await).await;
    assert_eq!(s, StatusCode::OK);
    drop(many);
    app.cleanup().await;
}

/// A deadline over the whole API, read on every request, answered in JSON. The request here is a real one that waits for a real lock: the test holds the row
/// of the project in a transaction of its own, so the update of the name has to wait for it.
#[tokio::test]
async fn a_request_that_takes_too_long_is_answered_in_json_after_the_deadline() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    set(&app, &c, json!({"limits.requestTimeoutSeconds": 1})).await;
    let mut lock = app.state.pool.begin().await.unwrap();
    sqlx::query("select 1 from projects where id = $1::uuid for update").bind(&pid).fetch_one(&mut *lock).await.unwrap();

    let started = Instant::now();
    let (s, h, v) = app.request(axum::http::Method::PATCH, &format!("/api/projects/{pid}"), Some(json!({"name": "waits for the lock"})), Some(c.clone()), &[]).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::GATEWAY_TIMEOUT, Some("request_timeout")), "{v}");
    assert!(h.get("content-type").and_then(|x| x.to_str().ok()).is_some_and(|t| t.starts_with("application/json")), "the one JSON shape: {h:?}");
    assert!(v["error"]["message"].as_str().unwrap().contains("limits.requestTimeoutSeconds"), "{v}");
    assert!(started.elapsed() >= Duration::from_millis(900) && started.elapsed() < Duration::from_secs(3), "after the deadline, not before or long after: {:?}", started.elapsed());
    assert_eq!(app.metric("infra_http_requests_total{method=\"PATCH\",route=\"/api/projects/{id}\",status=\"504\"}").await, 1.0, "the metrics see the 504");

    // the lock goes: the same request is answered at once, and the new value of the setting is what the next request is judged by
    lock.rollback().await.unwrap();
    let (s, _, v) = app.request(axum::http::Method::PATCH, &format!("/api/projects/{pid}"), Some(json!({"name": "no longer waits"})), Some(c.clone()), &[]).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // the routes that wait for another host are not cut by a deadline they cannot know: a source that takes two seconds to answer, under a deadline of one
    let (_, list) = app.get(&c, "/api/admin/config/catalog.versions").await;
    let mut table = list["items"].as_array().unwrap().iter().find(|i| i["id"] == "kafka").unwrap()["data"].clone();
    let rows = include_str!("../fixtures/endoflife/postgresql.json").to_string();
    let slow = Hostile::listen_after(Duration::from_secs(2), move |_| reply("200 OK", "Content-Type: application/json\r\n", &rows)).await;
    set(&app, &c, json!({"versions.sourceBases": [slow.url("/api/")]})).await;
    table["id"] = json!("slow");
    table["source"] = json!(slow.url("/api/slow.json"));
    let (s, v) = app.post(&c, "/api/admin/config/catalog.versions", json!({"id": "slow", "data": table})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let started = Instant::now();
    let (s, v) = app.post(&c, "/api/admin/versions/sync", json!({"ids": ["slow"]})).await;
    assert!(started.elapsed() > Duration::from_millis(1900), "the source was slow: {:?}", started.elapsed());
    assert_eq!((s, v["results"][0]["ok"].as_bool()), (StatusCode::OK, Some(true)), "the sync was not stopped at the deadline: {v}");

    // and 0 is no deadline
    set(&app, &c, json!({"limits.requestTimeoutSeconds": 0})).await;
    let mut lock = app.state.pool.begin().await.unwrap();
    sqlx::query("select 1 from projects where id = $1::uuid for update").bind(&pid).fetch_one(&mut *lock).await.unwrap();
    let waiting = {
        let (app, c, pid) = (&app, c.clone(), pid.clone());
        async move { app.request(axum::http::Method::PATCH, &format!("/api/projects/{pid}"), Some(json!({"name": "waits"})), Some(c), &[]).await }
    };
    let release = async {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        lock.rollback().await.unwrap();
    };
    let ((s, _, v), ()) = tokio::join!(waiting, release);
    assert_eq!(s, StatusCode::OK, "waited longer than the former deadline, as there is none now: {v}");
    app.cleanup().await;
}

/// `limits.maxBodyMb` was read once, when the router was built, although the settings promise a change without a restart.
#[tokio::test]
async fn the_body_limit_follows_the_setting_without_a_restart() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let body = format!("{{\"architecture\": \"{}\"}}", "x".repeat(1_500_000));
    set(&app, &c, json!({"limits.maxBodyMb": 1})).await;
    let (s, _, v) = app.raw(axum::http::Method::POST, "/api/evaluate", Some(&body), Some("application/json"), Some(&c)).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::PAYLOAD_TOO_LARGE, Some("payload_too_large")), "{v}");
    set(&app, &c, json!({"limits.maxBodyMb": 2})).await;
    let (s, _, v) = app.raw(axum::http::Method::POST, "/api/evaluate", Some(&body), Some("application/json"), Some(&c)).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_architecture")), "past the size check, refused for what it is: {v}");
    app.cleanup().await;
}

/// A client that sends half a header and stops. Serving is `server::serve::run`, the loop `main.rs` uses, on a real listener; the router is the application's.
#[tokio::test]
async fn slow_header_connection_is_closed() {
    use server::serve::{run, ServeOptions};
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    set(&app, &c, json!({"limits.headerReadTimeoutSeconds": 2})).await;
    let options = ServeOptions::from_settings(&app.state);
    assert_eq!(options.header_read_timeout().await, Some(Duration::from_secs(2)), "the setting is what the listener is given");
    set(&app, &c, json!({"limits.headerReadTimeoutSeconds": 0})).await;
    assert_eq!(options.header_read_timeout().await, None, "0 is no timeout, and the same options hear the change: it is read for every connection, not once");
    set(&app, &c, json!({"limits.headerReadTimeoutSeconds": 2})).await;

    let half = b"GET /api/public/config HTTP/1.1\r\nHost: localhost\r\nX-A: b";
    let start = |options: ServeOptions| {
        let router = app.router.clone();
        async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let serving = tokio::spawn(run(listener, router, options, async move {
                let _ = stopped.await;
            }));
            (addr, stop, serving)
        }
    };

    // with the timeout: closed after it, while the same listener goes on answering everybody else
    let (addr, stop, serving) = start(options).await;
    let mut slow = TcpStream::connect(addr).await.unwrap();
    slow.write_all(half).await.unwrap();
    let started = Instant::now();
    let http = reqwest::Client::new();
    let ok = http.get(format!("http://{addr}/api/public/config")).send().await.unwrap();
    assert_eq!(ok.status(), 200, "a normal request on the same listener, while the slow one is open");
    let login = http.post(format!("http://{addr}/api/auth/login")).json(&json!({"email": "admin@test.local", "password": "wrong password!!"})).send().await.unwrap();
    assert_eq!(login.status(), 401, "the handlers that read the peer address (ConnectInfo) get it from this loop");
    let (counted,): (i64,) = sqlx::query_as("select count(*) from rate_limits where key = 'login:ip:127.0.0.1'").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(counted, 1, "and it is the address of the peer");
    let mut buf = [0u8; 64];

    // a change in the admin panel is in force for the next client, on the listener that is already running; the connection that is already here keeps the limit it came with
    set(&app, &c, json!({"limits.headerReadTimeoutSeconds": 1})).await;
    let mut newer = TcpStream::connect(addr).await.unwrap();
    newer.write_all(half).await.unwrap();
    let newer_started = Instant::now();
    let closed = tokio::time::timeout(Duration::from_secs(6), newer.read(&mut buf)).await.expect("the newer connection was closed");
    assert!(matches!(closed, Ok(0) | Err(_)), "closed without an answer: {closed:?}");
    let waited = newer_started.elapsed();
    assert!(waited >= Duration::from_millis(800) && waited < Duration::from_millis(1700), "closed by the 1 s the setting says now, not by the 2 s the listener started with: {waited:?}");
    let closed = tokio::time::timeout(Duration::from_secs(6), slow.read(&mut buf)).await.expect("the server closed the connection");
    assert!(matches!(closed, Ok(0) | Err(_)), "closed without an answer: {closed:?}");
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(1800) && waited < Duration::from_secs(3), "closed by the 2 s timeout, not before and not long after: {waited:?}");
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), serving).await.expect("the loop ends when told to").unwrap().unwrap();

    // a connection kept open between two requests is closed by the same timeout when nothing comes: it is the time to the next request's headers, as hyper counts it
    let (addr, stop, serving) = start(ServeOptions::fixed(Some(Duration::from_secs(1)))).await;
    let mut kept = TcpStream::connect(addr).await.unwrap();
    kept.write_all(b"GET /api/public/config HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    let mut answer = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(3), kept.read(&mut answer)).await.expect("an answer").unwrap();
    assert!(String::from_utf8_lossy(&answer[..n]).starts_with("HTTP/1.1 200"), "{}", String::from_utf8_lossy(&answer[..n]));
    let idle = Instant::now();
    let closed = tokio::time::timeout(Duration::from_secs(5), kept.read(&mut answer)).await.expect("closed once idle for the timeout");
    assert!(matches!(closed, Ok(0) | Err(_)), "{closed:?}");
    assert!(idle.elapsed() < Duration::from_secs(3), "{:?}", idle.elapsed());
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), serving).await.expect("the loop ends when told to").unwrap().unwrap();

    // the control: without a timeout the same half-open connection is still open when the other one was closed long ago
    let (addr, stop, serving) = start(ServeOptions::fixed(None)).await;
    let mut open = TcpStream::connect(addr).await.unwrap();
    open.write_all(half).await.unwrap();
    let still = tokio::time::timeout(Duration::from_millis(3500), open.read(&mut buf)).await;
    assert!(still.is_err(), "nothing closed it in 3.5 s: {still:?}");
    drop(open);
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), serving).await.expect("the loop ends when told to").unwrap().unwrap();
    app.cleanup().await;
}

/// The list of gated routes is in the code once (`HEAVY_ROUTES`), and the reader of the API finds it in docs/API.md: a route added to the one and not the other fails here.
#[tokio::test]
async fn the_gated_routes_are_documented() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/API.md")).expect("docs/API.md");
    let section = text.split("### Heavy requests").nth(1).and_then(|t| t.split("\n### ").next().or(Some(t))).expect("the section on heavy requests").split("\n`scenario` is one of").next().unwrap();
    for (method, route) in HEAVY_ROUTES {
        let short = route.strip_prefix("/api").unwrap();
        let listed = section.lines().filter(|l| l.starts_with(&format!("| {method} |"))).any(|l| l.contains(&format!("`{short}`")));
        assert!(listed, "{method} {short} is gated and docs/API.md (Heavy requests) does not list it");
    }
    let listed_routes = section.lines().filter(|l| l.starts_with("| POST |") || l.starts_with("| GET |")).map(|l| l.matches('`').count() / 2).sum::<usize>();
    assert_eq!(listed_routes, HEAVY_ROUTES.len(), "and it lists nothing that is not gated");
}

/// What happens to a burst of one user's heavy requests that is bigger than the gate: the first ones are admitted and finish, the rest are sent away at once with
/// `Retry-After` - not queued behind them (the pool and the processors would be held by what nobody is waiting for any more) and not failed with a 5xx.
#[tokio::test]
async fn a_burst_beyond_the_gate_is_shed_and_not_queued() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = crate::demo_architecture(&app, &c).await;
    let mut jobs = vec![];
    for i in 0..12 {
        let pid = crate::new_project(&app, &c, &format!("shed {i}"), &arch).await;
        let proposal = crate::pending_proposal(&app, &pid, vec![crate::replicas_of("app-dc1", 40 + i)]).await;
        jobs.push((pid, proposal));
    }
    let paths: Vec<String> = jobs.iter().map(|(_, p)| format!("/api/proposals/{p}/apply")).collect();
    let answers = futures::future::join_all(paths.iter().map(|path| app.request(axum::http::Method::POST, path, Some(json!({})), Some(c.clone()), &[]))).await;
    let statuses: Vec<u16> = answers.iter().map(|(s, _, _)| s.as_u16()).collect();
    assert!(statuses.iter().all(|s| matches!(s, 200 | 429)), "admitted or sent away, nothing else: {statuses:?}");
    let admitted = statuses.iter().filter(|s| **s == 200).count();
    assert!((3..12).contains(&admitted), "the user's three slots are used and the rest is shed: {statuses:?}");
    for (s, h, v) in answers.iter().filter(|(s, _, _)| *s == StatusCode::TOO_MANY_REQUESTS) {
        assert!(h.get("retry-after").is_some_and(|x| x.to_str().unwrap().parse::<u64>().is_ok()), "{s}: {v}");
    }
    // what was sent away is untouched and can be sent again; what was admitted is applied exactly once
    for ((pid, proposal), (s, _, _)) in jobs.iter().zip(&answers) {
        let (_, now) = crate::project_rev(&app, &c, pid).await;
        let applied = crate::node_field(&now, "app-dc1", "props")["replicas"] != arch["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "app-dc1").unwrap()["props"]["replicas"];
        assert_eq!(applied, *s == StatusCode::OK, "{pid}");
        if *s == StatusCode::TOO_MANY_REQUESTS {
            let (s, v) = app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({})).await;
            assert_eq!(s, StatusCode::OK, "sent again, it is let in: {v}");
        }
    }
    assert_eq!(app.state.heavy.in_flight(), 0);
    app.cleanup().await;
}
