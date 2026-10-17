mod budgets;
mod common;

use axum::http::{header, Method, StatusCode};
use common::*;
use serde_json::{json, Value};

#[tokio::test]
async fn health_ready_and_auth_required() {
    let app = TestApp::new().await;
    let (s, _, _) = app.request(Method::GET, "/healthz", None, None, &[]).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = app.request(Method::GET, "/readyz", None, None, &[]).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, v) = app.request(Method::GET, "/api/bootstrap", None, None, &[]).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    let (s, _, v) = app.request(Method::GET, "/api/nope", None, None, &[]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(v["error"]["code"], "not_found");
    let (s, _, v) = app.request(Method::GET, "/api/public/config", None, None, &[]).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["defaultLanguage"], "ru");
    app.cleanup().await;
}

#[tokio::test]
async fn login_session_and_lockout() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let (s, me) = app.get(&c, "/api/auth/me").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["user"]["role"], "admin");
    assert!(me["user"].get("passwordHash").is_none() && me["user"].get("password_hash").is_none());

    // forged session is rejected
    let (s, _) = app.get("ia_session=abc.def", "/api/auth/me").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // wrong password x5 locks the account; the right one is then refused as well
    for _ in 0..5 {
        let (s, _, _) = app.login_as("admin@test.local", "wrong password!!").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, _, v) = app.login_as("admin@test.local", "correct horse battery staple").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{v}");
    assert_eq!(v["error"]["code"], "locked");
    app.cleanup().await;
}

#[tokio::test]
async fn temporary_password_only_unlocks_the_password_change() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    let (s, v) = app.post(&admin, "/api/admin/users", json!({"email": "temp@test.local", "role": "editor"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let temp = v["temporaryPassword"].as_str().expect("generated password").to_string();
    let (s, cookie, v) = app.login_as("temp@test.local", &temp).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["user"]["mustChangePassword"], true);
    let cookie = cookie.unwrap();

    let (s, v) = app.get(&cookie, "/api/projects").await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["error"]["code"], "password_change_required");
    let (s, _) = app.get(&cookie, "/api/bootstrap").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.get(&cookie, "/api/auth/me").await;
    assert_eq!(s, StatusCode::OK);

    let (s, h, v) = app.request(Method::POST, "/api/auth/password", Some(json!({"current": temp, "new": "a brand new long password"})), Some(cookie), &[]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let fresh = h.get(header::SET_COOKIE).and_then(|c| c.to_str().ok()).map(|c| c.split(';').next().unwrap().to_string()).unwrap();
    let (s, _) = app.get(&fresh, "/api/projects").await;
    assert_eq!(s, StatusCode::OK);
    app.cleanup().await;
}

#[tokio::test]
async fn csrf_guard_rejects_foreign_origin() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let (s, _, v) = app.request(Method::POST, "/api/projects", Some(json!({"name": "x"})), Some(c.clone()), &[("origin", "https://evil.example")]).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, _, v) = app.request(Method::POST, "/api/projects", Some(json!({"name": "x"})), Some(c), &[("origin", "http://localhost")]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn bootstrap_contains_catalog_rules_templates_and_etag() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let (s, v) = app.get(&c, "/api/bootstrap").await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["catalog"]["kinds"].as_array().unwrap().len() >= 100, "the library covers the common products");
    assert!(v["catalog"]["categories"].as_array().unwrap().iter().all(|c| !c["functions"].as_array().unwrap().is_empty()), "every category declares its functions");
    assert!(v["rules"].as_array().unwrap().len() >= 50);
    assert_eq!(v["templates"].as_array().unwrap().len(), engine::defaults::templates().iter().filter(|t| t.enabled).count());
    assert!(engine::lifecycle::parse_date(v["today"].as_str().unwrap()).is_some(), "the editor needs today's date for support dates");
    assert_eq!(v["engine"]["lifecycle"]["matureDays"], 90);
    assert_eq!(v["ai"]["enabled"], false, "no key configured yet");
    // the "About" window reads the license and the repository from the crate manifest, the same place `cargo` and the lock-file check read them from
    assert_eq!(v["about"]["license"], "Apache-2.0");
    assert!(v["about"]["repository"].as_str().unwrap().starts_with("https://"), "the source is a link");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    // conditional GET
    let (_, h, _) = app.request(Method::GET, "/api/bootstrap", None, Some(c.clone()), &[]).await;
    let etag = h.get("etag").unwrap().to_str().unwrap().to_string();
    let (s, _, _) = app.request(Method::GET, "/api/bootstrap", None, Some(c), &[("if-none-match", &etag)]).await;
    assert_eq!(s, StatusCode::NOT_MODIFIED);
    app.cleanup().await;
}

#[tokio::test]
async fn demo_project_projects_versions_conflicts_and_diff() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let (s, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
    assert_eq!(s, StatusCode::OK);
    let rev = p["rev"].as_i64().unwrap();
    let mut arch = p["architecture"].clone();
    assert!(arch["nodes"].as_array().unwrap().len() > 15);

    // evaluate shows the demo's known defects
    let (s, rep) = app.post(&c, "/api/evaluate", json!({"architecture": arch, "projectId": pid})).await;
    assert_eq!(s, StatusCode::OK, "{rep}");
    assert_eq!(rep["counts"]["critical"], 0);
    assert!(rep["counts"]["risk"].as_i64().unwrap() >= 2);
    assert!(rep["score"]["value"].as_f64().unwrap() > 60.0);

    // save draft; second save with a stale rev conflicts
    arch["nodes"][2]["props"]["replicas"] = json!(40);
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": arch, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["rev"], rev + 1);
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": arch, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["details"]["currentRev"], rev + 1);

    // invalid architecture (dangling edge) is refused with details
    let mut bad = arch.clone();
    bad["edges"][0]["target"] = json!("does-not-exist");
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": bad, "expectedRev": rev + 1})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "invalid_architecture");

    // version snapshot, diff draft vs v1, restore v1
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions"), json!({"message": "more pods"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["n"], 2);
    let (s, d) = app.get(&c, &format!("/api/projects/{pid}/diff?from=1&to=2")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(d["nodes"]["changed"].as_array().unwrap().len(), 1);
    assert_eq!(d["nodes"]["changed"][0]["changes"][0]["path"], "props.replicas");
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions/1/restore"), json!({"expectedRev": rev + 1})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, p2) = app.get(&c, &format!("/api/projects/{pid}")).await;
    assert_ne!(p2["architecture"]["nodes"][2]["props"]["replicas"], json!(40));
    app.cleanup().await;
}

#[tokio::test]
async fn project_list_carries_a_thumbnail_of_the_diagram() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let (_, tpl) = app.get(&c, "/api/templates/two-dc-witness").await;
    let arch = tpl["architecture"].clone();
    let (s, created) = app.post(&c, "/api/projects", json!({"name": "thumb", "templateId": "two-dc-witness"})).await;
    assert_eq!(s, StatusCode::OK, "{created}");
    let (s, blank) = app.post(&c, "/api/projects", json!({"name": "blank"})).await;
    assert_eq!(s, StatusCode::OK, "{blank}");

    let (s, list) = app.get(&c, "/api/projects").await;
    assert_eq!(s, StatusCode::OK);
    let find = |id: &Value| list["projects"].as_array().unwrap().iter().find(|p| p["id"] == *id).unwrap().clone();

    // everything a thumbnail needs, nothing else: ids, positions, kinds, zone rectangles, edge ends
    let item = find(&created["id"]);
    let thumb = &item["thumb"];
    let nodes = thumb["nodes"].as_array().unwrap();
    assert_eq!(nodes.len() as u64, item["nodeCount"].as_u64().unwrap());
    assert_eq!(nodes.len(), arch["nodes"].as_array().unwrap().len());
    assert_eq!(thumb["zones"].as_array().unwrap().len(), arch["zones"].as_array().unwrap().len());
    assert_eq!(thumb["edges"].as_array().unwrap().len(), arch["edges"].as_array().unwrap().len());
    let ids: Vec<&str> = nodes.iter().map(|n| n["i"].as_str().unwrap()).collect();
    for n in nodes {
        assert!(n["k"].is_string(), "kind: {n}");
        // a template has no layout until the editor opens it: coordinates are null, not missing
        assert!(n.get("x").is_some() && n.get("y").is_some(), "{n}");
    }
    for z in thumb["zones"].as_array().unwrap() {
        assert!(z["t"].is_string(), "{z}");
    }
    for e in thumb["edges"].as_array().unwrap() {
        assert!(ids.contains(&e[0].as_str().unwrap()) && ids.contains(&e[1].as_str().unwrap()) && e[2].is_string(), "{e}");
    }

    // once the editor has laid the diagram out and saved it, the thumbnail has the coordinates
    let (_, p) = app.get(&c, &format!("/api/projects/{}", created["id"].as_str().unwrap())).await;
    let mut laid = p["architecture"].clone();
    for (i, n) in laid["nodes"].as_array_mut().unwrap().iter_mut().enumerate() {
        n["ui"] = json!({"x": 40 + (i as i64 % 4) * 260, "y": 60 + (i as i64 / 4) * 120});
    }
    for (i, z) in laid["zones"].as_array_mut().unwrap().iter_mut().enumerate() {
        z["ui"] = json!({"x": 20 + i as i64 * 600, "y": 10, "w": 560, "h": 900});
    }
    let (s, v) = app.put(&c, &format!("/api/projects/{}/draft", created["id"].as_str().unwrap()), json!({"architecture": laid, "expectedRev": p["rev"]})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = app.get(&c, "/api/projects").await;
    let item = list["projects"].as_array().unwrap().iter().find(|p| p["id"] == created["id"]).unwrap().clone();
    let thumb = &item["thumb"];
    for n in thumb["nodes"].as_array().unwrap() {
        assert!(n["x"].is_number() && n["y"].is_number(), "{n}");
    }
    for z in thumb["zones"].as_array().unwrap() {
        assert!(z["w"].as_f64().unwrap() > 0.0 && z["h"].as_f64().unwrap() > 0.0, "{z}");
    }
    // names and properties stay on the server: the list must not grow with the document
    assert!(thumb["nodes"].as_array().unwrap().iter().all(|n| n.get("name").is_none() && n.get("props").is_none()));

    // an empty project has an empty thumbnail
    let item = find(&blank["id"]);
    assert_eq!(item["thumb"], json!({"zones": [], "nodes": [], "edges": []}));

    // a long list stays light: only the most recently changed projects bring their thumbnail (`limits.listThumbnails`),
    // the web UI loads the others as their cards scroll into view
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"limits.listThumbnails": 1}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = app.get(&c, "/api/projects").await;
    let items = list["projects"].as_array().unwrap();
    assert!(items.len() >= 3, "demo project, the template project and the blank one");
    assert!(items[0]["thumb"].is_object(), "the newest project has its thumbnail");
    assert!(items[1..].iter().all(|p| p["thumb"].is_null()), "the others are loaded later: {:?}", items.iter().map(|p| p["thumb"].is_null()).collect::<Vec<_>>());
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"limits.listThumbnails": 0}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = app.get(&c, "/api/projects").await;
    assert!(list["projects"].as_array().unwrap().iter().all(|p| p["thumb"].is_null()));
    app.cleanup().await;
}

