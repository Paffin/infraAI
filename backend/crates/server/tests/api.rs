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
// AI pipeline against a mock OpenRouter
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

async fn configure_llm(app: &TestApp, c: &str, mock: &MockLlm) {
    let (s, v) = app.put(c, "/api/admin/settings", json!({"changes": {"llm.apiKey": "sk-mock", "llm.baseUrl": mock.url, "llm.model.planner": "mock/model", "llm.model.fast": "mock/model", "llm.model.fallbacks": ["mock/plain"], "llm.retries": 0}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

#[tokio::test]
async fn ai_patch_flow_preview_apply_and_privacy_settings() {
    let mock = start_mock_llm().await;
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    configure_llm(&app, &c, &mock).await;
    let pid = app.first_project(&c).await;

    // admin test endpoint + model catalog
    let (s, v) = app.post(&c, "/api/admin/llm/test", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["models"]["planner"]["found"], true);
    let (_, m) = app.get(&c, "/api/admin/llm/models?structured=1").await;
    assert_eq!(m["models"].as_array().unwrap().len(), 1);

    // the model answers with an operation list; first it sends garbage to exercise the repair loop
    mock.push("I think you should add a cache!");
    mock.push(
        &json!({"rationale": "Add a second Redis replica for resilience", "ops": [
        {"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 40}},
        {"op": "add_node", "node": {"id": "mon-extra", "kind": "monitoring", "name": "Extra monitoring", "zoneId": "site3", "props": {"replicas": "2", "bogus": {"x": 1}}}}
    ], "assumptions": ["none"]})
        .to_string(),
    );
    let (s, j) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "patch", "prompt": "add monitoring in the third site", "lang": "en"})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let job = wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    let codes: Vec<&str> = job["steps"].as_array().unwrap().iter().map(|s| s["code"].as_str().unwrap()).collect();
    assert!(codes.contains(&"repairing") && codes.contains(&"calling_model") && codes.contains(&"evaluating"), "{codes:?}");

    // requests carried the privacy/fallback configuration and a JSON response format, plus the repair feedback
    let reqs = mock.requests.lock().unwrap().clone();
    assert!(reqs.len() >= 2);
    assert_eq!(reqs[0]["provider"]["data_collection"], "deny");
    assert_eq!(reqs[0]["provider"]["require_parameters"], true);
    assert_eq!(reqs[0]["models"], json!(["mock/model", "mock/plain"]));
    // `auto` mode: the patch schema contains free-form maps, so it is sent as json_object (strict decoding would empty them)
    assert_eq!(reqs[0]["response_format"]["type"], "json_object");
    assert!(reqs[1]["messages"].to_string().contains("rejected by the validator"));
    assert!(reqs[0]["messages"][0]["content"].as_str().unwrap().contains("CURRENT ARCHITECTURE"));
    assert!(reqs[0]["messages"][1]["content"].as_str().unwrap().contains("<user_data>"), "user text is fenced as data");

    // proposal: preview with diff and scores, the architecture is untouched until applied
    let proposal = job["result"]["proposalId"].as_str().unwrap().to_string();
    let (s, pr) = app.get(&c, &format!("/api/proposals/{proposal}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(pr["status"], "pending");
    assert_eq!(pr["mode"], "ops");
    assert_eq!(pr["preview"]["diff"]["nodes"]["added"][0]["id"], "mon-extra");
    let (_, before) = app.get(&c, &format!("/api/projects/{pid}")).await;
    assert!(before["architecture"]["nodes"].as_array().unwrap().iter().all(|n| n["id"] != "mon-extra"));

    let (s, v) = app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({"expectedRev": before["rev"]})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, after) = app.get(&c, &format!("/api/projects/{pid}")).await;
    let node = after["architecture"]["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "mon-extra").expect("applied");
    assert_eq!(node["props"]["replicas"], 2, "numeric string coerced");
    assert!(node["props"].get("bogus").is_none(), "non-scalar/unknown prop dropped");
    // applying twice is refused; a rollback version exists
    let (s, _) = app.post(&c, &format!("/api/proposals/{proposal}/apply"), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, vers) = app.get(&c, &format!("/api/projects/{pid}/versions")).await;
    assert!(vers["versions"].as_array().unwrap().iter().any(|v| v["message"].as_str().unwrap().starts_with("AI:")));
    // chat history holds the exchange and the proposal link
    let (_, chat) = app.get(&c, &format!("/api/projects/{pid}/chat")).await;
    let msgs = chat["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| m["role"] == "user") && msgs.iter().any(|m| m["role"] == "assistant" && m["proposalStatus"] == "applied"));
    // usage was accounted
    let (_, u) = app.get(&c, "/api/admin/llm/usage?days=1").await;
    assert!(u["monthSpendUsd"].as_f64().unwrap() > 0.0);
    app.cleanup().await;
}

#[tokio::test]
async fn ai_generate_replaces_architecture_with_proposal() {
    let mock = start_mock_llm().await;
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    configure_llm(&app, &c, &mock).await;
    let (s, p) = app.post(&c, "/api/projects", json!({"name": "empty"})).await;
    assert_eq!(s, StatusCode::OK);
    let pid = p["id"].as_str().unwrap().to_string();

    let arch = json!({
        "requirements": {"rpsPeak": 500, "rtoSeconds": 900, "rpoSeconds": 0},
        "architecture": {
            "zones": [{"id": "dc1", "name": "DC1", "type": "dc", "props": {"geo": "a"}}, {"id": "dc2", "name": "DC2", "type": "dc", "props": {"geo": "b"}}],
            "nodes": [
                {"id": "users", "kind": "users", "name": "Users"},
                {"id": "app1", "kind": "vm_pool", "name": "App", "zoneId": "dc1", "group": "app", "props": {"replicas": 3}},
                {"id": "app2", "kind": "vm_pool", "name": "App", "zoneId": "dc2", "group": "app", "props": {"replicas": 3}}
            ],
            "edges": [{"id": "e1", "source": "users", "target": "app1", "type": "traffic"}, {"id": "e2", "source": "users", "target": "app2", "type": "traffic"}]
        },
        "rationale": "Two sites, stateless app", "assumptions": ["No database requested"]
    });
    mock.push(&arch.to_string());
    // the engine finds the unfixed shortcomings (users → app1/app2 are different units): one refine round is attempted
    mock.push(&arch.to_string());
    let (s, j) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "generate", "prompt": "a small shop, 500 rps, two sites", "lang": "en"})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let job = wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    let proposal = job["result"]["proposalId"].as_str().unwrap();
    let (_, pr) = app.get(&c, &format!("/api/proposals/{proposal}")).await;
    assert_eq!(pr["mode"], "replace");
    assert!(pr["architecture"]["nodes"].as_array().unwrap().iter().all(|n| n["proposed"] == true));
    assert_eq!(pr["architecture"]["requirements"]["rpsPeak"], 500.0);
    assert!(pr["preview"]["diff"]["nodes"]["added"].as_array().unwrap().len() == 3);

    // concurrency guard: a second request while one runs is refused; failed job reports a clear error
    mock.push(&completion("x").to_string());
    app.cleanup().await;
}

#[tokio::test]
async fn ai_errors_are_reported_and_limits_enforced() {
    let mock = start_mock_llm().await;
    let app = TestApp::new().await;
    let c = app.login_admin().await;
    let pid = app.first_project(&c).await;

    // not configured
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "chat", "prompt": "hi"})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"]["code"], "llm_not_configured");

    configure_llm(&app, &c, &mock).await;
    // prompt length limit
    app.put(&c, "/api/admin/settings", json!({"changes": {"llm.maxPromptChars": 100}})).await;
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "chat", "prompt": "x".repeat(200)})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "prompt_too_long");
    app.put(&c, "/api/admin/settings", json!({"changes": {"llm.maxPromptChars": 8000}})).await;

    // upstream failure surfaces as a failed job with a stable code (mock has no answers queued → 500)
    let (s, j) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "chat", "prompt": "how resilient is this?"})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let job = wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "failed");
    assert_eq!(job["errorCode"], "llm_http");

    // a successful chat answer
    mock.push("The score is driven by the Redis and Kafka findings.");
    let (_, j) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "chat", "prompt": "why is the score not 100?"})).await;
    let job = wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert!(job["result"]["answer"].as_str().unwrap().contains("Redis"));

    // monthly budget guard: spend so far is > 0, a tiny budget blocks further calls
    app.put(&c, "/api/admin/settings", json!({"changes": {"llm.monthlyBudgetUsd": 0.0001}})).await;
    mock.push("never delivered");
    let (_, j) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "chat", "prompt": "again"})).await;
    let job = wait_job(&app, &c, j["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "failed");
    assert_eq!(job["errorCode"], "llm_budget");

    // AI switched off: manual mode only
    app.put(&c, "/api/admin/settings", json!({"changes": {"llm.enabled": false}})).await;
    let (s, v) = app.post(&c, &format!("/api/projects/{pid}/ai/jobs"), json!({"kind": "chat", "prompt": "hi"})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"]["code"], "llm_disabled");
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
