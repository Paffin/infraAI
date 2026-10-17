//! Administration: settings, LLM, catalog/rules/templates/prompts, users, audit, system status.
//! Every mutating call is audited; secrets are never returned.

use crate::api_support::{text, TextField};
use crate::audit;
use crate::auth::{check_password_policy, client_ip, failure_key, forget_user, is_acceptable_email, pwchange_key, AuthUser, Role};
use crate::config_store::{bump_rev, default_items, is_valid_item_id, range_regressions, single_document_id, validate_item, ALL_KINDS, MSG_INVALID_ITEM_ID, MSG_SINGLE_DOCUMENT};
use crate::crypto::random_password;
use crate::error::{ApiError, ApiResult};
use crate::json::{Json, Path, Query};
use crate::llm::openrouter::{call, fetch_models, CallCtx, LlmRequest, ModelRole, Msg};
use crate::outbound;
use crate::ratelimit;
use crate::settings;
use crate::state::AppState;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::Row;
use std::net::SocketAddr;
use uuid::Uuid;

fn ip(state: &AppState, headers: &HeaderMap, addr: SocketAddr) -> String {
    client_ip(headers, Some(addr), &state.boot)
}

async fn changed(state: &AppState) -> ApiResult<()> {
    bump_rev(&state.pool).await?;
    state.config.invalidate().await;
    Ok(())
}

/// A refused change of the settings as the API answers it. What PostgreSQL said stays in the log: it is about the database, not about the
/// setting, and it names no key (the answer must).
fn setting_error(e: settings::SaveError) -> ApiError {
    use settings::SaveError;
    // `reason` is the English sentence; `reasonCode` and `params` are what the interface puts in the language of the person (docs/API.md)
    let refused = |key: String, reason: settings::Refusal| {
        ApiError::bad_request("invalid_setting", format!("{key}: {reason}")).with_details(json!({"key": key, "reason": reason.text(), "reasonCode": reason.code, "params": reason.params}))
    };
    match e {
        SaveError::Invalid { key, reason } => refused(key, reason),
        SaveError::Store { key, source: sqlx::Error::Database(db) } => {
            tracing::warn!(%key, error = %db, "the database refused a setting");
            refused(key, settings::Refusal::new("cannot_store", Value::Null, "the value cannot be stored"))
        }
        SaveError::Store { source, .. } => ApiError::internal(source),
    }
}

// ------------------------------------------------------------------ settings

pub async fn get_settings(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let cfg = state.cfg().await?;
    let present = |k: &str| state.env_secret_present(k);
    Ok(Json(json!({"settings": cfg.settings.for_admin(&present), "rev": cfg.rev})))
}

#[derive(Deserialize)]
pub struct SaveSettings {
    pub changes: Map<String, Value>,
}

pub async fn put_settings(State(state): State<AppState>, user: AuthUser, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(b): Json<SaveSettings>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let keys = settings::save(&state.pool, &state.keys, &b.changes, &user.email).await.map_err(setting_error)?;
    changed(&state).await?;
    // log keys only; secret values never leave this function
    audit::log(&state.pool, Some(&user), "settings.update", "settings", "", json!({"keys": keys}), &ip(&state, &headers, addr)).await;
    let cfg = state.cfg().await?;
    let present = |k: &str| state.env_secret_present(k);
    Ok(Json(json!({"settings": cfg.settings.for_admin(&present), "rev": cfg.rev})))
}

// ------------------------------------------------------------------ LLM

#[derive(Deserialize)]
pub struct ModelsQuery {
    #[serde(default)]
    pub refresh: Option<u8>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub structured: Option<u8>,
    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn llm_models(State(state): State<AppState>, user: AuthUser, Query(q): Query<ModelsQuery>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let cfg = state.cfg().await?;
    let all = fetch_models(&state, &cfg, q.refresh.unwrap_or(0) == 1).await?;
    let needle = q.q.unwrap_or_default().to_lowercase();
    let mut list: Vec<Value> = all
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| needle.is_empty() || m["id"].as_str().unwrap_or("").to_lowercase().contains(&needle) || m["name"].as_str().unwrap_or("").to_lowercase().contains(&needle))
        .filter(|m| q.structured.unwrap_or(0) == 0 || m["structuredOutputs"].as_bool().unwrap_or(false))
        .collect();
    let total = list.len();
    list.truncate(q.limit.unwrap_or(200).min(1000));
    Ok(Json(json!({"models": list, "total": total})))
}