#[tokio::test]
async fn recommendations_try_the_fixes_and_say_what_each_one_measurably_does() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let (_, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
    // the demo's open findings: a queue replicated asynchronously has a fix, a cache without a replica has none
    let (s, plan) = app.post(&c, "/api/recommendations", json!({"architecture": p["architecture"], "projectId": pid, "lang": "en"})).await;
    assert_eq!(s, StatusCode::OK, "{plan}");
    let items = plan["items"].as_array().unwrap();
    assert!(!items.is_empty(), "{plan}");
    for r in items {
        let e = &r["effect"];
        assert!(e["score"]["after"].as_f64().unwrap() >= e["score"]["before"].as_f64().unwrap() || e["critical"]["after"].as_u64() < e["critical"]["before"].as_u64(), "{r}");
        assert!(r["ops"].as_array().map(|o| !o.is_empty()).unwrap_or(false));
        assert!(r["vars"].is_object() && r["findingId"].is_string() && r["ruleId"].is_string());
    }
    assert!(plan["manual"].as_array().unwrap().iter().any(|id| id.as_str().unwrap().starts_with("HA-CACHE-01")), "{plan}");

    // applying a recommendation does what it said: the score the effect names is the score the evaluation gives
    let first = &items[0];
    let (s, applied) = app.post(&c, "/api/ops/apply", json!({"architecture": p["architecture"], "ops": first["ops"]})).await;
    assert_eq!(s, StatusCode::OK, "{applied}");
    let (_, rep) = app.post(&c, "/api/evaluate", json!({"architecture": applied["architecture"], "projectId": pid})).await;
    assert_eq!(rep["score"]["value"], first["effect"]["score"]["after"]);

    // a risk the project accepted is not recommended away
    let fp = first["findingId"].as_str().unwrap();
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/waivers"), json!({"fingerprint": fp, "reason": "accepted on purpose, see the design review"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, plan2) = app.post(&c, "/api/recommendations", json!({"architecture": p["architecture"], "projectId": pid})).await;
    assert!(plan2["items"].as_array().unwrap().iter().all(|r| r["findingId"] != fp), "{plan2}");

    // an architecture the validator rejects is a client error, not a crash
    let (s, _) = app.post(&c, "/api/recommendations", json!({"architecture": {"nodes": [{"id": "n", "kind": "nope", "name": "N"}]}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    app.cleanup().await;
}

#[tokio::test]
async fn waivers_require_a_real_finding_and_a_reason_and_affect_the_score() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let (_, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
    let (_, rep) = app.post(&c, "/api/evaluate", json!({"architecture": p["architecture"], "projectId": pid})).await;
    let f = rep["findings"].as_array().unwrap().iter().find(|f| f["ruleId"] == "DR-KFK-01").unwrap();
    let fp = f["id"].as_str().unwrap();

    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/waivers"), json!({"fingerprint": fp, "reason": "short"})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "reason_too_short");
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/waivers"), json!({"fingerprint": "X-1:nonsense", "reason": "this is a sufficiently long justification"})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "unknown_finding");
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/waivers"), json!({"fingerprint": fp, "reason": "orders are protected by the outbox pattern"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let (_, rep2) = app.post(&c, "/api/evaluate", json!({"architecture": p["architecture"], "projectId": pid})).await;
    assert_eq!(rep2["counts"]["accepted"], 1);
    assert!(rep2["score"]["value"].as_f64().unwrap() > rep["score"]["value"].as_f64().unwrap());
    let w = rep2["findings"].as_array().unwrap().iter().find(|f| f["id"] == fp).unwrap();
    assert!(w["accepted"]["reason"].as_str().unwrap().contains("outbox"));
    app.cleanup().await;
}

#[tokio::test]
async fn roles_are_enforced() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    let viewer = app.create_user(&admin, "viewer@test.local", "viewer").await;
    let editor = app.create_user(&admin, "editor@test.local", "editor").await;
    let pid = app.first_project(&admin).await;
    let (_, p) = app.get(&viewer, &format!("/api/projects/{pid}")).await;
    // viewer reads but cannot write
    let (s, _) = app.post(&viewer, "/api/projects", json!({"name": "n"})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.put(&viewer, &format!("/api/projects/{pid}/draft"), json!({"architecture": p["architecture"], "expectedRev": p["rev"]})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // editor writes but is not an admin
    let (s, _) = app.post(&editor, "/api/projects", json!({"name": "n"})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app.get(&editor, "/api/admin/settings").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.get(&editor, "/api/admin/users").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // disabling a user revokes the session; the last admin cannot be removed
    let (_, users) = app.get(&admin, "/api/admin/users").await;
    let editor_id = users["users"].as_array().unwrap().iter().find(|u| u["email"] == "editor@test.local").unwrap()["id"].as_str().unwrap().to_string();
    let admin_id = users["users"].as_array().unwrap().iter().find(|u| u["email"] == "admin@test.local").unwrap()["id"].as_str().unwrap().to_string();
    let (s, _) = app.patch(&admin, &format!("/api/admin/users/{editor_id}"), json!({"disabled": true})).await;
    assert_eq!(s, StatusCode::OK);
    // session cache is 10 s; the DB-level check happens on the next cache miss, so verify through a fresh login
    let (s, _, _) = app.login_as("editor@test.local", "another long password 1").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v) = app.patch(&admin, &format!("/api/admin/users/{admin_id}"), json!({"role": "viewer"})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["code"], "self_lockout");
    app.cleanup().await;
}

#[tokio::test]
async fn admin_settings_secrets_and_config_changes_take_effect() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;

    // secret is stored encrypted and never returned
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"llm.apiKey": "sk-or-v1-supersecret", "llm.temperature": 0.5, "llm.model.fallbacks": ["a/b", "c/d"]}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(!v.to_string().contains("supersecret"));
    let key_def = v["settings"].as_array().unwrap().iter().find(|d| d["key"] == "llm.apiKey").unwrap();
    assert_eq!(key_def["secretSet"], true);
    assert_eq!(key_def["secretSource"], "database");
    let raw: (Value, bool) = sqlx::query_as("select value, encrypted from settings where key = 'llm.apiKey'").fetch_one(&app.state.pool).await.unwrap();
    assert!(raw.1 && raw.0.as_str().unwrap().starts_with("v1:") && !raw.0.to_string().contains("supersecret"));
    // validation
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"llm.temperature": 9}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = app.put(&c, "/api/admin/settings", json!({"changes": {"no.such.key": 1}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // bootstrap now reports AI as configured
    let (_, b) = app.get(&c, "/api/bootstrap").await;
    assert_eq!(b["ai"]["configured"], true);

    // disabling a rule in the admin panel changes the evaluation immediately
    let pid = app.first_project(&c).await;
    let (_, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
    let (_, before) = app.post(&c, "/api/evaluate", json!({"architecture": p["architecture"]})).await;
    assert!(before["rules"].as_array().unwrap().iter().any(|r| r["ruleId"] == "HA-CACHE-01" && r["status"] == "risk"));
    let (_, list) = app.get(&c, "/api/admin/config/rule").await;
    let mut rule = list["items"].as_array().unwrap().iter().find(|i| i["id"] == "HA-CACHE-01").unwrap()["data"].clone();
    rule["enabled"] = json!(false);
    let (s, v) = app.put(&c, "/api/admin/config/rule/HA-CACHE-01", json!({"data": rule})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, after) = app.post(&c, "/api/evaluate", json!({"architecture": p["architecture"]})).await;
    assert!(after["rules"].as_array().unwrap().iter().all(|r| r["ruleId"] != "HA-CACHE-01"));
    // invalid config is refused, reset restores the default
    let (s, _) = app.put(&c, "/api/admin/config/rule/HA-CACHE-01", json!({"data": {"id": "HA-CACHE-01", "check": "does_not_exist"}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app.post(&c, "/api/admin/config/rule/HA-CACHE-01/reset", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let (_, again) = app.post(&c, "/api/evaluate", json!({"architecture": p["architecture"]})).await;
    assert!(again["rules"].as_array().unwrap().iter().any(|r| r["ruleId"] == "HA-CACHE-01"));

    // custom rule instance on a built-in check type
    let custom = json!({"id": "CUSTOM-1", "category": "gov", "check": "prop", "severity": "risk",
        "params": {"subject": "node", "where": {"kind": "kubernetes"}, "key": "hpaMax", "op": "gte", "value": 100, "onMissing": "no_data"},
        "title": {"en": "HPA max is generous"}, "message": {"en": "HPA max of {node} is {actual}"}, "ok": {"en": "ok"}});
    let (s, v) = app.post(&c, "/api/admin/config/rule", json!({"id": "CUSTOM-1", "data": custom})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, rep) = app.post(&c, "/api/evaluate", json!({"architecture": p["architecture"]})).await;
    assert!(rep["rules"].as_array().unwrap().iter().any(|r| r["ruleId"] == "CUSTOM-1" && r["status"] == "risk"));
    let (s, _) = app.delete(&c, "/api/admin/config/rule/CUSTOM-1").await;
    assert_eq!(s, StatusCode::OK);
    // built-in items cannot be deleted
    let (s, _) = app.delete(&c, "/api/admin/config/rule/HA-SPOF-01").await;
    assert_eq!(s, StatusCode::CONFLICT);

    // audit trail recorded the changes without the secret
    let (_, a) = app.get(&c, "/api/admin/audit?limit=50").await;
    let text = a.to_string();
    assert!(text.contains("settings.update") && text.contains("config.update") && !text.contains("supersecret"));
    app.cleanup().await;
}

#[tokio::test]
async fn simulate_and_ops_preview() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let (_, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
    let (s, r) = app.post(&c, "/api/simulate", json!({"architecture": p["architecture"], "scenario": {"type": "zone", "zoneId": "dc1"}})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_ne!(r["state"], "down");
    assert_eq!(r["recovery"]["kind"], "auto");
    assert_eq!(r["nodeStates"]["pg-replica"], "promoted");
    // the story of the recovery: the moment of the failure, then every step that takes time; the last frame is the settled picture
    let frames = r["timeline"].as_array().expect("a timeline for a failure that recovers by itself");
    assert!(frames.len() >= 2 && frames[0]["at"] == 0.0, "{r}");
    assert!(frames[0]["nodes"].as_object().unwrap().len() >= 10, "the first frame holds every node");
    let promoted_at = r["events"].as_array().unwrap().iter().find(|e| e["code"] == "promoted").and_then(|e| e["at"].as_f64()).expect("a promotion has a time");
    assert!(frames.iter().any(|f| (f["at"].as_f64().unwrap() - promoted_at).abs() < 1e-9 && f["nodes"]["pg-replica"] == "promoted"), "the replica is promoted in the frame of its promotion");
    // where the traffic goes: in normal operation (nothing is lost), the moment the data centre is gone (requests sent into it are lost
    // until the balancers notice) and once everything has settled; every figure is relative to the load that enters (1 = all of it)
    let before = &r["trafficBefore"];
    assert!(before["served"].as_f64().unwrap() > 0.999, "nothing is lost in normal operation: {before}");
    assert!(before["flows"].as_object().unwrap().len() >= 10 && before.get("lost").is_none(), "{before}");
    assert!(frames[0]["traffic"]["flows"].is_object(), "every picture of the story says where the traffic goes");
    assert!(!frames[0]["traffic"]["lost"].as_object().expect("requests sent into the dead data centre are lost at first").is_empty());
    assert!(frames[0]["traffic"]["served"].as_f64().unwrap() < before["served"].as_f64().unwrap());
    assert!(r["traffic"]["served"].as_f64().unwrap() > 0.5, "once settled most of the requests are served again: {}", r["traffic"]);
    // a request without waiting steps has no story, but it has the traffic of normal operation (it is also what it compares with)
    let (_, none) = app.post(&c, "/api/simulate", json!({"architecture": p["architecture"], "scenario": {"type": "none"}})).await;
    assert!(none.get("timeline").is_none(), "{none}");
    assert_eq!(none["trafficBefore"], none["traffic"]);
    assert_eq!(none["trafficBefore"], *before, "the picture of normal operation does not depend on the failure it is compared with");

    let ops = json!([{"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 10}}]);
    let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": p["architecture"], "ops": ops, "preview": true})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["after"]["score"].as_f64().unwrap() <= v["before"]["score"].as_f64().unwrap(), "shrinking the app must not improve the score");
    assert!(!v["introduced"].as_array().unwrap().is_empty(), "capacity findings are introduced: {v}");
    let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": p["architecture"], "ops": [{"op": "remove_node", "id": "ghost"}]})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "ops_invalid");
    app.cleanup().await;
}

#[tokio::test]
async fn placement_options_preview_compare_and_project_creation() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;

    let (s, o) = app.get(&c, "/api/placement/options").await;
    assert_eq!(s, StatusCode::OK, "{o}");
    let patterns = o["patterns"].as_array().unwrap();
    assert!(patterns.iter().any(|p| p["id"] == "single-dc") && patterns.iter().any(|p| p["id"] == "n-sites"), "{o}");
    let layers = o["stacks"][0]["layers"].as_array().unwrap();
    assert!(layers.iter().any(|l| l["id"] == "db" && l["optional"] == true), "{o}");
    assert_eq!(o["maxNodes"], 500);

    // the pattern the wizard offers first is data: the administrator's choice, the first one when that is not on offer
    assert_eq!(o["defaultPattern"], "n-sites", "{o}");
    let (_, listed) = app.get(&c, "/api/admin/config/placement.settings").await;
    let mut settings = listed["items"][0]["data"].clone();
    assert_eq!(settings["defaultPattern"], "n-sites");
    settings["defaultPattern"] = json!("single-dc");
    let (s, v) = app.put(&c, "/api/admin/config/placement.settings/placement", json!({"data": settings.clone()})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, chosen) = app.get(&c, "/api/placement/options").await;
    assert_eq!(chosen["defaultPattern"], "single-dc", "{chosen}");
    settings["defaultPattern"] = json!("no-such-pattern");
    let (s, v) = app.put(&c, "/api/admin/config/placement.settings/placement", json!({"data": settings})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, fallback) = app.get(&c, "/api/placement/options").await;
    assert_eq!(fallback["defaultPattern"], fallback["patterns"][0]["id"], "an unknown choice falls back to the first pattern: {fallback}");
    let (_, kinds) = app.get(&c, "/api/admin/config").await;
    assert!(
        kinds["errors"].as_array().unwrap().iter().any(|e| e["kind"] == "placement.settings" && e["error"].as_str().unwrap_or("").contains("no-such-pattern")),
        "the administrator is told why: {kinds}"
    );
    let (s, _) = app.post(&c, "/api/admin/config/placement.settings/placement/reset", json!({})).await;
    assert_eq!(s, StatusCode::OK);

    // five sites: the engine judges the generated design, the response says what it survives
    let (s, p) = app.post(&c, "/api/placement/preview", json!({"pattern": "n-sites", "sites": 5})).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(p["siteFailures"], 2);
    assert_eq!(p["sites"].as_array().unwrap().len(), 5);
    assert_eq!(p["report"]["counts"]["critical"], 0, "{}", p["report"]);
    assert!(p["report"]["score"].as_f64().unwrap() >= 90.0);
    assert!(p["report"]["objectives"].as_array().unwrap().iter().any(|o| o["id"] == "sites_loss" && o["status"] == "ok"));
    assert_eq!(p["architecture"]["requirements"]["siteFailures"], 2);

    // wrong numbers are refused with a code the UI translates
    let (s, e) = app.post(&c, "/api/placement/preview", json!({"pattern": "two-sites", "sites": 7})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    assert_eq!(e["error"]["details"]["code"], "sites_out_of_range");

    // one request compares 1, 2, 3, 5 and 10 sites
    let variants: Vec<Value> = [3, 4, 5, 7, 10].iter().map(|n| json!({"sites": n})).collect();
    let (s, cmp) = app.post(&c, "/api/placement/compare", json!({"base": {"pattern": "n-sites"}, "variants": variants})).await;
    assert_eq!(s, StatusCode::OK, "{cmp}");
    let v = cmp["variants"].as_array().unwrap();
    assert_eq!(v.len(), 5);
    assert!(v.iter().all(|x| x["ok"] == true), "{cmp}");
    let cost = |i: usize| v[i]["report"]["cost"]["total"].as_f64().unwrap();
    assert!(cost(4) > cost(0), "ten sites cost more than three");
    let (s, e) = app.post(&c, "/api/placement/compare", json!({"base": {"pattern": "n-sites"}, "variants": []})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");

    // a project is created straight from the request: the draft is the generated architecture, the first version is judged
    let (s, created) = app.post(&c, "/api/projects", json!({"name": "Ten sites", "placement": {"pattern": "n-sites", "sites": 10}})).await;
    assert_eq!(s, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let (_, got) = app.get(&c, &format!("/api/projects/{id}")).await;
    // ten working sites and the arbiter site that gives an even number of locations a deciding vote
    assert_eq!(got["architecture"]["zones"].as_array().unwrap().len(), 11);
    let (_, versions) = app.get(&c, &format!("/api/projects/{id}/versions")).await;
    assert_eq!(versions["versions"][0]["message"], "Created from a placement pattern", "{versions}");
    let (s, e) = app.post(&c, "/api/projects", json!({"name": "Bad", "placement": {"pattern": "n-sites", "sites": 99}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");

    // viewers cannot create projects, but may look at previews
    let viewer = app.create_user(&c, "viewer@test.local", "viewer").await;
    let (s, _) = app.post(&viewer, "/api/placement/preview", json!({"pattern": "single-dc", "sites": 1})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app.post(&viewer, "/api/projects", json!({"name": "No", "placement": {"pattern": "single-dc", "sites": 1}})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    app.cleanup().await;
}

#[tokio::test]
async fn diff_endpoint_compares_two_architectures_without_storing_anything() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let (_, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
    let design = p["architecture"].clone();
    let mut actual = design.clone();
    let removed = actual["nodes"].as_array_mut().unwrap().remove(0);
    actual["nodes"][0]["props"]["replicas"] = json!(99);
    actual["nodes"][0]["ui"] = json!({"x": 1, "y": 2});

    let (s, d) = app.post(&c, "/api/diff", json!({"from": design, "to": actual})).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["nodes"]["removed"][0]["id"], removed["id"], "{d}");
    let changed = d["nodes"]["changed"].as_array().unwrap();
    assert!(changed.iter().any(|c| c["changes"].as_array().unwrap().iter().any(|x| x["path"] == "props.replicas")), "{d}");
    assert!(!changed.iter().any(|c| c["changes"].as_array().unwrap().iter().any(|x| x["path"].as_str().unwrap().starts_with("ui"))), "positions are not drift");

    let (s, v) = app.post(&c, "/api/diff", json!({"from": {"nodes": "nope"}, "to": design})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "invalid_architecture");
    app.cleanup().await;
}

#[tokio::test]
async fn admin_edge_cases_numeric_defaults_missing_items_self_lockout_and_audit_prefix() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;

    // saving a number equal to its default must not leave the setting "modified"
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"auth.sessionTtlHours": 24}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"auth.sessionTtlHours": 12.0}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let item = v["settings"].as_array().unwrap().iter().find(|x| x["key"] == "auth.sessionTtlHours").unwrap();
    assert_eq!(item["modified"], false, "{item}");

    // invalid values name the key and the reason
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"auth.sessionTtlHours": 100000}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "invalid_setting");
    assert_eq!(v["error"]["details"]["key"], "auth.sessionTtlHours");

    // unknown items are 404, built-in ones cannot be deleted
    let (s, _) = app.delete(&c, "/api/admin/config/rule/NO-SUCH-RULE").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = app.delete(&c, "/api/admin/config/rule/HA-SPOF-01").await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["code"], "not_deletable");

    // an administrator cannot lock themselves out even when another admin exists
    let other = app.create_user(&c, "second-admin@test.local", "admin").await;
    let _ = other;
    let (_, me) = app.get(&c, "/api/auth/me").await;
    let my_id = me["user"]["id"].as_str().unwrap().to_string();
    let (s, v) = app.patch(&c, &format!("/api/admin/users/{my_id}"), json!({"role": "editor"})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["code"], "self_lockout");

    // audit filter is a literal prefix: `_` is not a wildcard
    let (_, a) = app.get(&c, "/api/admin/audit?action=user.cre_te").await;
    assert_eq!(a["events"].as_array().unwrap().len(), 0, "{a}");
    let (_, a) = app.get(&c, "/api/admin/audit?action=user.create").await;
    assert!(!a["events"].as_array().unwrap().is_empty());
    app.cleanup().await;
}

#[tokio::test]
async fn exports_zip_and_json() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let (s, v) = app.get(&c, &format!("/api/projects/{pid}/export/terraform?lang=ru")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["files"].as_array().unwrap().iter().any(|f| f["path"] == "app-dc1/00-platform/main.tf"), "{v}");
    // the report says what was and was not generated – in the requested language
    let report = &v["report"];
    assert!(report["generated"].as_array().unwrap().iter().any(|g| g["kind"] == "postgresql"), "{report}");
    assert!(report["skipped"].as_array().unwrap().iter().any(|g| g["code"] == "needs_input" && g["props"][0] == "image"), "{report}");
    assert!(report["skipped"].as_array().unwrap().iter().any(|g| g["code"] == "no_generator"), "{report}");
    assert!(!report["prerequisites"].as_array().unwrap().is_empty());
    let (s, d) = app.get(&c, &format!("/api/projects/{pid}/export/adr?lang=en")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(d["report"].is_null(), "documents carry no IaC report");

    let (s, h, _) = app.request(Method::GET, &format!("/api/projects/{pid}/export/terraform?download=1"), None, Some(c.clone()), &[]).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.get("content-type").unwrap(), "application/zip");
    let (s, h, _) = app.request(Method::GET, &format!("/api/projects/{pid}/export/adr?download=1&lang=en"), None, Some(c.clone()), &[]).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h.get("content-disposition").unwrap().to_str().unwrap().contains("ADR-0001.md"));
    let (s, _) = app.get(&c, &format!("/api/projects/{pid}/export/nope")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // bootstrap announces the formats with their group
    let (_, boot) = app.get(&c, "/api/bootstrap").await;
    let formats = boot["exportFormats"].as_array().unwrap();
    assert!(formats.iter().any(|f| f["id"] == "kubernetes" && f["group"] == "iac"), "{formats:?}");
    assert!(formats.iter().any(|f| f["id"] == "adr" && f["group"] == "docs"));
    app.cleanup().await;
}

#[tokio::test]
async fn iac_templates_are_configuration_checked_when_saved_and_effective_immediately() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;

    // the shipped generators are config items like any other
    let (s, list) = app.get(&c, "/api/admin/config/iac").await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let item = |id: &str| list["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap_or_else(|| panic!("{id} not listed")).clone();
    let mut data = item("postgresql")["data"].clone();

    // a typo in an expression is refused with its location, nothing is stored
    let mut broken = data.clone();
    broken["objects"][0]["body"]["spec"]["instances"] = json!("{{ site.instances + }}");
    let (s, v) = app.put(&c, "/api/admin/config/iac/postgresql", json!({"data": broken})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("objects[0].body.spec.instances"), "{v}");

    // a valid change takes effect on the next export without a restart
    data["objects"][0]["body"]["metadata"]["annotations"]["team"] = json!("db-platform");
    let (s, v) = app.put(&c, "/api/admin/config/iac/postgresql", json!({"data": data})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let mut seen = false;
    for _ in 0..40 {
        let (_, e) = app.get(&c, &format!("/api/projects/{pid}/export/kubernetes?lang=en")).await;
        if e["files"].as_array().unwrap().iter().any(|f| f["path"].as_str().unwrap().ends_with("pg-cluster.yaml") && f["content"].as_str().unwrap().contains("team: db-platform")) {
            seen = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(seen, "the edited template was not used by the export");

    // disabling a generator is reported, not silently ignored
    let mut kafka = item("kafka")["data"].clone();
    kafka["enabled"] = json!(false);
    let (s, _) = app.put(&c, "/api/admin/config/iac/kafka", json!({"data": kafka})).await;
    assert_eq!(s, StatusCode::OK);
    let mut disabled = false;
    for _ in 0..40 {
        let (_, e) = app.get(&c, &format!("/api/projects/{pid}/export/kubernetes?lang=en")).await;
        if e["report"]["skipped"].as_array().unwrap().iter().any(|x| x["kind"] == "kafka" && x["code"] == "disabled") {
            disabled = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(disabled);
    app.cleanup().await;
}

// ---------------------------------------------------------------------------------------------
// AI: everything around the model that needs no model. The model itself is exercised by tests/live.rs
// (real OpenRouter) and by the pure pipeline tests (recorded real answers).
// ---------------------------------------------------------------------------------------------

async fn wait_job(app: &TestApp, c: &str, job: &str) -> Value {
    for _ in 0..100 {
        let (_, j) = app.get(c, &format!("/api/ai/jobs/{job}")).await;
        if matches!(j["status"].as_str(), Some("succeeded") | Some("failed") | Some("cancelled")) {
            return j;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("job did not finish");
}

async fn set(app: &TestApp, c: &str, changes: Value) {
    let (s, v) = app.put(c, "/api/admin/settings", json!({"changes": changes})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

#[tokio::test]
async fn ai_requests_are_guarded_by_configuration_limits_and_budget() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let jobs = format!("/api/projects/{pid}/ai/jobs");

    // not configured
    let (s, v) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "hi"})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"]["code"], "llm_not_configured");

    // a key is set; nothing below ever reaches a model
    set(&app, &c, json!({"llm.apiKey": "sk-or-v1-test-key", "llm.retries": 0})).await;
    let (s, v) = app.post(&c, &jobs, json!({"kind": "teleport", "prompt": "hi"})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_kind")));
    let (s, v) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "   "})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("empty_prompt")));
    let (s, v) = app.post(&c, &jobs, json!({"kind": "fix"})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("missing_finding")));
    set(&app, &c, json!({"llm.maxPromptChars": 100})).await;
    let (s, v) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "x".repeat(200)})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("prompt_too_long")), "{v}");
    set(&app, &c, json!({"llm.maxPromptChars": 8000})).await;

    // a viewer may ask questions but cannot change the diagram
    let viewer = app.create_user(&c, "viewer@test.local", "viewer").await;
    let (s, _) = app.post(&viewer, &jobs, json!({"kind": "patch", "prompt": "add a cache"})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // a real network failure (nothing listens on the discard port) is a failed job with a stable code
    set(&app, &c, json!({"llm.baseUrl": "http://127.0.0.1:9"})).await;
    let (s, j) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "how resilient is this?"})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let job = wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;
    assert_eq!((job["status"].as_str(), job["errorCode"].as_str()), (Some("failed"), Some("llm_error")), "{job}");
    assert!(job["error"].as_str().unwrap().contains("network error"), "{job}");
    // the failed attempt is in the usage ledger
    let (n,): (i64,) = sqlx::query_as("select count(*) from llm_calls where status = 'error'").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(n, 1);

    // monthly budget: real spend recorded in the ledger blocks further calls before any request is made
    sqlx::query("insert into llm_calls (purpose, requested_model, model, status, prompt_tokens, completion_tokens, cost_usd, latency_ms) values ('chat', 'm', 'm', 'ok', 1000, 500, 5.0, 900)")
        .execute(&app.state.pool)
        .await
        .unwrap();
    set(&app, &c, json!({"llm.monthlyBudgetUsd": 1})).await;
    let (_, j) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "again"})).await;
    let job = wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;
    assert_eq!((job["status"].as_str(), job["errorCode"].as_str()), (Some("failed"), Some("llm_budget")), "{job}");
    let (_, u) = app.get(&c, "/api/admin/llm/usage?days=1").await;
    assert!(u["monthSpendUsd"].as_f64().unwrap() >= 5.0, "{u}");
    set(&app, &c, json!({"llm.monthlyBudgetUsd": 0})).await;

    // fair use per user and hour (counted in the database, so it holds across replicas)
    set(&app, &c, json!({"llm.perUserPerHour": 2})).await; // two requests were made above
    let (s, v) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "one more"})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("ai_rate_limited")), "{v}");
    set(&app, &c, json!({"llm.perUserPerHour": 0})).await;

    // one running job per project: a fresh heartbeat blocks, a stale one does not
    let running: uuid::Uuid = sqlx::query_scalar("insert into ai_jobs (project_id, kind, status, instance, heartbeat_at) values ($1, 'chat', 'running', 'other', now()) returning id")
        .bind(uuid::Uuid::parse_str(&pid).unwrap())
        .fetch_one(&app.state.pool)
        .await
        .unwrap();
    let (s, v) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "parallel"})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("job_running")), "{v}");
    assert_eq!(v["error"]["details"]["jobId"], running.to_string());
    sqlx::query("update ai_jobs set heartbeat_at = now() - interval '10 minutes' where id = $1").bind(running).execute(&app.state.pool).await.unwrap();
    let (s, _) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "now it is free"})).await;
    assert_eq!(s, StatusCode::OK);

    // AI switched off: the editor stays fully usable in manual mode
    set(&app, &c, json!({"llm.enabled": false})).await;
    let (s, v) = app.post(&c, &jobs, json!({"kind": "chat", "prompt": "hi"})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("llm_disabled")));
    let (_, boot) = app.get(&c, "/api/bootstrap").await;
    assert_eq!(boot["ai"]["enabled"], false);
    app.cleanup().await;
}

