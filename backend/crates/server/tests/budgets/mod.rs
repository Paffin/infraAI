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
use tokio::net::TcpListener;

/// A listener on 127.0.0.1 that counts the requests it gets and answers each from `answer(path)`; `None` = answer for ever, chunked, without a length.
struct Hostile {
    port: u16,
    hits: Arc<AtomicUsize>,
}

impl Hostile {
    async fn listen(answer: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Hostile {
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

    let refused: [(&str, Value); 9] = [
        ("llm.baseUrl", json!("file:///etc")),
        ("llm.baseUrl", json!("http://169.254.169.254@evil.example")),
        ("llm.baseUrl", json!("not a url")),
        ("ui.helpUrl", json!("javascript:alert(document.domain)")),
        ("app.publicUrl", json!("https://architect.example.com/app")),
        ("llm.appReferer", json!("https://x.example\r\nX-Evil: 1")),
        ("llm.appTitle", json!("a\nb")),
        ("versions.sourceBases", json!(["https://endoflife.date/api/", "ftp://mirror.internal/"])),
        ("app.name", json!("x\u{0}y")),
    ];
    for (key, bad) in refused {
        let before = setting(&app, &c, key).await;
        let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {key: bad}})).await;
        assert_eq!((s, v["error"]["code"].as_str(), v["error"]["details"]["key"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_setting"), Some(key)), "{key} = {bad}: {v}");
        assert!(v["error"]["details"]["reason"].as_str().is_some_and(|r| !r.is_empty()), "the reason is said: {v}");
        assert_eq!(setting(&app, &c, key).await, before, "{key}: a refused value is not stored");
    }

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