#[derive(Deserialize, Default)]
pub struct TestBody {
    #[serde(default)]
    pub ping: bool,
}

pub async fn llm_test(State(state): State<AppState>, user: AuthUser, body: Option<Json<TestBody>>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let cfg = state.cfg().await?;
    let ping = body.map(|b| b.0.ping).unwrap_or(false);
    let Some(key) = state.llm_key(&cfg) else {
        return Ok(Json(json!({"ok": false, "code": "llm_not_configured", "message": "API key is not set (admin panel or OPENROUTER_API_KEY)"})));
    };
    let base = cfg.settings.str("llm.baseUrl");
    let res =
        state.http.get(format!("{}/key", base.trim_end_matches('/'))).bearer_auth(&key).timeout(std::time::Duration::from_secs(cfg.settings.num("llm.catalogTimeoutSeconds") as u64)).send().await;
    let (key_ok, key_info, key_msg) = match res {
        Err(e) => (false, Value::Null, format!("cannot reach OpenRouter: {}", e.without_url())),
        Ok(r) => {
            let status = r.status();
            // an answer that cannot be read (past `outbound.maxAnswerKb`, not JSON) says nothing about the key: it is not "ok" and not "rejected" either
            match (status.is_success(), outbound::read_json_capped(r, outbound::answer_cap(&cfg.settings)).await) {
                (true, Ok(v)) => {
                    let d = &v["data"];
                    (true, json!({"label": d["label"], "limit": d["limit"], "limitRemaining": d["limit_remaining"], "usage": d["usage"], "isFreeTier": d["is_free_tier"]}), String::new())
                }
                (true, Err(why)) => (false, Value::Null, format!("the answer of OpenRouter cannot be read ({why}), so the key was not checked")),
                (false, Ok(v)) => {
                    let said = v["error"]["message"].as_str().map(str::to_string);
                    let rejected = matches!(status, reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN);
                    (false, Value::Null, said.unwrap_or_else(|| if rejected { "the key was rejected".to_string() } else { format!("OpenRouter answered {status}") }))
                }
                (false, Err(_)) => (false, Value::Null, format!("OpenRouter answered {status}")),
            }
        }
    };
    let mut warnings: Vec<String> = vec![];
    let mut models = json!({});
    if let Ok(all) = fetch_models(&state, &cfg, false).await {
        for (role, id) in [("planner", cfg.settings.str("llm.model.planner")), ("fast", cfg.settings.str("llm.model.fast"))] {
            let found = all.as_array().and_then(|a| a.iter().find(|m| m["id"] == id)).cloned();
            match &found {
                None => warnings.push(format!("model '{id}' ({role}) was not found in the OpenRouter catalog")),
                Some(m) => {
                    if cfg.settings.str("llm.structuredOutput") == "json_schema" && role == "planner" && !m["structuredOutputs"].as_bool().unwrap_or(false) {
                        warnings.push(format!("model '{id}' does not advertise structured outputs; json_object mode will be used"));
                    }
                }
            }
            models[role] = json!({"id": id, "found": found.is_some(), "structuredOutputs": found.as_ref().map(|m| m["structuredOutputs"].clone()).unwrap_or(Value::Null), "contextLength": found.as_ref().map(|m| m["contextLength"].clone()).unwrap_or(Value::Null)});
        }
    }
    let mut ping_result = Value::Null;
    if ping && key_ok {
        let started = std::time::Instant::now();
        let r = call(
            &state,
            &cfg,
            &CallCtx { job_id: None, project_id: None, user_id: Some(user.id) },
            LlmRequest { purpose: "test", role: ModelRole::Fast, messages: vec![Msg::user("Reply with the single word: ok")], schema: None },
        )
        .await;
        ping_result = match r {
            Ok(r) => json!({"ok": true, "model": r.model, "latencyMs": started.elapsed().as_millis() as u64, "costUsd": r.cost_usd, "answer": r.content.chars().take(40).collect::<String>()}),
            Err(e) => json!({"ok": false, "code": e.code(), "message": e.to_string()}),
        };
    }
    Ok(Json(
        json!({"ok": key_ok, "message": key_msg, "key": key_info, "keySource": if cfg.settings.is_from_db("llm.apiKey") { "database" } else { "environment" }, "warnings": warnings, "models": models, "ping": ping_result}),
    ))
}

#[derive(Deserialize)]
pub struct UsageQuery {
    #[serde(default)]
    pub days: Option<i64>,
}