/// Proposals made from the operations of a recorded real model answer: preview, apply, rollback version,
/// stale detection and rejection – the part of the AI flow that happens after the model has answered.
#[tokio::test]
async fn proposals_are_previewed_applied_rejected_and_guarded_against_stale_state() {
    use engine::model::Architecture;
    use server::jobs::Job;
    use server::llm::pipeline::{parse_patch, propose_ops};

    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let pid_u = uuid::Uuid::parse_str(&pid).unwrap();
    let cfg = app.state.cfg().await.unwrap();
    let recorded: Value = serde_json::from_str(include_str!("fixtures/llm/patch_answer.json")).unwrap();
    let answer = recorded["answers"][0].as_str().unwrap();

    let make = |rev: i64, base: Architecture, ops: Vec<engine::ops::Op>, why: String| {
        let app = &app;
        let cfg = cfg.clone();
        async move {
            let job_id: uuid::Uuid =
                sqlx::query_scalar("insert into ai_jobs (project_id, kind, status) values ($1, 'patch', 'succeeded') returning id").bind(pid_u).fetch_one(&app.state.pool).await.unwrap();
            let job = Job { id: job_id, project_id: pid_u, user_id: None, kind: "patch".into(), input: json!({}) };
            propose_ops(&app.state, &cfg, &job, base, rev, ops, &why, "en").await.expect("proposal").to_string()
        }
    };
    let load = || async {
        let (_, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
        (p["rev"].as_i64().unwrap(), serde_json::from_value::<Architecture>(p["architecture"].clone()).unwrap())
    };

    // 1. the recorded real operations → a pending proposal with a preview computed by the engine
    let (rev, base) = load().await;
    let cand = parse_patch(&cfg, answer, &base).expect("recorded answer is valid");
    let proposal = make(rev, base.clone(), cand.ops.clone(), cand.rationale.clone()).await;
    let (s, pr) = app.get(&c, &format!("/api/proposals/{proposal}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((pr["status"].as_str(), pr["mode"].as_str()), (Some("pending"), Some("ops")));
    assert!(pr["preview"]["before"]["score"].is_number() && pr["preview"]["after"]["score"].is_number(), "{}", pr["preview"]);
    assert_eq!(pr["preview"]["diff"]["nodes"]["changed"][0]["id"], "app-dc1", "{}", pr["preview"]["diff"]);

    // 2. nothing changed yet; applying stores a rollback version first and is allowed once
    let (_, before) = app.get(&c, &format!("/api/projects/{pid}")).await;
    let replicas = |v: &Value| v["architecture"]["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "app-dc1").unwrap()["props"]["replicas"].clone();
    assert_ne!(replicas(&before), json!(40));
    let (s, v) = app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({"expectedRev": before["rev"]})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, after) = app.get(&c, &format!("/api/projects/{pid}")).await;
    assert_eq!(replicas(&after), json!(40));
    let (_, vers) = app.get(&c, &format!("/api/projects/{pid}/versions")).await;
    let msgs: Vec<&str> = vers["versions"].as_array().unwrap().iter().map(|v| v["message"].as_str().unwrap()).collect();
    assert!(msgs.iter().any(|m| m.starts_with("AI:")) && msgs.contains(&"Demo project"), "the previous state is already a version, the AI change is a new one: {msgs:?}");
    let (s, v) = app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("proposal_closed")));

    // 2b. unsaved work is never lost: if the draft differs from the last version it is snapshotted first
    let (rev, mut edited) = load().await;
    edited.nodes.iter_mut().find(|n| n.id == "app-dc2").unwrap().name = "App DC2 (edited by hand)".into();
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": edited, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (rev, base) = load().await;
    let second = make(rev, base, vec![engine::ops::Op::UpdateNode { id: "app-dc1".into(), set: [("props.replicas".to_string(), json!(41))].into_iter().collect() }], "tweak".into()).await;
    let (s, v) = app.post(&c, &format!("/api/proposals/{second}/apply"), json!({"expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, vers) = app.get(&c, &format!("/api/projects/{pid}/versions")).await;
    let snap = vers["versions"].as_array().unwrap().iter().find(|v| v["message"] == "Before AI change").expect("the hand-edited draft was snapshotted");
    let (_, old) = app.get(&c, &format!("/api/projects/{pid}/versions/{}", snap["n"])).await;
    assert!(old["architecture"]["nodes"].as_array().unwrap().iter().any(|n| n["name"] == "App DC2 (edited by hand)"));

    // 3. a proposal made for an older state is refused when the project moved on
    let (rev, base) = load().await;
    let stale = make(rev, base.clone(), vec![engine::ops::Op::UpdateNode { id: "app-dc2".into(), set: [("props.replicas".to_string(), json!(48))].into_iter().collect() }], "more".into()).await;
    let (s, _) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": base, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = app.post(&c, &format!("/api/proposals/{stale}/apply"), json!({"expectedRev": rev})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("conflict")), "{v}");
    assert!(v["error"]["details"]["currentRev"].as_i64().unwrap() > rev);

    // 4. operations that no longer fit the architecture mark the proposal stale
    let (rev, base) = load().await;
    let gone = make(rev, base.clone(), vec![engine::ops::Op::UpdateNode { id: "mon-dc1".into(), set: [("props.replicas".to_string(), json!(3))].into_iter().collect() }], "tune".into()).await;
    let mut without = base.clone();
    without.nodes.retain(|n| n.id != "mon-dc1");
    without.edges.retain(|e| e.source != "mon-dc1" && e.target != "mon-dc1");
    let (s, _) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": without, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = app.post(&c, &format!("/api/proposals/{gone}/apply"), json!({})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("proposal_stale")), "{v}");
    let (_, pr) = app.get(&c, &format!("/api/proposals/{gone}")).await;
    assert_eq!(pr["status"], "stale");

    // 5. rejecting closes a pending proposal exactly once
    let (rev, base) = load().await;
    let rejected = make(rev, base, vec![engine::ops::Op::UpdateNode { id: "app-dc2".into(), set: [("props.replicas".to_string(), json!(33))].into_iter().collect() }], "x".into()).await;
    let (s, _) = app.post(&c, &format!("/api/proposals/{rejected}/reject"), json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = app.post(&c, &format!("/api/proposals/{rejected}/reject"), json!({})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("proposal_closed")));
    app.cleanup().await;
}

#[tokio::test]
async fn stale_jobs_are_failed_and_config_rev_propagates_between_instances() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    // a job whose heartbeat is old (its instance died) must not stay running
    let id: uuid::Uuid =
        sqlx::query_scalar("insert into ai_jobs (project_id, kind, status, instance, heartbeat_at) values ($1, 'chat', 'running', 'dead', now() - interval '10 minutes') returning id")
            .bind(uuid::Uuid::parse_str(&pid).unwrap())
            .fetch_one(&app.state.pool)
            .await
            .unwrap();
    let st = app.state.clone();
    let h = tokio::spawn(server::jobs::sweeper(st));
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    h.abort();
    let (status, code): (String, Option<String>) = sqlx::query_as("select status, error_code from ai_jobs where id = $1").bind(id).fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(status, "failed");
    assert_eq!(code.as_deref(), Some("interrupted"));

    // a second "instance" (own cache) sees a change made through the first one after the TTL
    let other = server::config_store::ConfigCache::new(std::time::Duration::from_millis(0));
    let a = other.get(&app.state.pool, &app.state.keys).await.unwrap().rev;
    let (s, _) = app.put(&c, "/api/admin/settings", json!({"changes": {"app.name": "Renamed"}})).await;
    assert_eq!(s, StatusCode::OK);
    let b = other.get(&app.state.pool, &app.state.keys).await.unwrap();
    assert!(b.rev > a);
    assert_eq!(b.settings.str("app.name"), "Renamed");
    app.cleanup().await;
}

#[tokio::test]
async fn editor_timings_are_settings_and_reach_the_browser_through_bootstrap() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let (_, boot) = app.get(&c, "/api/bootstrap").await;
    let editor = boot["editor"].as_object().expect("bootstrap carries the editor settings");
    // every `editor.*` setting of the registry arrives under its short name, with its default
    let (_, reg) = app.get(&c, "/api/admin/settings").await;
    let defs: Vec<&Value> = reg["settings"].as_array().unwrap().iter().filter(|s| s["key"].as_str().unwrap().starts_with("editor.")).collect();
    assert_eq!(editor.len(), defs.len());
    for d in &defs {
        let short = d["key"].as_str().unwrap().trim_start_matches("editor.");
        assert_eq!(editor[short], d["value"], "{short}");
    }
    assert_eq!(editor["autosaveMs"], 900);

    // an administrator changes one: the next bootstrap says so; out-of-range values are refused
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"editor.autosaveMs": 1500, "editor.lodMiniBelow": 0.2}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, boot) = app.get(&c, "/api/bootstrap").await;
    assert_eq!(boot["editor"]["autosaveMs"], 1500);
    assert_eq!(boot["editor"]["lodMiniBelow"], 0.2);
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"editor.autosaveMs": 5}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn the_login_throttle_is_a_setting() {
    let app = TestApp::new().await;
    let c = app.login_admin().await; // one attempt counted for this address
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"auth.loginsPerMinutePerIp": 4}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // different accounts, so no account lockout interferes: the 4th attempt of the minute is the last one allowed
    let mut statuses = vec![];
    for i in 0..4 {
        let (s, _, _) = app.login_as(&format!("nobody{i}@test.local"), "wrong password 123").await;
        statuses.push(s);
    }
    assert_eq!(statuses, vec![StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED, StatusCode::TOO_MANY_REQUESTS], "{statuses:?}");
    // even the right password has to wait
    let (s, _, v) = app.login_as("admin@test.local", "correct horse battery staple").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn how_long_a_replica_trusts_a_users_role_is_a_setting() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    let editor = app.create_user(&admin, "cache@test.local", "editor").await;
    let (s, _) = app.get(&editor, "/api/projects").await;
    assert_eq!(s, StatusCode::OK);

    // another replica blocks the user in the database: this one has not heard of it yet and trusts its cache …
    sqlx::query("update users set disabled = true where email = 'cache@test.local'").execute(&app.state.pool).await.unwrap();
    let (s, _) = app.get(&editor, "/api/projects").await;
    assert_eq!(s, StatusCode::OK, "within the cache time (10 s by default) the block is not noticed yet");

    // … unless the administrator asked for no caching at all
    let (s, v) = app.put(&admin, "/api/admin/settings", json!({"changes": {"auth.userCacheSeconds": 0}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _) = app.get(&editor, "/api/projects").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "with `auth.userCacheSeconds` = 0 every request asks the database");
    app.cleanup().await;
}

#[tokio::test]
async fn release_tables_are_served_judged_edited_and_refreshed() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;

    // every signed-in user gets the tables, with names, and today's date; conditional GET works
    let (s, v) = app.get(&c, "/api/versions").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let tables = v["tables"].as_array().unwrap();
    assert!(tables.len() >= 100);
    assert!(tables.iter().all(|t| t["name"].as_str().map(|n| !n.is_empty()).unwrap_or(false) && !t["cycles"].as_array().unwrap().is_empty()));
    assert!(engine::lifecycle::parse_date(v["today"].as_str().unwrap()).is_some());
    let (_, h, _) = app.request(Method::GET, "/api/versions", None, Some(c.clone()), &[]).await;
    let etag = h.get("etag").unwrap().to_str().unwrap().to_string();
    let (s, _, _) = app.request(Method::GET, "/api/versions", None, Some(c.clone()), &[("if-none-match", &etag)]).await;
    assert_eq!(s, StatusCode::NOT_MODIFIED);

    // a declared version is judged by the table and the server's clock
    let eval = |version: &str| {
        json!({"architecture": {
            "requirements": {"rpsPeak": 100},
            "zones": [{"id": "dc1", "name": "DC1", "type": "dc", "props": {"geo": "a"}}],
            "nodes": [{"id": "n", "kind": "postgresql", "name": "PG", "zoneId": "dc1", "props": {"version": version}}],
            "edges": []
        }})
    };
    let status_of = |rep: &Value| rep["rules"].as_array().unwrap().iter().find(|r| r["ruleId"] == "VER-01").map(|r| r["status"].clone()).unwrap_or(Value::Null);
    let (s, rep) = app.post(&c, "/api/evaluate", eval("12.19")).await;
    assert_eq!(s, StatusCode::OK, "{rep}");
    assert_eq!(status_of(&rep), "critical", "PostgreSQL 12 stopped getting fixes in November 2024");
    let f = rep["findings"].as_array().unwrap().iter().find(|f| f["ruleId"] == "VER-01").unwrap();
    assert_eq!((f["vars"]["product"].as_str(), f["vars"]["version"].as_str(), f["vars"]["variant"].as_str()), (Some("PostgreSQL"), Some("12.19"), Some("expired")), "{f}");
    assert!(f["fixes"].as_array().unwrap().iter().any(|x| x["kind"] == "set_version"));

    // a table is configuration: a malformed one is refused, a valid edit takes effect at once
    let (_, list) = app.get(&c, "/api/admin/config/catalog.versions").await;
    let mut pg = list["items"].as_array().unwrap().iter().find(|i| i["id"] == "postgresql").unwrap()["data"].clone();
    let mut broken = pg.clone();
    broken["cycles"][0]["eol"] = json!("2030-13-45");
    let (s, v) = app.put(&c, "/api/admin/config/catalog.versions/postgresql", json!({"data": broken})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("is not a date"), "{v}");
    let mut empty = pg.clone();
    empty["cycles"] = json!([]);
    let (s, _) = app.put(&c, "/api/admin/config/catalog.versions/postgresql", json!({"data": empty})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "an empty table says nothing");
    for c12 in pg["cycles"].as_array_mut().unwrap().iter_mut().filter(|x| x["cycle"] == "12") {
        c12["eol"] = json!("2999-01-01");
    }
    let (s, v) = app.put(&c, "/api/admin/config/catalog.versions/postgresql", json!({"data": pg})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, rep) = app.post(&c, "/api/evaluate", eval("12.19")).await;
    assert_ne!(status_of(&rep), "critical", "an edited table is used immediately");

    // refreshing: only addresses under the configured bases are fetched; a failed source leaves the table as it was
    let mut custom = list["items"].as_array().unwrap().iter().find(|i| i["id"] == "kafka").unwrap()["data"].clone();
    custom["id"] = json!("elsewhere");
    custom["source"] = json!("http://127.0.0.1:9/api/kafka.json");
    let (s, v) = app.post(&c, "/api/admin/config/catalog.versions", json!({"id": "elsewhere", "data": custom})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app.post(&c, "/api/admin/versions/sync", json!({"ids": ["elsewhere"]})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["failed"], 1);
    assert!(v["results"][0]["error"].as_str().unwrap().contains("allowed"), "{v}");
    let (s, _) = app.put(&c, "/api/admin/settings", json!({"changes": {"versions.sourceBases": ["http://127.0.0.1:9/api/"]}})).await;
    assert_eq!(s, StatusCode::OK);
    let (_, v) = app.post(&c, "/api/admin/versions/sync", json!({"ids": ["elsewhere"]})).await;
    assert_eq!(v["failed"], 1, "{v}");
    assert!(!v["results"][0]["error"].as_str().unwrap().contains("allowed"), "now it is allowed, but nothing listens there: {v}");

    // an answer of the real source (captured from endoflife.date) replaces the cycles and moves asOf to today
    let rows: Value = serde_json::from_str(include_str!("fixtures/endoflife/postgresql.json")).unwrap();
    let today = server::versions::today_utc();
    let out = server::versions::apply_answer(&app.state, "postgresql", &rows, &today, 8.0).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.as_of.as_deref(), Some(today.as_str()));
    let (_, list) = app.get(&c, "/api/admin/config/catalog.versions").await;
    let pg = list["items"].as_array().unwrap().iter().find(|i| i["id"] == "postgresql").unwrap()["data"].clone();
    assert_eq!(pg["asOf"], today);
    assert_eq!(pg["name"], "PostgreSQL", "name and source survive a refresh");
    assert!(pg["cycles"].as_array().unwrap().iter().any(|x| x["cycle"] == "18"));
    // a bad answer never empties a table
    let out = server::versions::apply_answer(&app.state, "postgresql", &json!({"message": "Not found"}), &today, 8.0).await;
    assert!(!out.ok);
    let out = server::versions::apply_answer(&app.state, "no-such-table", &rows, &today, 8.0).await;
    assert!(!out.ok);
    let (_, rep) = app.post(&c, "/api/evaluate", eval("12.19")).await;
    assert_eq!(status_of(&rep), "critical", "the refreshed table says 12 is over again");

    // the admin sees how fresh the tables are; others cannot refresh
    let (s, st) = app.get(&c, "/api/admin/versions/status").await;
    assert_eq!(s, StatusCode::OK, "{st}");
    assert!(st["tables"].as_i64().unwrap() >= 100);
    assert_eq!(st["autoSyncDays"], 7.0);
    let viewer = app.create_user(&c, "viewer@test.local", "viewer").await;
    let (s, _) = app.post(&viewer, "/api/admin/versions/sync", json!({})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.get(&viewer, "/api/versions").await;
    assert_eq!(s, StatusCode::OK, "the tables are for everybody who edits");
    app.cleanup().await;
}

#[tokio::test]
async fn a_refreshed_table_is_not_overwritten_by_an_older_shipped_one_at_start() {
    let app = TestApp::new().await;
    // pretend this installation refreshed PostgreSQL's table after the program was built
    let today = server::versions::today_utc();
    let future = "2099-01-01";
    let mut data: Value = sqlx::query_scalar("select data from config_items where kind = 'catalog.versions' and id = 'postgresql'").fetch_one(&app.state.pool).await.unwrap();
    data["asOf"] = json!(future);
    data["cycles"][0]["latest"] = json!("18.99");
    sqlx::query("update config_items set data = $1 where kind = 'catalog.versions' and id = 'postgresql'").bind(&data).execute(&app.state.pool).await.unwrap();
    assert!(server::config_store::seed_defaults(&app.state.pool).await.is_ok());
    let kept: Value = sqlx::query_scalar("select data from config_items where kind = 'catalog.versions' and id = 'postgresql'").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(kept["asOf"], future, "{today}: a fresher table stays");
    assert_eq!(kept["cycles"][0]["latest"], "18.99");
    app.cleanup().await;
}

// ---------------------------------------------------------------------------------------------
// Integrity of the data: a handler never needs two connections at once, applying and restoring never
// lose a draft, a deleted project is gone for every endpoint, and a configuration import is all or nothing.
// ---------------------------------------------------------------------------------------------

/// The architecture of the demo project: a diagram with known nodes (`app-dc1` has `props.replicas`) that tests copy into as many projects as they need.
async fn demo_architecture(app: &TestApp, c: &str) -> Value {
    let pid = app.first_project(c).await;
    let (_, p) = app.get(c, &format!("/api/projects/{pid}")).await;
    p["architecture"].clone()
}

async fn new_project(app: &TestApp, c: &str, name: &str, arch: &Value) -> String {
    let (s, v) = app.post(c, "/api/projects", json!({"name": name, "architecture": arch})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["id"].as_str().unwrap().to_string()
}

fn replicas_of(node: &str, n: i64) -> engine::ops::Op {
    engine::ops::Op::UpdateNode { id: node.into(), set: [("props.replicas".to_string(), json!(n))].into_iter().collect() }
}

/// A pending proposal of `ops` for the project as it is now, stored exactly as the end of the patch pipeline stores it (`propose_ops`), without a model.
async fn pending_proposal(app: &TestApp, pid: &str, ops: Vec<engine::ops::Op>) -> String {
    use engine::model::Architecture;
    use server::jobs::Job;
    use server::llm::pipeline::propose_ops;
    let pid_u = uuid::Uuid::parse_str(pid).unwrap();
    let cfg = app.state.cfg().await.unwrap();
    let (draft, rev): (Value, i64) = sqlx::query_as("select draft, draft_rev from projects where id = $1").bind(pid_u).fetch_one(&app.state.pool).await.unwrap();
    let base: Architecture = serde_json::from_value(draft).unwrap();
    let job_id: uuid::Uuid = sqlx::query_scalar("insert into ai_jobs (project_id, kind, status) values ($1, 'patch', 'succeeded') returning id").bind(pid_u).fetch_one(&app.state.pool).await.unwrap();
    let job = Job { id: job_id, project_id: pid_u, user_id: None, kind: "patch".into(), input: json!({}) };
    propose_ops(&app.state, &cfg, &job, base, rev, ops, "tune", "en").await.expect("proposal").to_string()
}

async fn project_rev(app: &TestApp, c: &str, pid: &str) -> (i64, Value) {
    let (s, p) = app.get(c, &format!("/api/projects/{pid}")).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    (p["rev"].as_i64().unwrap(), p["architecture"].clone())
}

fn node_field<'a>(arch: &'a Value, id: &str, field: &str) -> &'a Value {
    &arch["nodes"].as_array().unwrap().iter().find(|n| n["id"] == id).unwrap_or_else(|| panic!("node {id}"))[field]
}

fn rename(arch: &mut Value, id: &str, name: &str) {
    arch["nodes"].as_array_mut().unwrap().iter_mut().find(|n| n["id"] == id).unwrap()["name"] = json!(name);
}

