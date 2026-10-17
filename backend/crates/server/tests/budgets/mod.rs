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