pub async fn llm_usage(State(state): State<AppState>, user: AuthUser, Query(q): Query<UsageQuery>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let days = q.days.unwrap_or(30).clamp(1, 365).to_string();
    let daily = sqlx::query(
        "select to_char(date_trunc('day', created_at), 'YYYY-MM-DD') d, count(*) calls, count(*) filter (where status = 'error') errors,
                coalesce(sum(prompt_tokens), 0)::bigint pt, coalesce(sum(completion_tokens), 0)::bigint ct, coalesce(sum(cost_usd), 0) cost
         from llm_calls where created_at > now() - ($1 || ' days')::interval group by 1 order by 1",
    )
    .bind(&days)
    .fetch_all(&state.pool)
    .await?;
    let by_model = sqlx::query(
        "select coalesce(model, requested_model) m, count(*) calls, coalesce(sum(cost_usd), 0) cost, coalesce(avg(latency_ms), 0)::int latency
         from llm_calls where created_at > now() - ($1 || ' days')::interval group by 1 order by cost desc limit 20",
    )
    .bind(&days)
    .fetch_all(&state.pool)
    .await?;
    let by_user = sqlx::query(
        "select coalesce(u.email, '—') email, count(*) calls, coalesce(sum(c.cost_usd), 0) cost from llm_calls c left join users u on u.id = c.user_id
         where c.created_at > now() - ($1 || ' days')::interval group by 1 order by cost desc limit 20",
    )
    .bind(&days)
    .fetch_all(&state.pool)
    .await?;
    let month: f64 = sqlx::query_scalar::<_, Option<f64>>("select sum(cost_usd) from llm_calls where created_at >= date_trunc('month', now())").fetch_one(&state.pool).await?.unwrap_or(0.0);
    let cfg = state.cfg().await?;
    Ok(Json(json!({
        "monthSpendUsd": month, "monthlyBudgetUsd": cfg.settings.num("llm.monthlyBudgetUsd"),
        "daily": daily.iter().map(|r| json!({"day": r.get::<String, _>("d"), "calls": r.get::<i64, _>("calls"), "errors": r.get::<i64, _>("errors"), "promptTokens": r.get::<i64, _>("pt"), "completionTokens": r.get::<i64, _>("ct"), "costUsd": r.get::<f64, _>("cost")})).collect::<Vec<_>>(),
        "byModel": by_model.iter().map(|r| json!({"model": r.get::<String, _>("m"), "calls": r.get::<i64, _>("calls"), "costUsd": r.get::<f64, _>("cost"), "avgLatencyMs": r.get::<i32, _>("latency")})).collect::<Vec<_>>(),
        "byUser": by_user.iter().map(|r| json!({"user": r.get::<String, _>("email"), "calls": r.get::<i64, _>("calls"), "costUsd": r.get::<f64, _>("cost")})).collect::<Vec<_>>(),
    })))
}

// ------------------------------------------------------------------ config items

pub async fn config_kinds(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let rows = sqlx::query("select kind, count(*) n, count(*) filter (where modified) modified, count(*) filter (where origin = 'custom') custom, count(*) filter (where update_available) updates from config_items group by kind").fetch_all(&state.pool).await?;
    let cfg = state.cfg().await?;
    Ok(Json(json!({
        "kinds": rows.iter().map(|r| json!({"kind": r.get::<String, _>("kind"), "count": r.get::<i64, _>("n"), "modified": r.get::<i64, _>("modified"), "custom": r.get::<i64, _>("custom"), "updatesAvailable": r.get::<i64, _>("updates")})).collect::<Vec<_>>(),
        "errors": cfg.errors.iter().map(|(k, i, e)| json!({"kind": k, "id": i, "error": e})).collect::<Vec<_>>(),
    })))
}

pub async fn config_list(State(state): State<AppState>, user: AuthUser, Path(kind): Path<String>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    if !ALL_KINDS.contains(&kind.as_str()) {
        return Err(ApiError::not_found("unknown config kind"));
    }
    let rows =
        sqlx::query("select id, data, origin, modified, update_available, updated_at, updated_by from config_items where kind = $1 order by sort, id").bind(&kind).fetch_all(&state.pool).await?;
    let defaults: std::collections::HashSet<String> = default_items().into_iter().filter(|d| d.kind == kind).map(|d| d.id).collect();
    Ok(Json(json!({"items": rows.iter().map(|r| {
        let id: String = r.get("id");
        json!({"id": id, "data": r.get::<Value, _>("data"), "origin": r.get::<String, _>("origin"), "modified": r.get::<bool, _>("modified"), "updateAvailable": r.get::<bool, _>("update_available"),
               "hasDefault": defaults.contains(&id), "updatedAt": r.get::<DateTime<Utc>, _>("updated_at"), "updatedBy": r.get::<String, _>("updated_by")})
    }).collect::<Vec<_>>()})))
}