async fn version_messages(app: &TestApp, c: &str, pid: &str) -> Vec<String> {
    let (s, v) = app.get(c, &format!("/api/projects/{pid}/versions")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["versions"].as_array().unwrap().iter().map(|v| v["message"].as_str().unwrap().to_string()).collect()
}

/// Twelve people press "apply" at once. Every apply used to hold a connection for its transaction and ask the pool for a second one
/// (the accepted risks), so with the pool as small as the harness's five the applies waited for each other until the pool's ten-second
/// timeout and answered 500 - and the whole API had no database meanwhile.
#[tokio::test]
async fn a_burst_of_applies_does_not_exhaust_the_pool() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    // the gate of heavy requests would shed most of this burst before it reaches the pool (`a_burst_beyond_the_gate_is_shed_and_not_queued` says how); this test is
    // about what the handlers do with the pool when they all run at once, so the gate is off for it
    set(&app, &c, json!({"limits.heavyConcurrentPerUser": 0, "limits.heavyConcurrentPerCore": 0})).await;
    let arch = demo_architecture(&app, &c).await;
    let mut jobs = vec![];
    for i in 0..12 {
        let pid = new_project(&app, &c, &format!("burst {i}"), &arch).await;
        let proposal = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 40 + i)]).await;
        jobs.push((pid, proposal));
    }

    let started = std::time::Instant::now();
    let paths: Vec<String> = jobs.iter().map(|(_, p)| format!("/api/proposals/{p}/apply")).collect();
    let answers = futures::future::join_all(paths.iter().map(|path| app.post(&c, path, json!({})))).await;
    let took = started.elapsed();
    let statuses: Vec<u16> = answers.iter().map(|(s, _)| s.as_u16()).collect();
    assert_eq!(statuses, vec![200; 12], "different projects: every apply is legitimate; {answers:?}");
    // the pool's own timeout is 10 s: that is what the old behaviour took
    assert!(took < std::time::Duration::from_secs(9), "twelve applies took {took:?}");

    for (i, (pid, _)) in jobs.iter().enumerate() {
        let (_, now) = project_rev(&app, &c, pid).await;
        assert_eq!(node_field(&now, "app-dc1", "props")["replicas"], json!(40 + i as i64), "project {i} was changed");
        assert!(version_messages(&app, &c, pid).await.iter().any(|m| m.starts_with("AI:")), "project {i} has the version of the AI change");
    }
    let (s, _) = app.get(&c, "/api/projects").await;
    assert_eq!(s, StatusCode::OK, "the API still has its database");
    app.cleanup().await;
}

/// The structural guard against the whole class of bugs "a handler holds a connection and asks for another": with a pool of ONE
/// connection every handler that writes must still work, which it can only do by never needing two at the same time.
#[tokio::test]
async fn mutating_handlers_never_need_two_connections() {
    let app = TestApp::with_pool(1).await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    let ok = |what: &str, s: StatusCode, v: &Value| assert_eq!(s, StatusCode::OK, "{what}: {v}");

    let (s, v) = app.post(&c, "/api/projects", json!({"name": "one connection", "templateId": "two-dc-witness"})).await;
    ok("create from a template", s, &v);
    let pid = new_project(&app, &c, "one connection, copy", &arch).await;

    // the draft is saved, a version is cut, and an AI change is applied and another rejected
    let (rev, mut edited) = project_rev(&app, &c, &pid).await;
    rename(&mut edited, "app-dc2", "App DC2 (edited)");
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": edited, "expectedRev": rev})).await;
    ok("save the draft", s, &v);
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions"), json!({"message": "kept"})).await;
    ok("create a version", s, &v);
    let first = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 41)]).await;
    let (s, v) = app.post(&c, &format!("/api/proposals/{first}/apply"), json!({})).await;
    ok("apply", s, &v);
    let second = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 42)]).await;
    let (s, v) = app.post(&c, &format!("/api/proposals/{second}/reject"), json!({})).await;
    ok("reject", s, &v);

    // restore snapshots an unsaved draft first (the other path that used to be one query)
    let (rev, mut again) = project_rev(&app, &c, &pid).await;
    rename(&mut again, "app-dc2", "App DC2 (edited again)");
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": again, "expectedRev": rev})).await;
    ok("save the draft again", s, &v);
    let (rev, _) = project_rev(&app, &c, &pid).await;
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions/1/restore"), json!({"expectedRev": rev})).await;
    ok("restore", s, &v);

    // an accepted risk is made and revoked
    let (_, rep) = app.post(&c, "/api/evaluate", json!({"architecture": arch, "projectId": pid})).await;
    let fp = rep["findings"].as_array().unwrap().iter().find(|f| f["ruleId"] == "DR-KFK-01").unwrap()["id"].as_str().unwrap().to_string();
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/waivers"), json!({"fingerprint": fp, "reason": "orders are protected by the outbox pattern"})).await;
    ok("accept a risk", s, &v);
    let (s, v) = app.delete(&c, &format!("/api/projects/{pid}/waivers/{}", v["id"].as_str().unwrap())).await;
    ok("revoke a risk", s, &v);

    // the administrator's side: settings, and an import of items together with settings
    let (s, v) = app.put(&c, "/api/admin/settings", json!({"changes": {"app.name": "One connection"}})).await;
    ok("save settings", s, &v);
    let category = json!({"id": "zz-one-connection", "order": 99, "name": {"ru": "Проба", "en": "Probe"}});
    let (s, v) = app.post(&c, "/api/admin/config-import", json!({"items": [{"kind": "catalog.category", "id": "zz-one-connection", "data": category}], "settings": {"app.name": "Imported"}})).await;
    ok("import", s, &v);
    assert_eq!(v["applied"], true, "{v}");
    app.cleanup().await;
}

/// What somebody else does to the project while an apply is busy evaluating.
enum Meanwhile<'a> {
    /// A save of the working copy: `edit` changes the draft and the revision moves.
    Save(&'a dyn Fn(&mut Value)),
    /// A version cut of the working copy as it is (what `POST /versions` does): the history moves, the revision does not.
    CutVersion,
    /// The proposal is rejected by somebody else.
    Reject(&'a str),
}

/// The apply evaluates holding no lock, so it must look at the project again when it finally takes the lock. To make that happen without
/// timing, the test holds the project's row lock itself, waits until the apply is queued behind it (the database says when), does what
/// `meanwhile` says in that window and lets go.
async fn while_the_apply_waits(app: &TestApp, pid: &str, meanwhile: Meanwhile<'_>, apply: impl std::future::Future<Output = (StatusCode, Value)>) -> (StatusCode, Value) {
    let pid_u = uuid::Uuid::parse_str(pid).unwrap();
    let mut tx = app.state.pool.begin().await.unwrap();
    // the apply takes the proposal's row first and the project's second: to stall it before it has taken anything, hold the one it will want first
    // (holding the other one, and then touching the first, would be the deadlock the order exists to prevent)
    let mut draft = match &meanwhile {
        Meanwhile::Reject(proposal) => {
            sqlx::query("select 1 from proposals where id = $1::uuid for update").bind(*proposal).fetch_one(&mut *tx).await.unwrap();
            Value::Null
        }
        _ => sqlx::query_scalar("select draft from projects where id = $1 for update").bind(pid_u).fetch_one(&mut *tx).await.unwrap(),
    };
    let doing = async {
        let mut waited = false;
        for _ in 0..500 {
            let n: i64 = sqlx::query_scalar("select count(*) from pg_stat_activity where datname = current_database() and wait_event_type = 'Lock'").fetch_one(&app.state.pool).await.unwrap();
            if n > 0 {
                waited = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(waited, "the apply never queued for the project row");
        match meanwhile {
            Meanwhile::Save(edit) => {
                edit(&mut draft);
                sqlx::query("update projects set draft = $2, draft_rev = draft_rev + 1, updated_at = now() where id = $1").bind(pid_u).bind(&draft).execute(&mut *tx).await.unwrap();
            }
            Meanwhile::CutVersion => {
                sqlx::query(
                    "insert into versions (project_id, n, message, author, architecture, summary, engine_version)
                     select id, head_version + 1, 'cut meanwhile', 'test', draft, '{}', 1 from projects where id = $1",
                )
                .bind(pid_u)
                .execute(&mut *tx)
                .await
                .unwrap();
                sqlx::query("update projects set head_version = head_version + 1 where id = $1").bind(pid_u).execute(&mut *tx).await.unwrap();
            }
            Meanwhile::Reject(proposal) => {
                sqlx::query("update proposals set status = 'rejected', decided_at = now() where id = $1::uuid").bind(proposal).execute(&mut *tx).await.unwrap();
            }
        }
        tx.commit().await.unwrap();
    };
    let (answer, ()) = tokio::join!(apply, doing);
    answer
}

#[tokio::test]
async fn apply_rechecks_the_revision_after_evaluating() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    let pid = new_project(&app, &c, "moved while evaluating", &arch).await;
    let (rev, _) = project_rev(&app, &c, &pid).await;

    // 1. the client said which revision it reviewed: a draft saved meanwhile is a conflict, never a silent overwrite
    let proposal = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 41)]).await;
    let (s, v) =
        while_the_apply_waits(&app, &pid, Meanwhile::Save(&|d| rename(d, "app-dc2", "saved by somebody else")), app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({"expectedRev": rev})))
            .await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("conflict")), "{v}");
    assert_eq!(v["error"]["details"]["currentRev"], rev + 1, "{v}");
    let (now_rev, now) = project_rev(&app, &c, &pid).await;
    assert_eq!(now_rev, rev + 1);
    assert_eq!(node_field(&now, "app-dc2", "name"), "saved by somebody else", "the other save survives");
    assert_ne!(node_field(&now, "app-dc1", "props")["replicas"], json!(41), "the proposal was not applied");
    let (_, pr) = app.get(&c, &format!("/api/proposals/{proposal}")).await;
    assert_eq!(pr["status"], "pending", "it can still be reviewed again");
    assert_eq!(version_messages(&app, &c, &pid).await.len(), 1, "no version was written");

    // 2. operations without a reviewed revision are rebased: the apply starts over on the draft it finds, snapshots THAT draft and applies on top of it
    let (s, v) = while_the_apply_waits(&app, &pid, Meanwhile::Save(&|d| rename(d, "app-dc2", "saved while evaluating")), app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, now) = project_rev(&app, &c, &pid).await;
    assert_eq!(node_field(&now, "app-dc1", "props")["replicas"], json!(41), "the change is there");
    assert_eq!(node_field(&now, "app-dc2", "name"), "saved while evaluating", "and the save made meanwhile is not lost");
    assert_eq!(version_messages(&app, &c, &pid).await, ["AI: tune", "Before AI change", "Created"]);
    let snapshot = v["version"].as_i64().unwrap() - 1;
    let (_, old) = app.get(&c, &format!("/api/projects/{pid}/versions/{snapshot}")).await;
    assert_eq!(node_field(&old["architecture"], "app-dc2", "name"), "saved while evaluating", "the snapshot is of the draft that was replaced");

    // 3. a version cut meanwhile (the revision stays, the history moves): the draft is saved by now, so there is nothing to snapshot - no duplicate is made
    let (rev, mut unsaved) = project_rev(&app, &c, &pid).await;
    rename(&mut unsaved, "app-dc2", "not in any version");
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": unsaved, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let second = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 42)]).await;
    let (s, v) = while_the_apply_waits(&app, &pid, Meanwhile::CutVersion, app.post(&c, &format!("/api/proposals/{second}/apply"), json!({}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(version_messages(&app, &c, &pid).await, ["AI: tune", "cut meanwhile", "AI: tune", "Before AI change", "Created"], "the version cut meanwhile is the snapshot");
    let (_, now) = project_rev(&app, &c, &pid).await;
    assert_eq!(node_field(&now, "app-dc1", "props")["replicas"], json!(42));

    // 4. rejected by somebody else meanwhile (the revision does not move): the apply must not bring a rejected proposal back
    let third = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 43)]).await;
    let (s, v) = while_the_apply_waits(&app, &pid, Meanwhile::Reject(&third), app.post(&c, &format!("/api/proposals/{third}/apply"), json!({}))).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("proposal_closed")), "{v}");
    let (_, now) = project_rev(&app, &c, &pid).await;
    assert_eq!(node_field(&now, "app-dc1", "props")["replicas"], json!(42), "the rejected change is not in the draft");
    app.cleanup().await;
}

#[tokio::test]
async fn apply_is_exactly_once_and_survives_concurrent_saves() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;

    // (a) the same proposal applied twice at once: one wins, the other is told it is closed, and there is one AI version
    let pid = new_project(&app, &c, "twice", &arch).await;
    let proposal = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 41)]).await;
    let path = format!("/api/proposals/{proposal}/apply");
    let (one, two) = tokio::join!(app.post(&c, &path, json!({})), app.post(&c, &path, json!({})));
    let mut got = vec![(one.0.as_u16(), one.1["error"]["code"].as_str().map(String::from)), (two.0.as_u16(), two.1["error"]["code"].as_str().map(String::from))];
    got.sort();
    assert_eq!(got, vec![(200, None), (409, Some("proposal_closed".to_string()))], "{one:?} {two:?}");
    assert_eq!(version_messages(&app, &c, &pid).await.iter().filter(|m| m.starts_with("AI:")).count(), 1);

    // (b) operations applied while four people save: whatever the interleaving, nobody's save is lost, the history has no gaps,
    //     and the change is in the draft exactly when the proposal says it was applied
    let pid = new_project(&app, &c, "busy", &arch).await;
    let proposal = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 44)]).await;
    let others: Vec<String> = arch["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap().to_string()).filter(|id| id != "app-dc1").take(4).collect();
    let saver = |i: usize| {
        let (app, c, pid, node) = (&app, c.clone(), pid.clone(), others[i].clone());
        async move {
            for _ in 0..50 {
                let (rev, mut a) = project_rev(app, &c, &pid).await;
                rename(&mut a, &node, &format!("saved by {i}"));
                let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": a, "expectedRev": rev})).await;
                if s == StatusCode::OK {
                    return;
                }
                assert_eq!(s, StatusCode::CONFLICT, "{v}");
            }
            panic!("save {i} never got through");
        }
    };
    let apply_path = format!("/api/proposals/{proposal}/apply");
    let (applied, _) = tokio::join!(app.post(&c, &apply_path, json!({})), futures::future::join_all((0..4).map(saver)));
    assert!(matches!(applied.0.as_u16(), 200 | 409), "{applied:?}");
    let (_, now) = project_rev(&app, &c, &pid).await;
    for (i, node) in others.iter().enumerate() {
        assert_eq!(node_field(&now, node, "name"), &json!(format!("saved by {i}")), "save {i} was lost");
    }
    let (_, pr) = app.get(&c, &format!("/api/proposals/{proposal}")).await;
    let in_draft = node_field(&now, "app-dc1", "props")["replicas"] == json!(44);
    assert_eq!(pr["status"] == "applied", in_draft, "applied={} but the change is {}in the draft", pr["status"], if in_draft { "" } else { "not " });
    assert_eq!(applied.0 == StatusCode::OK, in_draft, "{applied:?}");
    let (head, newest, oldest, count): (i32, i32, i32, i64) =
        sqlx::query_as("select p.head_version, max(v.n), min(v.n), count(*) from projects p join versions v on v.project_id = p.id where p.id = $1 group by p.head_version")
            .bind(uuid::Uuid::parse_str(&pid).unwrap())
            .fetch_one(&app.state.pool)
            .await
            .unwrap();
    assert_eq!((head, oldest, count), (newest, 1, newest as i64), "the head is the newest version and the numbers have no gaps");

    // (c) a replacement reviewed at an older revision is a conflict, with the revision to review again
    let pid = new_project(&app, &c, "replace", &arch).await;
    let (rev, base) = project_rev(&app, &c, &pid).await;
    let mut replacement = base.clone();
    rename(&mut replacement, "app-dc1", "replaced");
    let replace: uuid::Uuid = sqlx::query_scalar("insert into proposals (project_id, base_rev, change, rationale) values ($1, $2, $3, 'replace') returning id")
        .bind(uuid::Uuid::parse_str(&pid).unwrap())
        .bind(rev)
        .bind(json!({"mode": "replace", "architecture": replacement}))
        .fetch_one(&app.state.pool)
        .await
        .unwrap();
    let mut meanwhile = base.clone();
    rename(&mut meanwhile, "app-dc2", "meanwhile");
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": meanwhile, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app.post(&c, &format!("/api/proposals/{replace}/apply"), json!({"expectedRev": rev})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("conflict")), "{v}");
    assert_eq!(v["error"]["details"]["currentRev"], rev + 1);
    let (s, v) = app.post(&c, &format!("/api/proposals/{replace}/apply"), json!({"expectedRev": rev + 1})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, now) = project_rev(&app, &c, &pid).await;
    assert_eq!(node_field(&now, "app-dc1", "name"), "replaced");
    app.cleanup().await;
}

/// Projects that were deleted are gone: nothing about them is readable or changeable through any endpoint, whatever id it is reached by
/// (the project, its job, its proposal, its accepted risk). The control run on a live twin proves each call is well formed and the route exists,
/// so the router's own "no such endpoint" 404 cannot make a row pass.
#[tokio::test]
async fn soft_deleted_projects_are_gone_for_every_endpoint() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;

    struct Twin {
        pid: String,
        job: uuid::Uuid,
        reject: String,
        apply: String,
        waiver: uuid::Uuid,
    }
    let mut twins = vec![];
    for name in ["alive", "deleted"] {
        let pid = new_project(&app, &c, name, &arch).await;
        let pid_u = uuid::Uuid::parse_str(&pid).unwrap();
        let job: uuid::Uuid =
            sqlx::query_scalar("insert into ai_jobs (project_id, kind, status, instance) values ($1, 'chat', 'running', 'other') returning id").bind(pid_u).fetch_one(&app.state.pool).await.unwrap();
        let waiver: uuid::Uuid =
            sqlx::query_scalar("insert into waivers (project_id, fingerprint, rule_id, reason, created_by) values ($1, 'X-1:a', 'X-1', 'a sufficiently long justification', 'test') returning id")
                .bind(pid_u)
                .fetch_one(&app.state.pool)
                .await
                .unwrap();
        sqlx::query("insert into chat_messages (project_id, role, content) values ($1, 'user', 'hello')").bind(pid_u).execute(&app.state.pool).await.unwrap();
        let reject = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 41)]).await;
        let apply = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 42)]).await;
        twins.push(Twin { pid, job, reject, apply, waiver });
    }
    let [alive, gone] = <[Twin; 2]>::try_from(twins).ok().unwrap();

    // (label, method, path, body, what the 404 must speak about)
    let table = |t: &Twin| -> Vec<(&'static str, Method, String, Option<Value>, &'static str)> {
        let (p, arch) = (&t.pid, &arch);
        vec![
            ("restore a version", Method::POST, format!("/api/projects/{p}/versions/1/restore"), Some(json!({"expectedRev": 1})), "project"),
            ("list versions", Method::GET, format!("/api/projects/{p}/versions"), None, "project"),
            ("read a version", Method::GET, format!("/api/projects/{p}/versions/1"), None, "project"),
            ("diff", Method::GET, format!("/api/projects/{p}/diff?from=1&to=1"), None, "project"),
            ("read the chat", Method::GET, format!("/api/projects/{p}/chat"), None, "project"),
            ("clear the chat", Method::DELETE, format!("/api/projects/{p}/chat"), None, "project"),
            ("read a job", Method::GET, format!("/api/ai/jobs/{}", t.job), None, "job"),
            ("cancel a job", Method::POST, format!("/api/ai/jobs/{}/cancel", t.job), Some(json!({})), "job"),
            ("read a proposal", Method::GET, format!("/api/proposals/{}", t.reject), None, "proposal"),
            ("reject a proposal", Method::POST, format!("/api/proposals/{}/reject", t.reject), Some(json!({})), "proposal"),
            ("apply a proposal", Method::POST, format!("/api/proposals/{}/apply", t.apply), Some(json!({})), "project"),
            ("revoke an accepted risk", Method::DELETE, format!("/api/projects/{p}/waivers/{}", t.waiver), None, "project"),
            ("evaluate with the project's accepted risks", Method::POST, "/api/evaluate".into(), Some(json!({"architecture": arch, "projectId": p})), "project"),
            ("recommendations with the project's accepted risks", Method::POST, "/api/recommendations".into(), Some(json!({"architecture": arch, "projectId": p})), "project"),
            ("preview operations with the project's accepted risks", Method::POST, "/api/ops/apply".into(), Some(json!({"architecture": arch, "ops": [], "preview": true, "projectId": p})), "project"),
            ("export", Method::GET, format!("/api/projects/{p}/export/json"), None, "project"),
        ]
    };

    for (label, method, path, body, _) in table(&alive) {
        let (s, _, v) = app.request(method, &path, body, Some(c.clone()), &[]).await;
        assert_eq!(s, StatusCode::OK, "control, {label} on the live project: {v}");
    }

    let (s, v) = app.delete(&c, &format!("/api/projects/{}", gone.pid)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    for (label, method, path, body, noun) in table(&gone) {
        let (s, _, v) = app.request(method, &path, body, Some(c.clone()), &[]).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{label} on a deleted project: {v}");
        assert_eq!(v["error"]["code"], "not_found", "{label}: {v}");
        let message = v["error"]["message"].as_str().unwrap();
        assert!(message.contains(noun) && message != "no such endpoint", "{label}: the answer speaks about the {noun}, not about the router: {message}");
    }
    // a project nobody ever made is as absent as a deleted one (the lists used to answer 200 with nothing in them)
    let nobody = uuid::Uuid::new_v4();
    for path in [format!("/api/projects/{nobody}/versions"), format!("/api/projects/{nobody}/chat")] {
        let (s, v) = app.get(&c, &path).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")), "{path}: {v}");
    }

    // and the refused calls changed nothing
    let pool = &app.state.pool;
    let (cancel,): (bool,) = sqlx::query_as("select cancel_requested from ai_jobs where id = $1").bind(gone.job).fetch_one(pool).await.unwrap();
    assert!(!cancel, "the job of a deleted project was not cancelled");
    for p in [&gone.reject, &gone.apply] {
        let (st,): (String,) = sqlx::query_as("select status from proposals where id = $1::uuid").bind(p).fetch_one(pool).await.unwrap();
        assert_eq!(st, "pending", "the proposal was not decided");
    }
    let (messages,): (i64,) = sqlx::query_as("select count(*) from chat_messages where project_id = $1::uuid").bind(&gone.pid).fetch_one(pool).await.unwrap();
    assert_eq!(messages, 1, "the chat was not cleared");
    let (revoked,): (bool,) = sqlx::query_as("select revoked_at is not null from waivers where id = $1").bind(gone.waiver).fetch_one(pool).await.unwrap();
    assert!(!revoked, "the accepted risk was not revoked");
    let (rev, versions): (i64, i64) =
        sqlx::query_as("select p.draft_rev, (select count(*) from versions v where v.project_id = p.id) from projects p where p.id = $1::uuid").bind(&gone.pid).fetch_one(pool).await.unwrap();
    assert_eq!((rev, versions), (1, 1), "the draft and the history are untouched");
    app.cleanup().await;
}

