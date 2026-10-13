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
    assert!(v["catalog"]["kinds"].as_array().unwrap().len() >= 20);
    assert!(v["rules"].as_array().unwrap().len() >= 50);
    assert_eq!(v["templates"].as_array().unwrap().len(), 4);
    assert_eq!(v["ai"]["enabled"], false, "no key configured yet");
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