/// A catalog item whose ranges allow no value (a minimum above the maximum, a default outside the range) is refused: the design check reads these
/// ranges (`Catalog::prop_bounds`), so one such item would make every write of a node of that kind fail.
async fn refuse_ranges_that_allow_nothing(state: &AppState, kind: &str, id: &str, data: &Value) -> ApiResult<()> {
    let cfg = state.cfg().await?;
    let worse = range_regressions(&cfg.catalog, kind, id, data);
    if worse.is_empty() {
        return Ok(());
    }
    Err(ApiError::bad_request("invalid_config", worse.join("; ")))
}

#[derive(Deserialize)]
pub struct PutItem {
    pub data: Value,
}

pub async fn config_put(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Json(b): Json<PutItem>,
) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    if !ALL_KINDS.contains(&kind.as_str()) {
        return Err(ApiError::not_found("unknown config kind"));
    }
    validate_item(&kind, &id, &b.data).map_err(|e| ApiError::bad_request("invalid_config", e))?;
    refuse_ranges_that_allow_nothing(&state, &kind, &id, &b.data).await?;
    let old: Option<Value> = sqlx::query_scalar("select data from config_items where kind = $1 and id = $2").bind(&kind).bind(&id).fetch_optional(&state.pool).await?;
    let r = sqlx::query("update config_items set data = $3, modified = (origin = 'default'), updated_at = now(), updated_by = $4 where kind = $1 and id = $2")
        .bind(&kind)
        .bind(&id)
        .bind(&b.data)
        .bind(&user.email)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("config item not found"));
    }
    changed(&state).await?;
    let changed_keys: Vec<String> = match (&old, &b.data) {
        (Some(Value::Object(o)), Value::Object(n)) => o.keys().chain(n.keys()).filter(|k| o.get(*k) != n.get(*k)).cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect(),
        _ => vec![],
    };
    audit::log(&state.pool, Some(&user), "config.update", &kind, &id, json!({"changedKeys": changed_keys}), &ip(&state, &headers, addr)).await;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct CreateItem {
    pub id: String,
    pub data: Value,
}

pub async fn config_create(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(kind): Path<String>,
    Json(b): Json<CreateItem>,
) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    if !ALL_KINDS.contains(&kind.as_str()) || single_document_id(&kind).is_some() {
        return Err(ApiError::bad_request("invalid_kind", MSG_SINGLE_DOCUMENT));
    }
    if !is_valid_item_id(&b.id) {
        return Err(ApiError::bad_request("invalid_id", MSG_INVALID_ITEM_ID));
    }
    validate_item(&kind, &b.id, &b.data).map_err(|e| ApiError::bad_request("invalid_config", e))?;
    refuse_ranges_that_allow_nothing(&state, &kind, &b.id, &b.data).await?;
    let sort: i32 = sqlx::query_scalar("select coalesce(max(sort), 0) + 1 from config_items where kind = $1").bind(&kind).fetch_one(&state.pool).await?;
    sqlx::query("insert into config_items (kind, id, data, origin, modified, sort, updated_by) values ($1, $2, $3, 'custom', true, $4, $5)")
        .bind(&kind)
        .bind(&b.id)
        .bind(&b.data)
        .bind(sort)
        .bind(&user.email)
        .execute(&state.pool)
        .await?;
    changed(&state).await?;
    audit::log(&state.pool, Some(&user), "config.create", &kind, &b.id, json!({}), &ip(&state, &headers, addr)).await;
    Ok(Json(json!({"ok": true})))
}

pub async fn config_delete(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let origin: Option<String> = sqlx::query_scalar("select origin from config_items where kind = $1 and id = $2").bind(&kind).bind(&id).fetch_optional(&state.pool).await?;
    match origin.as_deref() {
        None => return Err(ApiError::not_found("config item not found")),
        Some("custom") => {}
        Some(_) => return Err(ApiError::conflict("not_deletable", "only custom items can be deleted; built-in items can be disabled or reset")),
    }
    sqlx::query("delete from config_items where kind = $1 and id = $2 and origin = 'custom'").bind(&kind).bind(&id).execute(&state.pool).await?;
    changed(&state).await?;
    audit::log(&state.pool, Some(&user), "config.delete", &kind, &id, json!({}), &ip(&state, &headers, addr)).await;
    Ok(Json(json!({"ok": true})))
}