/// Restore used to overwrite the draft with a version and keep no copy of it: every unsaved or unversioned edit was gone for good.
#[tokio::test]
async fn restore_snapshots_the_draft_first() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    let pid = new_project(&app, &c, "restore", &arch).await;

    // a hand edit that is in no version
    let (rev, mut edited) = project_rev(&app, &c, &pid).await;
    rename(&mut edited, "app-dc2", "hand edit");
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": edited, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // a restore against a revision that moved is refused and writes nothing
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions/1/restore"), json!({"expectedRev": rev})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("conflict")), "{v}");
    assert_eq!(v["error"]["details"]["currentRev"], rev + 1);
    assert_eq!(version_messages(&app, &c, &pid).await.len(), 1);

    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions/1/restore"), json!({"expectedRev": rev + 1})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["rev"].as_i64(), v["snapshot"].as_i64()), (Some(rev + 2), Some(2)), "{v}");
    let versions = version_messages(&app, &c, &pid).await;
    assert_eq!(versions, vec!["Before restore of v1".to_string(), "Created".to_string()], "one version more");
    let (_, saved) = app.get(&c, &format!("/api/projects/{pid}/versions/2")).await;
    assert_eq!(node_field(&saved["architecture"], "app-dc2", "name"), "hand edit", "the pre-restore draft can be restored again");
    let (_, v1) = app.get(&c, &format!("/api/projects/{pid}/versions/1")).await;
    let (_, now) = project_rev(&app, &c, &pid).await;
    assert_eq!(now, v1["architecture"], "the draft is version 1");
    let (_, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
    assert_eq!(p["project"]["headVersion"], 2);

    // a draft that is already the newest version has nothing to save
    let (_, v) = app.post(&c, &format!("/api/projects/{pid}/versions"), json!({"message": "kept"})).await;
    assert_eq!(v["n"], 3);
    let (rev, _) = project_rev(&app, &c, &pid).await;
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions/1/restore"), json!({"expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["snapshot"], Value::Null, "{v}");
    assert_eq!(version_messages(&app, &c, &pid).await.len(), 3);

    // the history stays within `limits.maxVersionsPerProject`, snapshots and AI changes included
    set(&app, &c, json!({"limits.maxVersionsPerProject": 10})).await;
    for i in 0..12 {
        let (rev, mut a) = project_rev(&app, &c, &pid).await;
        rename(&mut a, "app-dc2", &format!("edit {i}"));
        let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": a, "expectedRev": rev})).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let (_, list) = app.get(&c, &format!("/api/projects/{pid}/versions")).await;
        let newest = list["versions"][0]["n"].as_i64().unwrap();
        let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions/{newest}/restore"), json!({"expectedRev": rev + 1})).await;
        assert_eq!(s, StatusCode::OK, "restore {i}: {v}");
    }
    assert_eq!(version_messages(&app, &c, &pid).await.len(), 10, "twelve snapshots did not grow the history past the limit");
    let (rev, mut a) = project_rev(&app, &c, &pid).await;
    rename(&mut a, "app-dc2", "before an AI change");
    let (s, _) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": a, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK);
    let proposal = pending_proposal(&app, &pid, vec![replicas_of("app-dc1", 41)]).await;
    let (s, v) = app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(version_messages(&app, &c, &pid).await.len(), 10, "an AI change adds two versions and the oldest two go");
    app.cleanup().await;
}

/// An item that is valid, with a setting the validator accepts and PostgreSQL refuses (U+0000 cannot be stored in jsonb): the import used to
/// commit the item, then fail on the setting, and answer with the database's own words. The setting is a `json` one: every text setting now has a
/// format that refuses a control character before the database is asked (the first import below), so only a JSON value still reaches it.
#[tokio::test]
async fn config_import_is_atomic() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let (_, before) = app.get(&c, "/api/admin/settings").await;
    let name_before = before["settings"].as_array().unwrap().iter().find(|s| s["key"] == "app.name").unwrap()["value"].clone();
    let category = json!({"id": "zz-import", "order": 99, "name": {"ru": "Проба", "en": "Probe"}});

    let (s, v) = app.post(&c, "/api/admin/config-import", json!({"items": [{"kind": "catalog.category", "id": "zz-import", "data": category}], "settings": {"app.name": "x\u{0}y"}})).await;
    assert_eq!((s, v["applied"].as_bool()), (StatusCode::OK, Some(false)), "{v}");
    assert!(v["problems"][0].as_str().unwrap().starts_with("app.name:"), "a control character in a text setting is refused by its format, before anything is written: {v}");

    let (s, v) =
        app.post(&c, "/api/admin/config-import", json!({"items": [{"kind": "catalog.category", "id": "zz-import", "data": category}], "settings": {"ui.theme": {"--accent": "x\u{0}y"}}})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_setting")), "{v}");
    assert!(!v.to_string().contains("error returned from database"), "the database's words stay in the log: {v}");
    assert_eq!(v["error"]["details"]["key"], "ui.theme", "{v}");

    let (_, list) = app.get(&c, "/api/admin/config/catalog.category").await;
    assert!(list["items"].as_array().unwrap().iter().all(|i| i["id"] != "zz-import"), "the item of a failed import is not there");
    let (_, after) = app.get(&c, "/api/admin/settings").await;
    assert_eq!(after["rev"], before["rev"], "the configuration revision did not move");
    assert_eq!(after["settings"].as_array().unwrap().iter().find(|s| s["key"] == "app.name").unwrap()["value"], name_before);
    let (n,): (i64,) = sqlx::query_as("select count(*) from audit_log where action = 'config.import'").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(n, 0, "a refused import is not logged as done");

    // the same import without the bad setting goes through, in one piece
    let (s, v) = app.post(&c, "/api/admin/config-import", json!({"items": [{"kind": "catalog.category", "id": "zz-import", "data": category}], "settings": {"app.name": "Imported"}})).await;
    assert_eq!((s, v["applied"].as_bool()), (StatusCode::OK, Some(true)), "{v}");
    let (_, list) = app.get(&c, "/api/admin/config/catalog.category").await;
    assert!(list["items"].as_array().unwrap().iter().any(|i| i["id"] == "zz-import"));
    let (_, after) = app.get(&c, "/api/admin/settings").await;
    assert_eq!(after["settings"].as_array().unwrap().iter().find(|s| s["key"] == "app.name").unwrap()["value"], "Imported");
    assert!(after["rev"].as_i64() > before["rev"].as_i64());
    app.cleanup().await;
}

/// Creating an item checks its id and refuses to add a second document to a kind that has one; an import is the same write by another door.
#[tokio::test]
async fn config_import_enforces_the_same_ids_as_create() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let category = |id: &str| json!({"id": id, "order": 99, "name": {"ru": "Проба", "en": "Probe"}});
    let import = |items: Value| {
        let (app, c) = (&app, c.clone());
        async move { app.post(&c, "/api/admin/config-import", json!({"items": items})).await }
    };
    let count = |kind: &str| {
        let (app, c, kind) = (&app, c.clone(), kind.to_string());
        async move { app.get(&c, &format!("/api/admin/config/{kind}")).await.1["items"].as_array().unwrap().len() }
    };
    let categories = count("catalog.category").await;
    let (engines, all) = (count("engine").await, count("catalog.common_props").await);

    let too_long = "x".repeat(81);
    let bad = [
        ("a space", json!([{"kind": "catalog.category", "id": "bad id", "data": category("bad id")}])),
        ("a path", json!([{"kind": "catalog.category", "id": "a/../b", "data": category("a/../b")}])),
        ("130 characters", json!([{"kind": "catalog.category", "id": "y".repeat(130), "data": category(&"y".repeat(130))}])),
        ("81 characters", json!([{"kind": "catalog.category", "id": too_long, "data": category(&too_long)}])),
        ("a second engine document", json!([{"kind": "engine", "id": "zzz", "data": {}}])),
    ];
    for (what, items) in bad {
        let (s, v) = import(items).await;
        assert_eq!((s, v["ok"].as_bool(), v["applied"].as_bool()), (StatusCode::OK, Some(false), Some(false)), "{what}: {v}");
        assert!(v["problems"].as_array().unwrap().len() == 1 && v["problems"][0].as_str().unwrap().contains(':'), "{what}: the problem names the item: {v}");
    }
    assert_eq!((count("catalog.category").await, count("engine").await, count("catalog.common_props").await), (categories, engines, all), "nothing was written");

    // the documents that exist can be imported over themselves
    for (kind, id) in [("engine", "engine"), ("catalog.common_props", "common"), ("placement.settings", "placement")] {
        let (_, list) = app.get(&c, &format!("/api/admin/config/{kind}")).await;
        let data = list["items"][0]["data"].clone();
        let (s, v) = import(json!([{"kind": kind, "id": id, "data": data}])).await;
        assert_eq!((s, v["applied"].as_bool()), (StatusCode::OK, Some(true)), "{kind}: {v}");
    }

    // a new item goes after the built-in ones, as when it is created by hand, and the order survives a round trip
    for id in ["zz_first", "aa_second"] {
        let (s, v) = app.post(&c, "/api/admin/config/catalog.category", json!({"id": id, "data": category(id)})).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let order = |list: &Value| list["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let (_, list) = app.get(&c, "/api/admin/config/catalog.category").await;
    let by_hand = order(&list);
    assert_eq!(&by_hand[by_hand.len() - 2..], ["zz_first", "aa_second"]);
    let (_, export) = app.get(&c, "/api/admin/config-export").await;
    for id in ["zz_first", "aa_second"] {
        assert_eq!(app.delete(&c, &format!("/api/admin/config/catalog.category/{id}")).await.0, StatusCode::OK);
    }
    let (s, v) = app.post(&c, "/api/admin/config-import", json!({"items": export["items"], "settings": export["settings"]})).await;
    assert_eq!((s, v["applied"].as_bool()), (StatusCode::OK, Some(true)), "{v}");
    let (_, list) = app.get(&c, "/api/admin/config/catalog.category").await;
    assert_eq!(order(&list), by_hand, "imported after the built-in items, in the order they were exported");
    app.cleanup().await;
}

/// What the administrator exports restores an instance: a changed setting, a changed built-in rule and the custom items come back.
#[tokio::test]
async fn config_export_then_import_round_trips() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    set(&app, &c, json!({"limits.maxNodes": 777, "llm.apiKey": "sk-or-v1-never-in-an-export"})).await;
    let (_, list) = app.get(&c, "/api/admin/config/rule").await;
    let mut rule = list["items"].as_array().unwrap().iter().find(|i| i["id"] == "HA-CACHE-01").unwrap()["data"].clone();
    rule["enabled"] = json!(false);
    assert_eq!(app.put(&c, "/api/admin/config/rule/HA-CACHE-01", json!({"data": rule})).await.0, StatusCode::OK);
    for id in ["zz_first", "aa_second"] {
        let (s, v) = app.post(&c, "/api/admin/config/catalog.category", json!({"id": id, "data": {"id": id, "order": 99, "name": {"ru": "Проба", "en": "Probe"}}})).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let (s, first) = app.get(&c, "/api/admin/config-export").await;
    assert_eq!(s, StatusCode::OK);
    assert!(!first.to_string().contains("never-in-an-export"), "secrets are not exported");
    assert_eq!(first["settings"]["limits.maxNodes"], 777);

    // wreck the instance
    for id in ["zz_first", "aa_second"] {
        assert_eq!(app.delete(&c, &format!("/api/admin/config/catalog.category/{id}")).await.0, StatusCode::OK);
    }
    assert_eq!(app.post(&c, "/api/admin/config/rule/HA-CACHE-01/reset", json!({})).await.0, StatusCode::OK);
    set(&app, &c, json!({"limits.maxNodes": 500})).await;
    let (_, wrecked) = app.get(&c, "/api/admin/config-export").await;
    assert_ne!(wrecked["items"], first["items"]);

    // a dry run says what would happen and changes nothing
    let body = json!({"items": first["items"], "settings": first["settings"]});
    let (s, v) = app.post(&c, "/api/admin/config-import", json!({"items": first["items"], "settings": first["settings"], "dryRun": true})).await;
    assert_eq!((s, v["ok"].as_bool(), v["applied"].as_bool()), (StatusCode::OK, Some(true), Some(false)), "{v}");
    let (_, still) = app.get(&c, "/api/admin/config-export").await;
    assert_eq!((&still["items"], &still["settings"]), (&wrecked["items"], &wrecked["settings"]));

    let (s, v) = app.post(&c, "/api/admin/config-import", body).await;
    assert_eq!((s, v["applied"].as_bool()), (StatusCode::OK, Some(true)), "{v}");
    let (_, second) = app.get(&c, "/api/admin/config-export").await;
    assert_eq!(second["items"], first["items"], "the items and their order come back");
    assert_eq!(second["settings"], first["settings"]);
    // the restored rule is in force
    let arch = demo_architecture(&app, &c).await;
    let (_, rep) = app.post(&c, "/api/evaluate", json!({"architecture": arch})).await;
    assert!(rep["rules"].as_array().unwrap().iter().all(|r| r["ruleId"] != "HA-CACHE-01"), "the disabled rule stays disabled");
    app.cleanup().await;
}

// ---------------------------------------------------------------------------------------------
// One validation gate: a number outside its range is refused when it is written, reported when it is read, and never a 500.
// ---------------------------------------------------------------------------------------------

fn set_node_prop(arch: &mut Value, id: &str, key: &str, v: Value) {
    arch["nodes"].as_array_mut().unwrap().iter_mut().find(|n| n["id"] == id).unwrap_or_else(|| panic!("node {id}"))["props"][key] = v;
}

#[tokio::test]
async fn out_of_range_props_are_rejected_not_stored() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    let pid = new_project(&app, &c, "range", &arch).await;
    let (rev, _) = project_rev(&app, &c, &pid).await;

    // a million million instances of a service: the catalog allows a hundred thousand
    let mut bad = arch.clone();
    set_node_prop(&mut bad, "app-dc1", "replicas", json!(1_000_000_000_000u64));
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": bad, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "invalid_architecture");
    let first = &v["error"]["details"]["issues"][0];
    assert_eq!((first["code"].as_str(), first["subject"].as_str()), (Some("prop_out_of_range"), Some("app-dc1")), "{v}");
    let said = v["error"]["message"].as_str().unwrap();
    assert!(said.contains("app-dc1") && said.contains("replicas") && said.contains("1 to 100000"), "the message names the node, the prop and the range: {said}");
    // nothing was stored: the revision is the one it was, and the project still evaluates
    let (rev_after, stored) = project_rev(&app, &c, &pid).await;
    assert_eq!(rev_after, rev);
    assert_eq!(stored, arch);
    let (s, rep) = app.post(&c, "/api/evaluate", json!({"architecture": stored, "projectId": pid})).await;
    assert_eq!(s, StatusCode::OK, "{rep}");

    // the same refusal for a number that is not one, a requirement outside its domain, and the other ways in
    let mut text = arch.clone();
    set_node_prop(&mut text, "app-dc1", "replicas", json!("many"));
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": text, "expectedRev": rev})).await;
    assert_eq!((s, v["error"]["details"]["issues"][0]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("prop_type")), "{v}");
    let mut slo = arch.clone();
    slo["requirements"]["availabilitySlo"] = json!(150);
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": slo, "expectedRev": rev})).await;
    assert_eq!((s, v["error"]["details"]["issues"][0]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("requirement_out_of_range")), "{v}");
    let (s, v) = app.post(&c, "/api/projects", json!({"name": "born wrong", "architecture": bad})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_architecture")), "{v}");
    let ops = json!([{"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 1e12}}]);
    let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": arch, "ops": ops})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("ops_invalid")), "{v}");
    assert_eq!(v["error"]["details"]["issues"][0]["code"], "prop_out_of_range");
    let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": arch, "ops": ops, "preview": true})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("ops_invalid")), "{v}");

    // a number inside the range is saved, at its edge too
    let mut edge = arch.clone();
    set_node_prop(&mut edge, "app-dc1", "replicas", json!(100_000));
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": edge, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // what the generator makes is held to the same rules, and a design for a big load fits them
    let (s, v) = app.post(&c, "/api/projects", json!({"name": "wizard", "placement": {"pattern": "n-sites", "sites": 3, "requirements": {"rpsPeak": 1_000_000}}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn stored_out_of_range_draft_still_opens() {
    // projects saved before the ranges were enforced: a draft with a million million instances used to answer every evaluation, version,
    // export and accepted risk of its project with a 500 ("attempt to add with overflow"); now it opens and says what is wrong with it
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    let pid = new_project(&app, &c, "legacy", &arch).await;
    let mut legacy = arch.clone();
    set_node_prop(&mut legacy, "app-dc1", "replicas", json!(1_000_000_000_000u64));
    sqlx::query("update projects set draft = $1 where id = $2").bind(&legacy).bind(uuid::Uuid::parse_str(&pid).unwrap()).execute(&app.state.pool).await.unwrap();

    let (s, p) = app.get(&c, &format!("/api/projects/{pid}")).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    let (s, rep) = app.post(&c, "/api/evaluate", json!({"architecture": p["architecture"], "projectId": pid})).await;
    assert_eq!(s, StatusCode::OK, "{rep}");
    let issue = rep["issues"].as_array().unwrap().iter().find(|i| i["code"] == "prop_out_of_range").unwrap_or_else(|| panic!("the report says it: {}", rep["issues"]));
    assert_eq!((issue["level"].as_str(), issue["subject"].as_str()), (Some("warning"), Some("app-dc1")));
    // the engine counts the instances at the most a service may have, it does not wrap them
    let cap = rep["capacity"].as_array().unwrap().iter().find(|r| r["nodes"].as_array().is_some_and(|n| n.iter().any(|x| x == "app-dc1"))).expect("a capacity row for the service");
    assert!(cap["instances"].as_u64().unwrap() <= 200_000, "{cap}");
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/versions"), json!({"message": "as it is"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    for format in ["kubernetes", "json", "adr", "terraform"] {
        let (s, v) = app.get(&c, &format!("/api/projects/{pid}/export/{format}")).await;
        assert_eq!(s, StatusCode::OK, "{format}: {v}");
    }
    let finding = rep["findings"].as_array().unwrap().iter().find(|f| f["ruleId"] == "DR-KFK-01").expect("the demo's known defect");
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/waivers"), json!({"fingerprint": finding["id"], "reason": "accepted for the test of a legacy project"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app.post(&c, "/api/recommendations", json!({"architecture": p["architecture"], "projectId": pid})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app.get(&c, &format!("/api/projects/{pid}/diff?from=1&to=2")).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // saving is where it is refused, and putting it right is saved
    let (rev, _) = project_rev(&app, &c, &pid).await;
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": p["architecture"], "expectedRev": rev})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_architecture")), "{v}");
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": arch, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn compute_endpoints_stay_lenient() {
    // the editor sends what is typed to /evaluate, /simulate and /recommendations on every pause: a half-typed number is a warning
    // in the report, not a banner of errors and not a 500
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    for typed in [json!(0), json!(-3), json!(5_000_000), json!(0.5)] {
        let mut a = arch.clone();
        set_node_prop(&mut a, "app-dc1", "replicas", typed.clone());
        let (s, rep) = app.post(&c, "/api/evaluate", json!({"architecture": a})).await;
        assert_eq!(s, StatusCode::OK, "{typed}: {rep}");
        let warned = rep["issues"].as_array().unwrap().iter().any(|i| i["code"] == "prop_out_of_range" && i["level"] == "warning");
        assert!(warned, "{typed}: {}", rep["issues"]);
        let (s, v) = app.post(&c, "/api/simulate", json!({"architecture": a, "scenario": {"type": "zone", "zoneId": "dc1"}})).await;
        assert_eq!(s, StatusCode::OK, "{typed}: {v}");
        let (s, v) = app.post(&c, "/api/recommendations", json!({"architecture": a})).await;
        assert_eq!(s, StatusCode::OK, "{typed}: {v}");
    }
    // a requirement outside its domain is the same: said, not refused
    let mut slo = arch.clone();
    slo["requirements"]["availabilitySlo"] = json!(150);
    let (s, rep) = app.post(&c, "/api/evaluate", json!({"architecture": slo})).await;
    assert_eq!(s, StatusCode::OK, "{rep}");
    assert!(rep["issues"].as_array().unwrap().iter().any(|i| i["code"] == "requirement_out_of_range" && i["level"] == "warning"), "{}", rep["issues"]);
    // while the structure of the design is not negotiable on the endpoints that need one
    let mut dangling = arch.clone();
    dangling["edges"][0]["target"] = json!("nowhere");
    let (s, v) = app.post(&c, "/api/simulate", json!({"architecture": dangling, "scenario": {"type": "none"}})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_architecture")), "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn operations_on_a_design_with_a_number_out_of_range_apply_only_when_they_put_the_number_right() {
    // docs/API.md: the base of /ops/apply is looked at (parsed leniently), but what the operations make of it is written, so it is held to the
    // ranges: an operation that leaves the number where it was is refused, naming it, and one that puts it right is applied. A recommended fix
    // of some other finding therefore cannot be previewed on such a design until the number is put right
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    for typed in [json!(1_000_000_000_000u64), json!(0), json!(-3), json!(5_000_000), json!(0.5)] {
        let mut legacy = arch.clone();
        set_node_prop(&mut legacy, "app-dc1", "replicas", typed.clone());
        let elsewhere = json!([{"op": "update_node", "id": "kafka-dc1", "set": {"props.replicas": 4}}]);
        for preview in [false, true] {
            let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": legacy, "ops": elsewhere, "preview": preview})).await;
            assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("ops_invalid")), "{typed} preview={preview}: {v}");
            let issues = v["error"]["details"]["issues"].as_array().unwrap_or_else(|| panic!("{v}"));
            let it = issues.iter().find(|i| i["code"] == "prop_out_of_range").unwrap_or_else(|| panic!("{typed}: {v}"));
            assert_eq!((it["level"].as_str(), it["subject"].as_str()), (Some("error"), Some("app-dc1")), "{typed}: {it}");
        }
        let right = json!([{"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 32}}]);
        for preview in [false, true] {
            let (s, v) = app.post(&c, "/api/ops/apply", json!({"architecture": legacy, "ops": right, "preview": preview})).await;
            assert_eq!(s, StatusCode::OK, "{typed} preview={preview}: {v}");
            let after = v["architecture"]["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "app-dc1").unwrap()["props"]["replicas"].clone();
            assert_eq!(after, json!(32), "{typed} preview={preview}");
        }
    }
    app.cleanup().await;
}

#[tokio::test]
async fn a_catalog_item_with_a_range_that_allows_nothing_is_refused_when_the_administrator_saves_it() {
    // the ranges of the catalog are what the design check holds every written number to (ADR-0007), so a maximum below a minimum, or a default
    // outside the range, would make every write of a node of that kind fail: it is refused where it is made, naming the item
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let (_, list) = app.get(&c, "/api/admin/config/catalog.common_props").await;
    let item = list["items"][0].clone();
    let (id, common) = (item["id"].as_str().unwrap().to_string(), item["data"].clone());
    let with = |field: &str, v: Value| {
        let mut d = common.clone();
        d.as_array_mut().unwrap().iter_mut().find(|p| p["key"] == "replicas").unwrap()[field] = v;
        d
    };
    let url = format!("/api/admin/config/catalog.common_props/{id}");
    for (field, v, said) in [
        ("max", json!(0), "min 1 above max 0"),
        ("max", json!(2), "kind 'kubernetes' prop 'replicas' has default 3 outside its range"),
        ("default", json!(100_001), "default 100001 outside its range"),
    ] {
        let (s, e) = app.put(&c, &url, json!({"data": with(field, v.clone())})).await;
        assert_eq!((s, e["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_config")), "{field} = {v}: {e}");
        assert!(e["error"]["message"].as_str().unwrap().contains(said), "{field} = {v}: {e}");
    }
    // a kind that is created, or edited, with a range of its own that allows nothing
    let (_, kinds) = app.get(&c, "/api/admin/config/catalog.kind").await;
    let kubernetes = kinds["items"].as_array().unwrap().iter().find(|i| i["id"] == "kubernetes").unwrap()["data"].clone();
    let mut bad = kubernetes.clone();
    bad["props"].as_array_mut().unwrap().iter_mut().find(|p| p["key"] == "replicas").unwrap()["max"] = json!(0);
    let (s, e) = app.put(&c, "/api/admin/config/catalog.kind/kubernetes", json!({"data": bad})).await;
    assert_eq!((s, e["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_config")), "{e}");
    let mut mine = bad.clone();
    mine["id"] = json!("my-kubernetes");
    let (s, e) = app.post(&c, "/api/admin/config/catalog.kind", json!({"id": "my-kubernetes", "data": mine})).await;
    assert_eq!((s, e["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_config")), "{e}");
    // nothing of it was stored: the catalog is as it was, and designs are still written
    let arch = demo_architecture(&app, &c).await;
    let pid = new_project(&app, &c, "after the refusals", &arch).await;
    let (rev, _) = project_rev(&app, &c, &pid).await;
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": arch, "expectedRev": rev})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // a range that is sensible is a setting like any other, and takes effect at once
    let (s, v) = app.put(&c, &url, json!({"data": with("max", json!(40))})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let mut big = arch.clone();
    set_node_prop(&mut big, "app-dc1", "replicas", json!(41));
    let (rev, _) = project_rev(&app, &c, &pid).await;
    let (s, v) = app.put(&c, &format!("/api/projects/{pid}/draft"), json!({"architecture": big, "expectedRev": rev})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_architecture")), "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("1 to 40"), "{v}");
    app.cleanup().await;
}

/// A small deterministic generator (a linear congruential one): the same requests every run, and no new dependency.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
        &of[(self.next() % of.len() as u64) as usize]
    }
}

#[tokio::test]
async fn extreme_numbers_never_500() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    let hostile: Vec<Value> = vec![
        json!(-1),
        json!(0),
        json!(0.5),
        json!(4e9),
        json!(4294967295u64),
        json!(4294967296u64),
        json!(1e12),
        json!(1e18),
        json!(1e308),
        json!(1.7e308),
        json!("abc"),
        json!("NaN"),
        json!(true),
        Value::Null,
        json!([1]),
    ];
    let node_keys = ["replicas", "failoverReplicas", "vcpu", "ramGb", "diskGb", "rpsPerReplica", "hpaMin", "hpaMax", "votes", "scaleUpSeconds", "healthCheckSeconds", "failThreshold"];
    let req_keys = ["rpsPeak", "growthPercent", "availabilitySlo", "rtoSeconds", "rpoSeconds", "budgetMonthly", "siteFailures"];
    let scenarios = [
        json!({"type": "none"}),
        json!({"type": "zone", "zoneId": "dc1"}),
        json!({"type": "instance", "nodeId": "app-dc1", "count": 4294967295u64}),
        json!({"type": "node", "nodeId": "app-dc2"}),
        json!({"type": "sites", "zoneIds": ["dc1", "dc2"]}),
        json!({"type": "link", "a": "dc1", "b": "dc2"}),
    ];
    let node_ids: Vec<String> = arch["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap().to_string()).collect();
    let mut rng = Lcg(0x1B03);
    let (mut ok, mut refused, mut failed) = (0, 0, vec![]);
    for i in 0..240 {
        // one to three hostile values in a copy of the design
        let mut a = arch.clone();
        for _ in 0..=(rng.next() % 3) {
            let v = rng.pick(&hostile).clone();
            match rng.next() % 4 {
                0 | 1 => {
                    let id = rng.pick(&node_ids).clone();
                    set_node_prop(&mut a, &id, rng.pick(&node_keys), v);
                }
                2 => a["requirements"][*rng.pick(&req_keys)] = v,
                _ => {
                    let n = a["edges"].as_array().unwrap().len();
                    a["edges"][(rng.next() % n as u64) as usize]["props"]["fanout"] = v;
                }
            }
        }
        let (s, body) = match i % 8 {
            0..=2 => app.post(&c, "/api/evaluate", json!({"architecture": a})).await,
            3 | 4 => app.post(&c, "/api/simulate", json!({"architecture": a, "scenario": rng.pick(&scenarios)})).await,
            5 => app.post(&c, "/api/recommendations", json!({"architecture": a})).await,
            _ => {
                let ops = json!([{"op": "update_node", "id": rng.pick(&node_ids), "set": {format!("props.{}", rng.pick(&node_keys)): rng.pick(&hostile)}}]);
                app.post(&c, "/api/ops/apply", json!({"architecture": a, "ops": ops, "preview": i % 16 == 6})).await
            }
        };
        match s {
            StatusCode::OK => ok += 1,
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => refused += 1,
            other => failed.push(format!("#{i} {other}: {} {}", body["error"]["code"], a["requirements"])),
        }
    }
    eprintln!("extreme numbers: {ok} answered, {refused} refused, {} failed", failed.len());
    assert!(failed.is_empty(), "requests the server could not answer:\n{}", failed.join("\n"));
    assert!(ok > 120 && refused > 0, "the run is neither all accepted nor all refused: {ok} / {refused}");
    app.cleanup().await;
}

// ---------------------------------------------------------------------------------------------
// A scenario names what the design has: nothing is "up" because the failed thing does not exist.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn simulate_rejects_unknown_scenario_references() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let arch = demo_architecture(&app, &c).await;
    let ask = |scenario: Value| {
        let (c, arch) = (c.clone(), arch.clone());
        let app = &app;
        async move { app.post(&c, "/api/simulate", json!({"architecture": arch, "scenario": scenario})).await }
    };
    // (scenario, code, what the answer has to name: the id that is not there, or the reason a scenario is no failure)
    let refused: Vec<(Value, &str, &str)> = vec![
        (json!({"type": "zone", "zoneId": "dc9"}), "unknown_scenario_reference", "dc9"),
        (json!({"type": "node", "nodeId": "ghost"}), "unknown_scenario_reference", "ghost"),
        (json!({"type": "instance", "nodeId": "ghost", "count": 1}), "unknown_scenario_reference", "ghost"),
        (json!({"type": "unit", "unit": "no-such-cluster"}), "unknown_scenario_reference", "no-such-cluster"),
        (json!({"type": "geo", "geo": "atlantis"}), "unknown_scenario_reference", "atlantis"),
        (json!({"type": "link", "a": "dc1", "b": "dc9"}), "unknown_scenario_reference", "dc9"),
        (json!({"type": "link", "a": "dc1", "b": "dc1"}), "invalid_scenario", "link_to_itself"),
        (json!({"type": "isolate", "zoneIds": []}), "invalid_scenario", "no_zones"),
        (json!({"type": "sites", "zoneIds": []}), "invalid_scenario", "no_zones"),
        (json!({"type": "sites", "zoneIds": ["nope"]}), "unknown_scenario_reference", "nope"),
        (json!({"type": "sites", "zoneIds": ["dc1", "nope"]}), "unknown_scenario_reference", "nope"),
        (json!({"type": "combo", "items": []}), "invalid_scenario", "empty_combo"),
        (json!({"type": "combo", "items": [{"type": "zone", "zoneId": "dc1"}, {"type": "zone", "zoneId": "dc9"}]}), "unknown_scenario_reference", "dc9"),
        // an id that is a node where a zone is meant
        (json!({"type": "zone", "zoneId": "app-dc1"}), "unknown_scenario_reference", "app-dc1"),
        (json!({"type": "instance", "nodeId": "app-dc1", "count": 0}), "invalid_scenario", "count_out_of_range"),
        (json!({"type": "instance", "nodeId": "app-dc1", "count": 100001}), "invalid_scenario", "count_out_of_range"),
    ];
    for (sc, code, names) in &refused {
        let (s, v) = ask(sc.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{sc}: {v}");
        assert_eq!(v["error"]["code"], *code, "{sc}: {v}");
        if *code == "unknown_scenario_reference" {
            assert!(v["error"]["message"].as_str().unwrap().contains(names), "the message names '{names}': {v}");
            assert!(v["error"]["details"]["refs"].as_array().unwrap().iter().any(|r| r["id"] == *names), "{v}");
        } else {
            assert_eq!(v["error"]["details"]["reason"], *names, "{v}");
        }
    }
    // every name that is wrong is listed, not only the first
    let (_, v) = ask(json!({"type": "sites", "zoneIds": ["x1", "dc1", "x2"]})).await;
    let refs: Vec<&str> = v["error"]["details"]["refs"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(refs, vec!["x1", "x2"], "{v}");
    assert_eq!(v["error"]["details"]["refs"][0]["kind"], "zone");
    // a combo nested past the limit
    let mut deep = json!({"type": "zone", "zoneId": "dc1"});
    for _ in 0..6 {
        deep = json!({"type": "combo", "items": [deep]});
    }
    let (s, v) = ask(deep).await;
    assert_eq!((s, v["error"]["code"].as_str(), v["error"]["details"]["reason"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_scenario"), Some("too_deep")), "{v}");

    // and what is real is answered, at the edges too
    for sc in [
        json!({"type": "none"}),
        json!({"type": "zone", "zoneId": "dc1"}),
        json!({"type": "combo", "items": [{"type": "zone", "zoneId": "dc1"}, {"type": "node", "nodeId": "app-dc2"}]}),
        json!({"type": "instance", "nodeId": "app-dc1", "count": 1}),
        json!({"type": "instance", "nodeId": "app-dc1", "count": 100000}),
        json!({"type": "link", "a": "dc1", "b": "dc2"}),
        json!({"type": "unit", "unit": "pg"}),
    ] {
        let (s, v) = ask(sc.clone()).await;
        assert_eq!(s, StatusCode::OK, "{sc}: {v}");
    }
    // every failure the engine itself lists for a design is one it accepts, a geo label that is the id of a region zone included
    for template in ["two-dc-witness", "ten-sites-regions", "multi-region-pilot-light", "three-az-region"] {
        let (_, t) = app.get(&c, &format!("/api/templates/{template}")).await;
        let a = t["architecture"].clone();
        let (s, rep) = app.post(&c, "/api/evaluate", json!({"architecture": a})).await;
        assert_eq!(s, StatusCode::OK, "{template}: {rep}");
        let listed = rep["scenarios"].as_array().unwrap();
        assert!(!listed.is_empty());
        for sc in listed.iter().take(40) {
            let (s, v) = app.post(&c, "/api/simulate", json!({"architecture": a, "scenario": sc["scenario"]})).await;
            assert_eq!(s, StatusCode::OK, "{template} {}: {v}", sc["scenario"]);
        }
    }
    app.cleanup().await;
}

// ---------------------------------------------------------------------------------------------
// One shape for every error, and a missing file is a missing file.
// ---------------------------------------------------------------------------------------------

/// The codes API.md documents: the first column of its error table, and every other code in backticks.
fn documented_codes() -> std::collections::BTreeSet<String> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/API.md")).expect("docs/API.md");
    let table = text.split("### Errors").nth(1).and_then(|t| t.split("\n### ").next()).expect("the error table of API.md");
    let mut codes = std::collections::BTreeSet::new();
    for line in table.lines().filter(|l| l.starts_with('|')) {
        let first = line.split('|').nth(1).unwrap_or("");
        for part in first.split('`').skip(1).step_by(2) {
            codes.insert(part.to_string());
        }
    }
    codes
}

#[tokio::test]
async fn every_error_is_the_json_shape() {
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;
    let documented = documented_codes();
    assert!(documented.len() > 40, "{documented:?}");
    let big = "x".repeat(5 * 1024 * 1024);
    let evaluate = "/api/evaluate";
    // (method, path, body, content type, the status and the code that are expected)
    type Probe<'a> = (Method, String, Option<String>, Option<&'a str>, u16, &'a str);
    let probes: Vec<Probe> = vec![
        (Method::POST, evaluate.into(), Some("{\"architecture\": ".into()), Some("application/json"), 400, "invalid_json"),
        (Method::POST, evaluate.into(), Some("not json at all".into()), Some("application/json"), 400, "invalid_json"),
        (Method::POST, evaluate.into(), Some("{}".into()), Some("application/json"), 422, "invalid_body"),
        (Method::POST, "/api/ops/apply".into(), Some("{\"architecture\": {}, \"ops\": \"x\"}".into()), Some("application/json"), 422, "invalid_body"),
        (Method::POST, "/api/simulate".into(), Some("{\"architecture\": {}, \"scenario\": {\"type\": \"nope\"}}".into()), Some("application/json"), 422, "invalid_body"),
        (Method::POST, evaluate.into(), Some("{\"architecture\": {}}".into()), None, 415, "unsupported_media_type"),
        (Method::POST, evaluate.into(), Some("{\"architecture\": {}}".into()), Some("text/plain"), 415, "unsupported_media_type"),
        (Method::GET, "/api/projects/not-a-uuid".into(), None, None, 400, "invalid_path"),
        (Method::GET, format!("/api/projects/{pid}/versions/abc"), None, None, 400, "invalid_path"),
        (Method::GET, format!("/api/projects/{pid}/export/json?download=abc"), None, None, 400, "invalid_query"),
        (Method::GET, "/api/admin/audit?limit=many".to_string(), None, None, 400, "invalid_query"),
        (Method::POST, evaluate.into(), Some(format!("{{\"architecture\": {{}}, \"lang\": \"{big}\"}}")), Some("application/json"), 413, "payload_too_large"),
        (Method::DELETE, evaluate.into(), None, None, 405, "method_not_allowed"),
        (Method::PUT, "/api/projects".into(), None, None, 405, "method_not_allowed"),
        (Method::GET, "/api/no/such/endpoint".into(), None, None, 404, "not_found"),
    ];
    let mut shown = 0;
    for (method, path, body, ct, status, code) in probes {
        let (s, h, v) = app.raw(method.clone(), &path, body.as_deref(), ct, Some(c.as_str())).await;
        assert_eq!(s.as_u16(), status, "{method} {path}: {v}");
        let ctype = h.get(header::CONTENT_TYPE).and_then(|x| x.to_str().ok()).unwrap_or("");
        assert!(ctype.starts_with("application/json"), "{method} {path} answers {ctype:?}: {v}");
        assert_eq!(v["error"]["code"], code, "{method} {path}: {v}");
        assert!(v["error"]["message"].as_str().is_some_and(|m| !m.is_empty()), "{method} {path}: {v}");
        assert!(documented.contains(code), "{code} is not in the error table of docs/API.md");
        if status == 405 {
            let allow = h.get(header::ALLOW).and_then(|x| x.to_str().ok()).unwrap_or("");
            assert!(!allow.is_empty() && !allow.contains(method.as_str()), "{method} {path}: Allow is {allow:?}");
        }
        shown += 1;
    }
    assert_eq!(shown, 15);
    // where the body is wrong the answer says which field
    let (_, _, v) = app.raw(Method::POST, "/api/ops/apply", Some("{\"architecture\": {}, \"ops\": \"x\"}"), Some("application/json"), Some(c.as_str())).await;
    assert_eq!(v["error"]["details"]["field"], "ops", "{v}");
    let (_, _, v) = app.raw(Method::POST, "/api/simulate", Some("{\"architecture\": {}}"), Some("application/json"), Some(c.as_str())).await;
    assert_eq!(v["error"]["details"]["field"], "scenario", "{v}");
    // the endpoints without a session answer in the same shape as the ones with one
    let (s, h, v) = app.raw(Method::POST, evaluate, Some("{"), Some("application/json"), None).await;
    assert_eq!((s.as_u16(), v["error"]["code"].as_str()), (401, Some("unauthorized")), "{v}");
    assert!(h.get(header::CONTENT_TYPE).unwrap().to_str().unwrap().starts_with("application/json"));
    let (s, _, v) = app.raw(Method::POST, "/api/auth/login", Some("{\"email\": "), Some("application/json"), None).await;
    assert_eq!((s.as_u16(), v["error"]["code"].as_str()), (400, Some("invalid_json")), "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn every_error_code_is_documented() {
    // the codes are read off the source of the server: one added without a row in the table of docs/API.md fails here
    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = vec![];
    rust_files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let first_literal = |rest: &str| -> Option<String> {
        let lit = rest.trim_start().strip_prefix('"')?.split('"').next()?;
        (!lit.is_empty() && lit.chars().all(|c| c.is_ascii_lowercase() || c == '_')).then(|| lit.to_string())
    };
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for opener in ["ApiError::bad_request(", "ApiError::conflict(", "ApiError::too_many(", "ApiError::unavailable(", "ApiError::forbidden_code("] {
            for rest in text.split(opener).skip(1) {
                found.extend(first_literal(rest));
            }
        }
        // `ApiError::new(<status>, "code", …)`, and the same as `Self::new` inside error.rs where the helpers are
        let opener = if f.file_name().is_some_and(|n| n == "error.rs") { "Self::new(" } else { "ApiError::new(" };
        for rest in text.split(opener).skip(1) {
            if let Some((_, after_status)) = rest.split_once(',') {
                found.extend(first_literal(after_status));
            }
        }
    }
    let documented = documented_codes();
    let missing: Vec<&String> = found.iter().filter(|c| !documented.contains(*c)).collect();
    assert!(missing.is_empty(), "error codes the server says and docs/API.md does not list: {missing:?}");
    assert!(found.len() > 40, "only {} codes were found in the source: the scan is blind", found.len());
    for known in ["invalid_json", "unknown_scenario_reference", "payload_too_large", "internal", "not_found"] {
        assert!(found.contains(known), "the scan misses {known}");
    }
}

#[tokio::test]
async fn missing_asset_is_404_without_immutable_cache() {
    // a page that was built before a deploy asks for a chunk the new build no longer has: that is a 404, not the index page that every browser
    // would then keep for a year as if it were the chunk
    let web = temp_web_dir(&[("index.html", "<!doctype html><title>index</title>"), ("assets/app-ABC.js", "console.log('app')")]);
    let w = web.clone();
    let app = TestApp::with_boot(move |b| b.web_dir = w).await;
    let cache = |h: &axum::http::HeaderMap| h.get(header::CACHE_CONTROL).and_then(|x| x.to_str().ok()).unwrap_or("").to_string();

    let (s, h, body) = app.request(Method::GET, "/assets/app-ABC.js", None, None, &[]).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert!(cache(&h).contains("immutable") && cache(&h).contains("max-age=31536000"), "{}", cache(&h));

    let (s, h, body) = app.request(Method::GET, "/assets/gone-123.js", None, None, &[]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(cache(&h), "no-store", "a file that is not there is not cached");
    assert!(!body.to_string().contains("<title>index</title>"), "{body}");
    let (s, _, _) = app.request(Method::GET, "/assets/", None, None, &[]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // the pages of the application are the index page, and are asked about every time
    for route in ["/", "/projects/abc", "/some/spa/route"] {
        let (s, h, body) = app.request(Method::GET, route, None, None, &[]).await;
        assert_eq!(s, StatusCode::OK, "{route}");
        assert!(body.to_string().contains("<title>index</title>"), "{route}: {body}");
        assert_eq!(cache(&h), "no-cache", "{route}");
    }
    app.cleanup().await;
    let _ = std::fs::remove_dir_all(web);
}

// ------------------------------------------------------------------ authentication: what a sign-in answers, who it locks out, what it costs

/// Argon2 verifications done so far (`/metrics`). Every sign-in that reaches a password check costs exactly one, whatever the account is.
const PASSWORD_CHECKS: &str = "infra_auth_password_checks_total";

/// The key under which the server counts the failures of one account (a digest of the e-mail address, never the address itself).
fn account_key(email: &str) -> String {
    format!("login:fail:{}", &server::crypto::sha256_hex(email.trim().to_lowercase().as_bytes())[..32])
}

/// `n` calls at the same moment, each on its own task (the runtime of the test must be multi-threaded).
async fn concurrently<T, F, Fut>(app: &std::sync::Arc<TestApp>, n: usize, f: F) -> Vec<T>
where
    F: Fn(std::sync::Arc<TestApp>, usize) -> Fut,
    Fut: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let tasks: Vec<_> = (0..n).map(|i| tokio::spawn(f(app.clone(), i))).collect();
    let mut out = vec![];
    for t in tasks {
        out.push(t.await.unwrap());
    }
    out
}

async fn user_id(app: &TestApp, admin: &str, email: &str) -> String {
    let (_, users) = app.get(admin, "/api/admin/users").await;
    users["users"].as_array().unwrap().iter().find(|u| u["email"] == email).unwrap_or_else(|| panic!("{email} is not in {users}"))["id"].as_str().unwrap().to_string()
}

async fn locked_until(app: &TestApp, admin: &str, email: &str) -> Value {
    let (_, users) = app.get(admin, "/api/admin/users").await;
    users["users"].as_array().unwrap().iter().find(|u| u["email"] == email).unwrap()["lockedUntil"].clone()
}

#[tokio::test]
async fn login_answers_are_identical_for_unknown_wrong_password_and_disabled() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    app.create_user(&admin, "active@test.local", "editor").await;
    app.create_user(&admin, "off@test.local", "editor").await;
    let off = user_id(&app, &admin, "off@test.local").await;
    let (s, v) = app.patch(&admin, &format!("/api/admin/users/{off}"), json!({"disabled": true})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // an unknown address, a wrong password, a disabled account with the wrong and with the right password: one answer, and one password check each
    let cases = [("nobody@test.local", "wrong password!!"), ("active@test.local", "wrong password!!"), ("off@test.local", "wrong password!!"), ("off@test.local", "another long password 1")];
    let mut answers = vec![];
    for (email, password) in cases {
        let before = app.metric(PASSWORD_CHECKS).await;
        let (s, cookie, v) = app.login_as(email, password).await;
        assert!(cookie.is_none(), "{email}: no session for a refused sign-in");
        assert_eq!(app.metric(PASSWORD_CHECKS).await - before, 1.0, "{email} / {password}: exactly one password check, so every answer costs the same");
        answers.push((email, s, v));
    }
    for (email, s, v) in &answers {
        assert_eq!(*s, StatusCode::UNAUTHORIZED, "{email}: {v}");
        assert_eq!(v, &answers[0].2, "{email}: the body tells nothing about the account");
    }
    app.cleanup().await;
}

#[tokio::test]
async fn refused_sign_ins_do_no_hashing_and_look_the_same_for_unknown_and_known_emails() {
    let app = TestApp::new().await;
    // five failures from one address lock the pair (account, address) — for an address nobody has an account for as well
    for email in ["admin@test.local", "ghost@test.local"] {
        for _ in 0..5 {
            let (s, _, _) = app.login_as(email, "wrong password!!").await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{email}");
        }
    }
    let before = app.metric(PASSWORD_CHECKS).await;
    let (s_known, cookie, known) = app.login_as("admin@test.local", "correct horse battery staple").await;
    let (s_unknown, _, unknown) = app.login_as("ghost@test.local", "any password at all").await;
    assert_eq!((s_known, s_unknown), (StatusCode::TOO_MANY_REQUESTS, StatusCode::TOO_MANY_REQUESTS), "{known} / {unknown}");
    assert!(cookie.is_none());
    assert_eq!(known["error"]["code"], "locked");
    assert_eq!(known, unknown, "a locked account and a locked unknown address read the same");
    assert_eq!(app.metric(PASSWORD_CHECKS).await, before, "a refused sign-in does not hash: the lock is not a way to make the server work");

    // the same for the throttle of the address
    let app2 = TestApp::new().await;
    let admin = app2.login_admin().await;
    let (s, v) = app2.put(&admin, "/api/admin/settings", json!({"changes": {"auth.loginsPerMinutePerIp": 3}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    for i in 0..2 {
        let (s, _, _) = app2.login_from("10.1.1.1", &[], &format!("nobody{i}@test.local"), "wrong password!!").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let before = app2.metric(PASSWORD_CHECKS).await;
    let (s, _, v) = app2.login_from("10.1.1.1", &[], "nobody9@test.local", "wrong password!!").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    let (s, _, v) = app2.login_from("10.1.1.1", &[], "nobody9@test.local", "wrong password!!").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{v}");
    assert_eq!(v["error"]["code"], "rate_limited");
    assert_eq!(app2.metric(PASSWORD_CHECKS).await - before, 1.0, "only the attempt that was let through is hashed");
    app.cleanup().await;
    app2.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_wrong_passwords_lock_after_exactly_the_limit() {
    let app = std::sync::Arc::new(TestApp::new().await);
    let admin = app.login_admin().await;
    app.create_user(&admin, "victim@test.local", "editor").await;
    let before = app.metric(PASSWORD_CHECKS).await;

    let answers = concurrently(&app, 20, |app, _| async move { app.login_as("victim@test.local", "wrong password!!").await }).await;
    let wrong = answers.iter().filter(|(s, _, _)| *s == StatusCode::UNAUTHORIZED).count();
    let locked = answers.iter().filter(|(s, _, v)| *s == StatusCode::TOO_MANY_REQUESTS && v["error"]["code"] == "locked").count();
    assert_eq!(
        (wrong, locked),
        (5, 15),
        "20 attempts at the same moment: exactly auth.maxFailedAttempts are checked, the others are refused: {:?}",
        answers.iter().map(|a| a.0.as_u16()).collect::<Vec<_>>()
    );
    assert_eq!(app.metric(PASSWORD_CHECKS).await - before, 5.0, "so no more than five guesses were ever made, however many came at once");
    assert_ne!(locked_until(&app, &admin, "victim@test.local").await, Value::Null, "the administrator sees the lock");
    let (s, _, v) = app.login_as("victim@test.local", "another long password 1").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "the right password does not open a locked account: {v}");

    let app = std::sync::Arc::try_unwrap(app).ok().expect("the tasks are done");
    app.cleanup().await;
}

#[tokio::test]
async fn one_address_cannot_lock_an_account_for_everybody() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    app.create_user(&admin, "alice@test.local", "editor").await;
    app.create_user(&admin, "bob@test.local", "editor").await;
    for _ in 0..5 {
        let (s, _, _) = app.login_from("10.0.0.1", &[], "alice@test.local", "wrong password!!").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, _, v) = app.login_from("10.0.0.1", &[], "alice@test.local", "another long password 1").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("locked")), "the attacker's own address is locked out: {v}");
    let (s, cookie, v) = app.login_from("10.0.0.2", &[], "alice@test.local", "another long password 1").await;
    assert_eq!(s, StatusCode::OK, "another address still signs in: {v}");
    assert!(cookie.is_some());

    // the setting that makes the lock the account's: five failures from anywhere lock it for everybody (the behaviour before the scope existed)
    let (s, v) = app.put(&admin, "/api/admin/settings", json!({"changes": {"auth.lockoutScope": "account"}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    for _ in 0..5 {
        let (s, _, _) = app.login_from("10.0.0.1", &[], "bob@test.local", "wrong password!!").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, _, v) = app.login_from("10.0.0.2", &[], "bob@test.local", "another long password 1").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("locked")), "with the account as the scope the lock is for everybody: {v}");
    app.cleanup().await;
}

#[tokio::test]
async fn a_distributed_guess_hits_the_account_ceiling() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    app.create_user(&admin, "carol@test.local", "editor").await;
    // two failures lock one address; six failures, from wherever, lock the account
    let (s, v) = app.put(&admin, "/api/admin/settings", json!({"changes": {"auth.maxFailedAttempts": 2, "auth.maxFailedAttemptsPerAccount": 6}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // one address alone never pushes the account counter beyond the limit of an address
    for i in 0..6 {
        let (s, _, v) = app.login_from("10.0.1.1", &[], "carol@test.local", "wrong password!!").await;
        let want = if i < 2 { StatusCode::UNAUTHORIZED } else { StatusCode::TOO_MANY_REQUESTS };
        assert_eq!(s, want, "attempt {i}: {v}");
    }
    let (count,): (i32,) = sqlx::query_as("select count from rate_limits where key = $1").bind(account_key("carol@test.local")).fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(count, 2, "a locked address is refused without being charged to the account");

    // two more addresses with two failures each reach six
    for ip in ["10.0.1.2", "10.0.1.3"] {
        for _ in 0..2 {
            let (s, _, v) = app.login_from(ip, &[], "carol@test.local", "wrong password!!").await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
        }
    }
    let (s, _, v) = app.login_from("10.0.1.4", &[], "carol@test.local", "another long password 1").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("locked")), "a guess spread over addresses meets the ceiling of the account: {v}");
    app.cleanup().await;
}

#[tokio::test]
async fn the_right_password_that_reaches_the_account_ceiling_signs_in_and_locks_nobody() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    app.create_user(&admin, "gwen@test.local", "editor").await;
    // two failures lock one address; five, from wherever, lock the account
    let (s, v) = app.put(&admin, "/api/admin/settings", json!({"changes": {"auth.maxFailedAttempts": 2, "auth.maxFailedAttemptsPerAccount": 5}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // a guess spread over two addresses leaves the account one failure short of its ceiling
    for ip in ["10.0.5.1", "10.0.5.2"] {
        for _ in 0..2 {
            let (s, _, v) = app.login_from(ip, &[], "gwen@test.local", "wrong password!!").await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
        }
    }
    // the attempt that would be the fifth is the owner's: it is charged to the account (that is what keeps a flood from being checked), trips the
    // ceiling, is the right password, and signs in
    let (s, cookie, v) = app.login_from("10.0.5.3", &[], "gwen@test.local", "another long password 1").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(cookie.is_some());
    // ... and the sign-in is given back whole: the account is not locked, for nobody (itself, another address). (The list of users still shows the locks that
    // the two guessing addresses closed on themselves; that is a record of what happened, not what refuses a sign-in.)
    let (count, locked): (i32, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("select count, locked_until from rate_limits where key = $1").bind(account_key("gwen@test.local")).fetch_one(&app.state.pool).await.unwrap();
    assert_eq!((count, locked), (4, None), "four failures on record, and no lock for a count below the ceiling");
    for ip in ["10.0.5.3", "10.0.5.4"] {
        let (s, _, v) = app.login_from(ip, &[], "gwen@test.local", "another long password 1").await;
        assert_eq!(s, StatusCode::OK, "{ip}: a successful sign-in locked the account it signed in to: {v}");
    }
    // the ceiling still means something: the next failure is the fifth, and it does lock
    let (s, _, v) = app.login_from("10.0.5.5", &[], "gwen@test.local", "wrong password!!").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    let (s, _, v) = app.login_from("10.0.5.6", &[], "gwen@test.local", "another long password 1").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("locked")), "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn the_sweep_removes_counters_whose_window_is_over_and_keeps_locks() {
    use server::ratelimit::{charge, sweep, Limit};
    let app = TestApp::new().await;
    let pool = &app.state.pool;
    let counted = Limit { window_secs: 900.0, max: 5, lock_secs: 900.0 };
    let locks_at_once = Limit { max: 1, ..counted };
    for (key, limit) in [("sweep:old", counted), ("sweep:old-lock-over", locks_at_once), ("sweep:old-lock-active", locks_at_once), ("sweep:fresh", counted), ("sweep:fresh-lock", locks_at_once)] {
        charge(pool, key, limit).await.unwrap();
    }
    // the windows of three began an hour ago; the lock of one has run out, the lock of another has not
    sqlx::query("update rate_limits set window_start = now() - interval '1 hour' where key in ('sweep:old', 'sweep:old-lock-over', 'sweep:old-lock-active')").execute(pool).await.unwrap();
    sqlx::query("update rate_limits set locked_until = now() - interval '1 minute' where key = 'sweep:old-lock-over'").execute(pool).await.unwrap();
    sqlx::query("update rate_limits set locked_until = now() + interval '5 minutes' where key = 'sweep:old-lock-active'").execute(pool).await.unwrap();

    let removed = sweep(pool, 960.0).await.unwrap();
    let mut left: Vec<String> = sqlx::query_scalar("select key from rate_limits order by key").fetch_all(pool).await.unwrap();
    left.retain(|k| k.starts_with("sweep:")); // the administrator's sign-in made rows of its own
    assert_eq!(left, ["sweep:fresh", "sweep:fresh-lock", "sweep:old-lock-active"], "a window that is over goes, a lock that is running does not, nothing recent goes");
    assert_eq!(removed, 2, "the old counter and the old lock that has run out");
    assert_eq!(sweep(pool, 960.0).await.unwrap(), 0, "and a second round has nothing to do");
    // a window of an hour is not over for a sweep that only forgets what is older than two
    sqlx::query("update rate_limits set window_start = now() - interval '1 hour' where key = 'sweep:fresh'").execute(pool).await.unwrap();
    assert_eq!(sweep(pool, 7200.0).await.unwrap(), 0);
    app.cleanup().await;
}

#[tokio::test]
async fn reset_password_clears_the_lock() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    app.create_user(&admin, "dave@test.local", "editor").await;
    for _ in 0..5 {
        let (s, _, _) = app.login_from("10.0.2.1", &[], "dave@test.local", "wrong password!!").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    assert_ne!(locked_until(&app, &admin, "dave@test.local").await, Value::Null);
    let id = user_id(&app, &admin, "dave@test.local").await;
    let (s, v) = app.post(&admin, &format!("/api/admin/users/{id}/reset-password"), json!({"password": "a brand new password 77"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(locked_until(&app, &admin, "dave@test.local").await, Value::Null);
    let (s, _, v) = app.login_from("10.0.2.1", &[], "dave@test.local", "a brand new password 77").await;
    assert_eq!(s, StatusCode::OK, "the administrator's reset is the way to unlock an account, at once, for every address: {v}");
    app.cleanup().await;
}

#[tokio::test]
async fn login_throttle_is_shared_between_replicas() {
    let first = TestApp::new().await;
    let admin = first.login_admin().await;
    let (s, v) = first.put(&admin, "/api/admin/settings", json!({"changes": {"auth.loginsPerMinutePerIp": 3}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let second = first.second_instance().await; // after the change: it reads the limit when it starts
                                                // one address, six attempts, thrown at the two replicas in turn: the fourth is refused wherever it lands
    let mut statuses = vec![];
    for i in 0..6 {
        let replica = if i % 2 == 0 { &first } else { &second };
        let (s, _, _) = replica.login_from("10.0.3.1", &[], &format!("nobody{i}@test.local"), "wrong password!!").await;
        statuses.push(s.as_u16());
    }
    assert_eq!(statuses, vec![401, 401, 401, 429, 429, 429], "the count is in the database, not in the process");
    second.cleanup().await;
    first.cleanup().await;
}

#[tokio::test]
async fn forged_forwarded_for_does_not_raise_the_limit() {
    let app = TestApp::with_boot(|b| b.trust_proxy = true).await;
    let admin = app.login_admin().await;
    let (s, v) = app.put(&admin, "/api/admin/settings", json!({"changes": {"auth.loginsPerMinutePerIp": 3}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // the proxy (the peer) appends the address it saw, 9.9.9.9; whatever the client wrote before it is the client's to forge
    let mut statuses = vec![];
    for i in 0..5 {
        let forged = format!("10.20.30.{i}, 9.9.9.9");
        let (s, _, _) = app.login_from("172.16.0.5", &[("x-forwarded-for", &forged)], &format!("nobody{i}@test.local"), "wrong password!!").await;
        statuses.push(s.as_u16());
    }
    assert_eq!(statuses, vec![401, 401, 401, 429, 429], "a new first entry on every request is still one client");
    app.cleanup().await;
}

#[tokio::test]
async fn oversized_input_is_rejected_before_it_is_stored() {
    let app = TestApp::new().await;
    let long = format!("{}@test.local", "a".repeat(190)); // 201 characters
    let huge = format!("{}@test.local", "b".repeat(1_000_000));
    for email in [long.as_str(), huge.as_str(), ""] {
        let (s, _, v) = app.login_as(email, "wrong password!!").await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_email")), "{} characters: {v}", email.len());
    }
    // a control character is not part of an address, and a NUL byte cannot even be stored: it must not get as far as the lookup (a 500, after three counters)
    for email in ["a\u{0}b@test.local", "a\tb@test.local", "a\u{7f}b@test.local", "a\nb@test.local"] {
        let (s, _, v) = app.login_as(email, "wrong password!!").await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_email")), "{email:?}: {v}");
    }
    let (n,): (i64,) = sqlx::query_as("select count(*) from audit_log where length(target_id) > 300").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(n, 0, "the audit log does not take what the client sent");
    let (n,): (i64,) = sqlx::query_as("select count(*) from rate_limits").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(n, 0, "and neither do the counters");
    // an address of exactly the length users_create allows is an address like any other
    let fits = format!("{}@test.local", "c".repeat(189)); // 200
    let (s, _, _) = app.login_as(&fits, "wrong password!!").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // the administrator who makes an account is held to the same: no address that the sign-in would refuse to look up
    let admin = app.login_admin().await;
    for email in ["a\u{0}b@test.local", "a\u{1b}b@test.local"] {
        let (s, v) = app.post(&admin, "/api/admin/users", json!({"email": email, "role": "editor"})).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_email")), "creating {email:?}: {v}");
    }
    app.cleanup().await;
}

#[tokio::test]
async fn a_wrong_current_password_counts_towards_a_lock() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    let cookie = app.create_user(&admin, "erin@test.local", "editor").await;
    // somebody holding a session cannot guess the current password for ever
    for _ in 0..5 {
        let (s, v) = app.post(&cookie, "/api/auth/password", json!({"current": "wrong password!!", "new": "a brand new password 88"})).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("wrong_password")), "{v}");
    }
    let (s, v) = app.post(&cookie, "/api/auth/password", json!({"current": "another long password 1", "new": "a brand new password 88"})).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("locked")), "{v}");
    // the administrator's reset opens it again
    let id = user_id(&app, &admin, "erin@test.local").await;
    let (s, v) = app.post(&admin, &format!("/api/admin/users/{id}/reset-password"), json!({"password": "temporary password 99"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, cookie, v) = app.login_as("erin@test.local", "temporary password 99").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app.post(&cookie.unwrap(), "/api/auth/password", json!({"current": "temporary password 99", "new": "a brand new password 88"})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.cleanup().await;
}

#[tokio::test]
async fn hops_two_takes_the_second_entry_from_the_right() {
    // a load balancer in front of an ingress: the ingress appends the address it saw (the balancer's), the balancer appended the client's
    let app = TestApp::with_boot(|b| {
        b.trust_proxy = true;
        b.trust_proxy_hops = 2;
    })
    .await;
    let admin = app.login_admin().await;
    let (s, v) = app.put(&admin, "/api/admin/settings", json!({"changes": {"auth.loginsPerMinutePerIp": 3}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let mut statuses = vec![];
    for i in 0..5 {
        // a different forged first entry, and a different last one (the address of the balancer, as its replicas differ): the client is the second from the right
        let chain = format!("10.20.30.{i}, 8.8.8.8, 172.16.0.{i}");
        let (s, _, _) = app.login_from("172.16.9.9", &[("x-forwarded-for", &chain)], &format!("nobody{i}@test.local"), "wrong password!!").await;
        statuses.push(s.as_u16());
    }
    assert_eq!(statuses, vec![401, 401, 401, 429, 429]);
    // and a client that shows up with a chain that is too short for two proxies is the peer, not the one entry it wrote
    for i in 0..3 {
        let (s, _, _) = app.login_from("172.16.9.10", &[("x-forwarded-for", &format!("10.20.31.{i}"))], &format!("somebody{i}@test.local"), "wrong password!!").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, _, v) = app.login_from("172.16.9.10", &[("x-forwarded-for", "10.20.31.99")], "somebody9@test.local", "wrong password!!").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "four attempts from the same peer with chains of one entry: {v}");
    app.cleanup().await;
}

#[tokio::test]
async fn rate_limit_window_expires_and_locks_last_from_the_trip() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    app.create_user(&admin, "frank@test.local", "editor").await;
    let pair = format!("{}:10.5.5.5", account_key("frank@test.local"));
    let from = |pw: &'static str| {
        let app = &app;
        async move { app.login_from("10.5.5.5", &[], "frank@test.local", pw).await }
    };

    // failures are counted in a window that starts at the first one: four, then the window is over, and four more are four, not eight
    for _ in 0..4 {
        assert_eq!(from("wrong password!!").await.0, StatusCode::UNAUTHORIZED);
    }
    sqlx::query("update rate_limits set window_start = now() - interval '1 hour' where key = $1").bind(&pair).execute(&app.state.pool).await.unwrap();
    for _ in 0..4 {
        assert_eq!(from("wrong password!!").await.0, StatusCode::UNAUTHORIZED, "the first four are forgotten");
    }
    let (count,): (i32,) = sqlx::query_as("select count from rate_limits where key = $1").bind(&pair).fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(count, 4);

    // the fifth within the window closes the lock; it lasts from that moment, however old the window is by then
    assert_eq!(from("wrong password!!").await.0, StatusCode::UNAUTHORIZED);
    sqlx::query("update rate_limits set window_start = now() - interval '1 hour' where key = $1").bind(&pair).execute(&app.state.pool).await.unwrap();
    let (s, _, v) = from("another long password 1").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("locked")), "an old window does not open a lock that was closed a moment ago: {v}");
    // ... and it runs out by itself
    sqlx::query("update rate_limits set locked_until = now() - interval '1 second' where key = $1").bind(&pair).execute(&app.state.pool).await.unwrap();
    let (s, cookie, v) = from("another long password 1").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(cookie.is_some());
    let (n,): (i64,) = sqlx::query_as("select count(*) from rate_limits where key = $1").bind(&pair).fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(n, 0, "a sign-in that succeeds forgets the failures of its address");

    // the limit of an address: a window of a minute that starts with its first attempt
    let (s, v) = app.put(&admin, "/api/admin/settings", json!({"changes": {"auth.loginsPerMinutePerIp": 3}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let mut statuses = vec![];
    for i in 0..4 {
        statuses.push(app.login_from("10.5.6.6", &[], &format!("nobody{i}@test.local"), "wrong password!!").await.0.as_u16());
    }
    assert_eq!(statuses, vec![401, 401, 401, 429]);
    sqlx::query("update rate_limits set window_start = now() - interval '2 minutes' where key = 'login:ip:10.5.6.6'").execute(&app.state.pool).await.unwrap();
    assert_eq!(app.login_from("10.5.6.6", &[], "nobody9@test.local", "wrong password!!").await.0, StatusCode::UNAUTHORIZED, "a minute later the address is let in again");
    let (count,): (i32,) = sqlx::query_as("select count from rate_limits where key = 'login:ip:10.5.6.6'").fetch_one(&app.state.pool).await.unwrap();
    assert_eq!(count, 1);
    app.cleanup().await;
}

#[tokio::test]
async fn a_refusal_says_when_to_come_back() {
    let app = TestApp::new().await;
    for _ in 0..5 {
        app.login_as("ghost@test.local", "wrong password!!").await;
    }
    let (s, h, v) = app.request(Method::POST, "/api/auth/login", Some(json!({"email": "ghost@test.local", "password": "wrong password!!"})), None, &[]).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("locked")));
    let wait: u64 = h.get(header::RETRY_AFTER).expect("Retry-After on a lock").to_str().unwrap().parse().unwrap();
    assert!((1..=15 * 60).contains(&wait), "{wait} seconds, for a lock of fifteen minutes");
    app.cleanup().await;
}

#[tokio::test]
async fn the_sign_in_metrics_name_the_outcome_and_never_the_reason() {
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    app.create_user(&admin, "gina@test.local", "editor").await;
    app.login_as("ghost@test.local", "wrong password!!").await; // no such account
    app.login_as("gina@test.local", "wrong password!!").await; // wrong password
    for _ in 0..5 {
        app.login_as("hugo@test.local", "wrong password!!").await; // no such account either; the fifth closes the lock
    }
    app.login_as("hugo@test.local", "wrong password!!").await; // locked
    let (s, v) = app.put(&admin, "/api/admin/settings", json!({"changes": {"auth.loginsPerMinutePerIp": 3}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.login_as("anyone@test.local", "wrong password!!").await; // throttled

    let outcome = |o: &'static str| format!("infra_auth_logins_total{{outcome=\"{o}\"}}");
    assert_eq!(app.metric(&outcome("ok")).await, 2.0, "the administrator and the new user");
    assert_eq!(app.metric(&outcome("failed")).await, 1.0 + 1.0 + 5.0, "no such account, wrong password, and the five that closed the lock on hugo (the fifth was checked, the sixth was not)");
    assert_eq!((app.metric(&outcome("locked")).await, app.metric(&outcome("throttled")).await), (1.0, 1.0));
    let (_, _, text) = app.request(Method::GET, "/metrics", None, None, &[]).await;
    let text = text.as_str().unwrap();
    for line in text.lines().filter(|l| l.starts_with("infra_auth_")) {
        for reason in ["unknown", "disabled", "password\"", "bad_", "no_such", "ghost", "gina", "hugo"] {
            assert!(!line.contains(reason), "{line}: the metric says why a sign-in failed, or for whom");
        }
    }
    app.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn password_hashing_waits_for_a_slot_in_every_handler() {
    // a gate of one slot, and the test holds it: whoever hashes a password has to wait for it. That every handler that hashes goes through the gate is
    // what keeps the hashing off the threads that serve the other requests and the number of hashes (19 MiB each) bounded; the gate's own work is
    // off the runtime thread (crypto.rs, password_gate_work_runs_off_the_runtime_thread)
    let app = std::sync::Arc::new(TestApp::with_hash_slots(1).await);
    let admin = app.login_admin().await;
    let victim = app.create_user(&admin, "ivan@test.local", "editor").await;
    let id = user_id(&app, &admin, "ivan@test.local").await;
    let checks = app.state.metrics.auth_password_checks.get();

    let held = app.state.passwords.reserve().await;
    let (a, v, c, u) = (app.clone(), app.clone(), app.clone(), app.clone());
    let (admin_c, victim_c) = (admin.clone(), victim.clone());
    let login = tokio::spawn(async move { a.login_as("ivan@test.local", "wrong password!!").await.0 });
    let change = tokio::spawn(async move { c.post(&victim_c, "/api/auth/password", json!({"current": "another long password 1", "new": "a brand new password 88"})).await.0 });
    let reset = tokio::spawn({
        let admin = admin.clone();
        async move { v.post(&admin, &format!("/api/admin/users/{id}/reset-password"), json!({"password": "temporary password 99"})).await.0 }
    });
    let create = tokio::spawn(async move { u.post(&admin_c, "/api/admin/users", json!({"email": "jane@test.local", "role": "viewer", "password": "yet another long one 3"})).await.0 });

    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let waiting = [login.is_finished(), change.is_finished(), reset.is_finished(), create.is_finished()];
    assert_eq!(waiting, [false; 4], "login, change password, reset password and create user (in that order) all wait for the slot");
    assert_eq!(app.state.metrics.auth_password_checks.get(), checks, "and none of them has started to check");
    drop(held);
    let done = [login.await.unwrap(), change.await.unwrap(), reset.await.unwrap(), create.await.unwrap()];
    // the order they get the slot in is theirs; what each answers depends on it only for the login and the change of password, which both fail or succeed
    // before or after the reset: the reset and the creation always succeed
    assert_eq!((done[2], done[3]), (StatusCode::OK, StatusCode::OK));
    assert!([StatusCode::OK, StatusCode::BAD_REQUEST].contains(&done[1]) && [StatusCode::UNAUTHORIZED, StatusCode::TOO_MANY_REQUESTS].contains(&done[0]), "{done:?}");

    let app = std::sync::Arc::try_unwrap(app).ok().expect("the tasks are done");
    app.cleanup().await;
}

/// The lines of `/metrics` that are samples (not `# HELP` / `# TYPE`): one per series.
fn series(body: &Value) -> Vec<String> {
    body.as_str().unwrap_or_default().lines().filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect()
}

#[tokio::test]
async fn metrics_method_label_is_bounded() {
    // /metrics may be open (METRICS_TOKEN is empty by default), and HTTP takes any token as a method: a label that is "whatever the client sent" is an
    // unauthenticated way to grow the registry without end, fifteen series (twelve buckets, sum, count, the counter) per invented method
    let app = TestApp::new().await;
    let scrape = || async { series(&app.request(Method::GET, "/metrics", None, None, &[]).await.2) };
    let send = |method: &str| {
        let method = Method::from_bytes(method.as_bytes()).unwrap();
        let app = &app;
        async move {
            // two paths, so that a method that answers differently on one of them still lands on routes the first round has seen
            for path in ["/healthz", "/api/projects"] {
                app.request(method.clone(), path, None, None, &[]).await;
            }
        }
    };

    // the seven methods the label keeps, and one unusual one (it creates the `other` series, once)
    for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "AUDITB07M0"] {
        send(m).await;
    }
    // a scrape is a request too, and its own series appear after the first one has been answered: let that happen before counting
    scrape().await;
    let before = scrape().await;
    for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "other"] {
        assert!(before.iter().any(|l| l.starts_with("infra_http_requests_total{") && l.contains(&format!("method=\"{m}\""))), "{m} has a series of its own: {before:#?}");
    }

    // twenty more invented methods, and two that exist but are not on the list: none of them adds a series
    let invented: Vec<String> = (1..=20).map(|i| format!("AUDITB07M{i}")).chain(["TRACE".to_string(), "CONNECT".to_string()]).collect();
    for m in &invented {
        send(m).await;
    }
    let after = scrape().await;
    let name_of = |l: &String| l.rsplit_once(' ').map(|(name, _)| name.to_string()).unwrap_or_default();
    let known: std::collections::BTreeSet<String> = before.iter().map(name_of).collect();
    let new: Vec<String> = after.iter().map(name_of).filter(|n| !known.contains(n)).collect();
    assert!(new.is_empty() && after.len() == before.len(), "{} invented methods changed the series: {} -> {}, new: {new:#?}", invented.len(), before.len(), after.len());
    let text = after.join("\n");
    for m in &invented {
        assert!(!text.contains(&format!("method=\"{m}\"")), "{m} is a series of its own");
    }
    // every label that is there is one of the eight
    let methods: std::collections::BTreeSet<&str> = after.iter().filter_map(|l| l.split("method=\"").nth(1)).filter_map(|rest| rest.split('"').next()).collect();
    assert_eq!(methods, ["DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT", "other"].into_iter().collect(), "{methods:?}");
    // and what the unusual requests did is counted, under `other`
    let other: f64 = after.iter().filter(|l| l.starts_with("infra_http_requests_total{") && l.contains("method=\"other\"")).filter_map(|l| l.rsplit_once(' ')?.1.parse::<f64>().ok()).sum();
    assert_eq!(other, 2.0 * (1 + invented.len()) as f64, "every unusual request is counted once, as `other`");
    app.cleanup().await;
}

fn posture_ids(v: &Value) -> Vec<String> {
    v["posture"].as_array().expect("posture is a list").iter().map(|c| c["id"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn admin_system_reports_posture() {
    // a development server is meant to be open: the list is there and empty (the key is always present, so that an empty list means "nothing found")
    let app = TestApp::new().await;
    let admin = app.login_admin().await;
    let (s, v) = app.get(&admin, "/api/admin/system").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["posture"], json!([]), "{v}");
    assert!(v["env"]["METRICS_PORT"].is_null(), "no metrics port is null: {v}");
    // only an administrator reads it
    let editor = app.create_user(&admin, "editor@test.local", "editor").await;
    let (s, _) = app.get(&editor, "/api/admin/system").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    app.cleanup().await;

    // a production installation as the quick start leaves it: http cookies, /metrics on the application's port without a token, the published admin address
    let app = TestApp::with_boot(|b| {
        b.production = true;
        b.cookie_secure = false;
        b.admin_email = Some("admin@example.com".into());
    })
    .await;
    let (_, admin, _) = app.login_as("admin@example.com", "correct horse battery staple").await;
    let admin = admin.expect("session");
    let (s, v) = app.get(&admin, "/api/admin/system").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(posture_ids(&v), ["cookie_insecure", "metrics_open", "default_admin_account", "public_url_empty"], "{v}");
    for c in v["posture"].as_array().unwrap() {
        assert!(["warn", "info"].contains(&c["level"].as_str().unwrap()), "{c}");
        for part in ["message", "fix"] {
            assert!(c[part]["ru"].as_str().unwrap().len() > 20 && c[part]["en"].as_str().unwrap().len() > 20, "{c}");
        }
    }
    let default_admin = v["posture"].as_array().unwrap().iter().find(|c| c["id"] == "default_admin_account").unwrap();
    assert!(default_admin["message"]["en"].as_str().unwrap().contains("admin@example.com") && default_admin["message"]["ru"].as_str().unwrap().contains("admin@example.com"));

    // what the operator does about it takes the finding away: the setting that switches the metrics off, a public URL
    set(&app, &admin, json!({"observability.metricsEnabled": false, "app.publicUrl": "https://architect.example.com"})).await;
    let (_, v) = app.get(&admin, "/api/admin/system").await;
    assert_eq!(posture_ids(&v), ["cookie_insecure", "default_admin_account"], "{v}");

    // the settings the checks read: a key with AI on and no budget, an audit log that is kept too short (the threshold is a setting too)
    set(&app, &admin, json!({"llm.enabled": true, "llm.apiKey": "sk-or-v1-posture-test-key", "llm.monthlyBudgetUsd": 0, "retention.auditDays": 30})).await;
    let (_, v) = app.get(&admin, "/api/admin/system").await;
    assert_eq!(posture_ids(&v), ["cookie_insecure", "default_admin_account", "llm_unbounded_spend", "audit_retention_short"], "{v}");
    let short = v["posture"].as_array().unwrap().iter().find(|c| c["id"] == "audit_retention_short").unwrap();
    assert!(short["message"]["en"].as_str().unwrap().contains("30 days") && short["message"]["en"].as_str().unwrap().contains("90"), "{short}");
    set(&app, &admin, json!({"llm.monthlyBudgetUsd": 20, "retention.auditDaysWarnBelow": 7})).await;
    let (_, v) = app.get(&admin, "/api/admin/system").await;
    assert_eq!(posture_ids(&v), ["cookie_insecure", "default_admin_account"], "{v}");
    // the key itself is never in the answer
    assert!(!v.to_string().contains("sk-or-v1-posture-test-key"));

    // the default administrator is disabled by another administrator: the finding goes (it is a fact about the accounts, read from the database)
    let other = app.create_user(&admin, "second.admin@test.local", "admin").await;
    let (_, users) = app.get(&other, "/api/admin/users").await;
    let id = users["users"].as_array().unwrap().iter().find(|u| u["email"] == "admin@example.com").unwrap()["id"].as_str().unwrap().to_string();
    let (s, v) = app.patch(&other, &format!("/api/admin/users/{id}"), json!({"disabled": true})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = app.get(&other, "/api/admin/system").await;
    assert_eq!(posture_ids(&v), ["cookie_insecure"], "{v}");
    app.cleanup().await;

    // behind a proxy that is trusted, and with the metrics on a port of their own
    let app = TestApp::with_boot(|b| {
        b.production = true;
        b.cookie_secure = true;
        b.trust_proxy = true;
        b.trust_proxy_hops = 2;
        b.metrics_port = Some(9090);
    })
    .await;
    let admin = app.login_admin().await;
    let (_, v) = app.get(&admin, "/api/admin/system").await;
    assert_eq!(posture_ids(&v), ["trust_proxy", "public_url_empty"], "{v}");
    assert!(v["posture"][0]["message"]["en"].as_str().unwrap().contains("2 entries"), "{v}");
    assert_eq!(v["env"]["METRICS_PORT"], 9090);
    app.cleanup().await;
}