pub async fn config_reset(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let d = default_items().into_iter().find(|d| d.kind == kind && d.id == id).ok_or_else(|| ApiError::not_found("no built-in default for this item"))?;
    let h = crate::crypto::sha256_hex(serde_json::to_string(&d.data).unwrap_or_default().as_bytes());
    sqlx::query("update config_items set data = $3, modified = false, update_available = false, default_hash = $4, updated_at = now(), updated_by = $5 where kind = $1 and id = $2")
        .bind(&kind)
        .bind(&id)
        .bind(&d.data)
        .bind(h)
        .bind(&user.email)
        .execute(&state.pool)
        .await?;
    changed(&state).await?;
    audit::log(&state.pool, Some(&user), "config.reset", &kind, &id, json!({}), &ip(&state, &headers, addr)).await;
    Ok(Json(json!({"ok": true})))
}

// ------------------------------------------------------------------ release tables

#[derive(Deserialize, Default)]
pub struct SyncBody {
    /// Tables to refresh (all with an allowed source when empty).
    #[serde(default)]
    pub ids: Vec<String>,
}

/// Fetch the release tables from their sources now.
pub async fn versions_sync(State(state): State<AppState>, user: AuthUser, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, body: Option<Json<SyncBody>>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let ids = body.map(|b| b.0.ids).unwrap_or_default();
    let out = crate::versions::sync_by(&state, &user, &ids, &ip(&state, &headers, addr)).await?;
    Ok(Json(json!({"results": out, "ok": out.iter().filter(|o| o.ok).count(), "failed": out.iter().filter(|o| !o.ok).count()})))
}

/// How fresh the release tables are and how the last refresh went.
pub async fn versions_status(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let cfg = state.cfg().await?;
    let today = crate::versions::today_utc();
    let day = engine::lifecycle::parse_date(&today).unwrap_or(0);
    let stale_days = cfg.engine.lifecycle.stale_days;
    let ages: Vec<(String, Option<i64>)> = cfg.versions.iter().map(|v| (v.id.clone(), v.age_days(day))).collect();
    Ok(Json(json!({
        "today": today,
        "tables": ages.len(),
        "staleDays": stale_days,
        "stale": ages.iter().filter(|(_, a)| a.map(|a| a > stale_days).unwrap_or(true)).map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        "oldestDays": ages.iter().filter_map(|(_, a)| *a).max(),
        "autoSyncDays": cfg.settings.num("versions.autoSyncDays"),
        "sourceBases": cfg.settings.list("versions.sourceBases"),
        "last": crate::versions::last_sync(&state).await,
    })))
}

/// Backup / migration between environments: everything an administrator changed.
pub async fn config_export(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let rows = sqlx::query("select kind, id, data, origin from config_items where modified or origin = 'custom' order by kind, sort, id").fetch_all(&state.pool).await?;
    let cfg = state.cfg().await?;
    let mut changed_settings = Map::new();
    for d in settings::registry() {
        if d.ty != "secret" && cfg.settings.is_from_db(&d.key) {
            changed_settings.insert(d.key.clone(), cfg.settings.get(&d.key));
        }
    }
    Ok(Json(json!({
        "format": "infra-architect-config/1",
        "exportedAt": Utc::now(),
        "settings": changed_settings,
        "items": rows.iter().map(|r| json!({"kind": r.get::<String, _>("kind"), "id": r.get::<String, _>("id"), "data": r.get::<Value, _>("data"), "origin": r.get::<String, _>("origin")})).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBody {
    #[serde(default)]
    pub settings: Map<String, Value>,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub dry_run: bool,
}

/// An id in a message about an item: a 130-character id is quoted by its first characters, not whole.
fn shown_id(id: &str) -> String {
    if id.chars().count() > 48 {
        format!("{}…", id.chars().take(47).collect::<String>())
    } else {
        id.to_string()
    }
}

/// Import a configuration backup: items and settings, all or nothing. Every item is held to what creating one by hand would hold it to
/// (a well-formed id, no second document in a kind that has one, a valid body); the writes - items, settings and the revision bump that
/// tells the other replicas - are one transaction, so a value the database refuses leaves the instance as it was.
pub async fn config_import(State(state): State<AppState>, user: AuthUser, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(b): Json<ImportBody>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let mut problems = vec![];
    for (k, v) in &b.settings {
        match settings::def(k) {
            None => problems.push(format!("unknown setting '{k}'")),
            Some(d) => {
                if d.ty == "secret" {
                    problems.push(format!("secret '{k}' cannot be imported"));
                } else if let Err(e) = settings::validate(d, v) {
                    problems.push(format!("{k}: {e}"));
                }
            }
        }
    }
    let mut items = vec![];
    for it in &b.items {
        let (Some(kind), Some(id), Some(data)) = (it["kind"].as_str(), it["id"].as_str(), it.get("data")) else {
            problems.push("item without kind/id/data".to_string());
            continue;
        };
        let name = format!("{kind}:{}", shown_id(id));
        if !ALL_KINDS.contains(&kind) {
            problems.push(format!("{name}: unknown kind"));
            continue;
        }
        if !is_valid_item_id(id) {
            problems.push(format!("{name}: {MSG_INVALID_ITEM_ID}"));
            continue;
        }
        if single_document_id(kind).is_some_and(|only| only != id) {
            problems.push(format!("{name}: {MSG_SINGLE_DOCUMENT}"));
            continue;
        }
        if let Err(e) = validate_item(kind, id, data) {
            problems.push(format!("{name}: {e}"));
            continue;
        }
        items.push((kind.to_string(), id.to_string(), data.clone()));
    }
    if !problems.is_empty() || b.dry_run {
        return Ok(Json(json!({"ok": problems.is_empty(), "problems": problems, "items": items.len(), "settings": b.settings.len(), "applied": false})));
    }
    let mut tx = state.pool.begin().await?;
    for (kind, id, data) in &items {
        // a new item goes after the ones the kind already has, as when it is created by hand; an item that exists keeps its place
        sqlx::query(
            "insert into config_items (kind, id, data, origin, modified, sort, updated_by)
             values ($1, $2, $3, 'custom', true, (select coalesce(max(sort), 0) + 1 from config_items where kind = $1), $4)
             on conflict (kind, id) do update set data = $3, modified = (config_items.origin = 'default'), updated_at = now(), updated_by = $4",
        )
        .bind(kind)
        .bind(id)
        .bind(data)
        .bind(&user.email)
        .execute(&mut *tx)
        .await?;
    }
    if !b.settings.is_empty() {
        settings::save_in(&mut tx, &state.keys, &b.settings, &user.email).await.map_err(setting_error)?;
    }
    bump_rev(&mut *tx).await?;
    tx.commit().await?;
    state.config.invalidate().await;
    audit::log(&state.pool, Some(&user), "config.import", "config", "", json!({"items": items.len(), "settings": b.settings.len()}), &ip(&state, &headers, addr)).await;
    Ok(Json(json!({"ok": true, "problems": [], "items": items.len(), "settings": b.settings.len(), "applied": true})))
}

// ------------------------------------------------------------------ users

pub async fn users_list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let rows = sqlx::query("select id, email, name, role, disabled, must_change_password, created_at, last_login_at, case when locked_until > now() then locked_until end as locked_until from users order by lower(email)").fetch_all(&state.pool).await?;
    Ok(Json(json!({"users": rows.iter().map(|r| json!({
        "id": r.get::<Uuid, _>("id"), "email": r.get::<String, _>("email"), "name": r.get::<String, _>("name"), "role": r.get::<String, _>("role"),
        "disabled": r.get::<bool, _>("disabled"), "mustChangePassword": r.get::<bool, _>("must_change_password"),
        "createdAt": r.get::<DateTime<Utc>, _>("created_at"), "lastLoginAt": r.get::<Option<DateTime<Utc>>, _>("last_login_at"), "lockedUntil": r.get::<Option<DateTime<Utc>>, _>("locked_until"),
    })).collect::<Vec<_>>()})))
}

#[derive(Deserialize)]
pub struct NewUser {
    pub email: String,
    #[serde(default)]
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub password: Option<String>,
}

pub async fn users_create(State(state): State<AppState>, user: AuthUser, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(b): Json<NewUser>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let cfg = state.cfg().await?;
    let email = b.email.trim().to_lowercase();
    if !email.contains('@') || !is_acceptable_email(&email) {
        return Err(ApiError::bad_request("invalid_email", "invalid email"));
    }
    let name = text(&cfg, TextField::Name, "name", b.name.trim())?;
    let role = Role::parse(&b.role).ok_or_else(|| ApiError::bad_request("invalid_role", "role must be admin, editor or viewer"))?;
    let (pw, generated) = match b.password.filter(|p| !p.is_empty()) {
        Some(p) => {
            check_password_policy(&cfg.settings, &p)?;
            (p, false)
        }
        None => (random_password(), true),
    };
    let id: Uuid = sqlx::query_scalar("insert into users (email, name, role, password_hash, must_change_password) values ($1, $2, $3, $4, $5) returning id")
        .bind(&email)
        .bind(name)
        .bind(role.as_str())
        .bind(state.passwords.hash(pw.clone()).await?)
        .bind(generated)
        .fetch_one(&state.pool)
        .await?;
    audit::log(&state.pool, Some(&user), "user.create", "user", &id.to_string(), json!({"email": email, "role": role.as_str()}), &ip(&state, &headers, addr)).await;
    Ok(Json(json!({"id": id, "temporaryPassword": if generated { Some(pw) } else { None }})))
}

#[derive(Deserialize)]
pub struct PatchUser {
    pub name: Option<String>,
    pub role: Option<String>,
    pub disabled: Option<bool>,
}

async fn other_admins(state: &AppState, except: Uuid) -> ApiResult<i64> {
    Ok(sqlx::query_scalar("select count(*) from users where role = 'admin' and not disabled and id <> $1").bind(except).fetch_one(&state.pool).await?)
}

pub async fn users_patch(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<PatchUser>,
) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let cfg = state.cfg().await?;
    if let Some(n) = &b.name {
        text(&cfg, TextField::Name, "name", n.trim())?;
    }
    let new_role = match &b.role {
        Some(r) => Some(Role::parse(r).ok_or_else(|| ApiError::bad_request("invalid_role", "role must be admin, editor or viewer"))?),
        None => None,
    };
    let (cur_role, cur_disabled): (String, bool) =
        sqlx::query_as("select role, disabled from users where id = $1").bind(id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    if id == user.id && (new_role.map(|r| r != Role::Admin).unwrap_or(false) || b.disabled == Some(true)) {
        return Err(ApiError::conflict("self_lockout", "you cannot demote or disable your own account"));
    }
    let losing_admin = (cur_role == "admin" && !cur_disabled) && (new_role.map(|r| r != Role::Admin).unwrap_or(false) || b.disabled == Some(true));
    if losing_admin && other_admins(&state, id).await? == 0 {
        return Err(ApiError::conflict("last_admin", "there must be at least one active administrator"));
    }
    sqlx::query("update users set name = coalesce($2, name), role = coalesce($3, role), disabled = coalesce($4, disabled), session_version = session_version + (case when $3 is not null or $4 is not null then 1 else 0 end) where id = $1")
        .bind(id).bind(b.name.map(|n| n.trim().to_string())).bind(new_role.map(|r| r.as_str())).bind(b.disabled)
        .execute(&state.pool).await?;
    forget_user(&state, id);
    audit::log(&state.pool, Some(&user), "user.update", "user", &id.to_string(), json!({"role": b.role, "disabled": b.disabled}), &ip(&state, &headers, addr)).await;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize, Default)]
pub struct ResetPw {
    #[serde(default)]
    pub password: Option<String>,
}

pub async fn users_reset_password(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: Option<Json<ResetPw>>,
) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let cfg = state.cfg().await?;
    let given = body.and_then(|b| b.0.password).filter(|p| !p.is_empty());
    let (pw, generated) = match given {
        Some(p) => {
            check_password_policy(&cfg.settings, &p)?;
            (p, false)
        }
        None => (random_password(), true),
    };
    let hash = state.passwords.hash(pw.clone()).await?;
    let email: Option<String> = sqlx::query_scalar(
        "update users set password_hash = $2, must_change_password = true, failed_attempts = 0, locked_until = null, session_version = session_version + 1 where id = $1 returning lower(email)",
    )
    .bind(id)
    .bind(hash)
    .fetch_optional(&state.pool)
    .await?;
    let Some(email) = email else {
        return Err(ApiError::not_found("user not found"));
    };
    // the reset is how an administrator unlocks an account: for every address it was counted from, and for the wrong current passwords alike
    ratelimit::clear_prefix(&state.pool, &failure_key(&email)).await?;
    ratelimit::clear(&state.pool, &pwchange_key(id)).await?;
    forget_user(&state, id);
    audit::log(&state.pool, Some(&user), "user.reset_password", "user", &id.to_string(), json!({}), &ip(&state, &headers, addr)).await;
    Ok(Json(json!({"temporaryPassword": if generated { Some(pw) } else { None }})))
}

// ------------------------------------------------------------------ audit & system

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
}

pub async fn audit_list(State(state): State<AppState>, user: AuthUser, Query(q): Query<AuditQuery>) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let rows = sqlx::query(
        "select id, at, actor, action, target_type, target_id, details, ip from audit_log
         where ($1::bigint is null or id < $1) and ($2::text is null or left(action, length($2)) = $2) and ($3::text is null or lower(actor) like '%' || lower($3) || '%')
         order by id desc limit $4",
    )
    .bind(q.before)
    .bind(q.action.filter(|s| !s.is_empty()))
    .bind(q.actor.filter(|s| !s.is_empty()))
    .bind(q.limit.unwrap_or(100).clamp(1, 500))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({"events": rows.iter().map(|r| json!({
        "id": r.get::<i64, _>("id"), "at": r.get::<DateTime<Utc>, _>("at"), "actor": r.get::<String, _>("actor"), "action": r.get::<String, _>("action"),
        "targetType": r.get::<String, _>("target_type"), "targetId": r.get::<String, _>("target_id"), "details": r.get::<Value, _>("details"), "ip": r.get::<String, _>("ip"),
    })).collect::<Vec<_>>()})))
}

pub async fn system(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    user.require_admin()?;
    let cfg = state.cfg().await?;
    let t = std::time::Instant::now();
    let version: String = sqlx::query_scalar("select version()").fetch_one(&state.pool).await.unwrap_or_default();
    let db_ms = t.elapsed().as_millis() as u64;
    let migrations: i64 = sqlx::query_scalar("select count(*) from _sqlx_migrations where success").fetch_one(&state.pool).await.unwrap_or(0);
    let jobs = sqlx::query("select status, count(*) n from ai_jobs where created_at > now() - interval '1 day' group by status").fetch_all(&state.pool).await?;
    let counts = sqlx::query("select (select count(*) from projects where deleted_at is null) projects, (select count(*) from users) users").fetch_one(&state.pool).await?;
    let posture: Vec<Value> = crate::posture::gather(&state).await?.iter().map(crate::posture::describe).collect();
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"), "engineVersion": engine::evaluate::ENGINE_VERSION, "instance": state.boot.instance_id,
        "uptimeSeconds": state.started_at.elapsed().as_secs(), "production": state.boot.production,
        "db": {"ok": true, "latencyMs": db_ms, "version": version, "migrations": migrations, "poolSize": state.pool.size(), "poolIdle": state.pool.num_idle()},
        "configRev": cfg.rev,
        "configErrors": cfg.errors.iter().map(|(k, i, e)| json!({"kind": k, "id": i, "error": e})).collect::<Vec<_>>(),
        "jobsLast24h": jobs.iter().map(|r| json!({"status": r.get::<String, _>("status"), "count": r.get::<i64, _>("n")})).collect::<Vec<_>>(),
        "counts": {"projects": counts.get::<i64, _>("projects"), "users": counts.get::<i64, _>("users")},
        "llm": {"enabled": cfg.settings.boolean("llm.enabled"), "keyConfigured": state.llm_key(&cfg).is_some(), "keySource": if cfg.settings.is_from_db("llm.apiKey") { "database" } else if state.boot.openrouter_api_key.is_some() { "environment" } else { "none" }},
        // what the installation's configuration says about its security, judged in production only (posture.rs); an empty list is "nothing found", not "secure"
        "posture": posture,
        "env": {
            "APP_ENV": if state.boot.production { "production" } else { "development" },
            "COOKIE_SECURE": state.boot.cookie_secure, "TRUST_PROXY": state.boot.trust_proxy, "TRUST_PROXY_HOPS": state.boot.trust_proxy_hops, "METRICS_TOKEN": state.boot.metrics_token.is_some(), "METRICS_PORT": state.boot.metrics_port,
            "OPENROUTER_API_KEY": state.boot.openrouter_api_key.is_some(), "DB_MAX_CONNECTIONS": state.boot.db_max_connections,
        },
    })))
}
