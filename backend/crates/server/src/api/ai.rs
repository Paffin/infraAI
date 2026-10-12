//! AI endpoints: jobs, chat history and proposals (preview → explicit apply / reject).

use crate::api::projects::engine_waivers;
use crate::api_support::{lang_of, limits, parse_arch, run_evaluate, summarize};
use crate::audit;
use crate::auth::{client_ip, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use engine::model::Architecture;
use engine::ops::{apply_ops, Op};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::net::SocketAddr;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewJob {
    /// `generate` | `patch` | `fix` | `chat`
    pub kind: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub finding_id: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
}

pub async fn create_job(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(b): Json<NewJob>,
) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    match b.kind.as_str() {
        "generate" | "patch" | "fix" => user.require_editor()?,
        "chat" => {}
        _ => return Err(ApiError::bad_request("bad_kind", "kind must be generate, patch, fix or chat")),
    }
    if !cfg.settings.boolean("llm.enabled") {
        return Err(ApiError::new(axum::http::StatusCode::CONFLICT, "llm_disabled", "AI is disabled by the administrator; the editor works in manual mode"));
    }
    if state.llm_key(&cfg).is_none() {
        return Err(ApiError::new(axum::http::StatusCode::CONFLICT, "llm_not_configured", "the OpenRouter API key is not configured"));
    }
    let max_chars = cfg.settings.num("llm.maxPromptChars") as usize;
    let prompt = b.prompt.trim().to_string();
    if b.kind != "fix" && prompt.is_empty() {
        return Err(ApiError::bad_request("empty_prompt", "the request is empty"));
    }
    if prompt.chars().count() > max_chars {
        return Err(ApiError::bad_request("prompt_too_long", format!("the request is longer than {max_chars} characters")));
    }
    if b.kind == "fix" && b.finding_id.as_deref().unwrap_or("").is_empty() {
        return Err(ApiError::bad_request("missing_finding", "findingId is required"));
    }
    let exists: Option<(i32,)> = sqlx::query_as("select 1 from projects where id = $1 and deleted_at is null").bind(project_id).fetch_optional(&state.pool).await?;
    if exists.is_none() {
        return Err(ApiError::not_found("project not found"));
    }
    // fair use, enforced in the database so it holds across replicas
    let per_hour = cfg.settings.num("llm.perUserPerHour") as i64;
    if per_hour > 0 {
        let (n,): (i64,) = sqlx::query_as("select count(*) from ai_jobs where user_id = $1 and created_at > now() - interval '1 hour'").bind(user.id).fetch_one(&state.pool).await?;
        if n >= per_hour {
            return Err(ApiError::too_many("ai_rate_limited", format!("limit of {per_hour} AI requests per hour reached")));
        }
    }
    // one running job per project at a time keeps proposals coherent
    let busy: Option<(Uuid,)> = sqlx::query_as("select id from ai_jobs where project_id = $1 and status in ('queued','running') and heartbeat_at > now() - interval '120 seconds' limit 1").bind(project_id).fetch_optional(&state.pool).await?;
    if let Some((id,)) = busy {
        return Err(ApiError::conflict("job_running", "another AI request is already running for this project").with_details(json!({"jobId": id})));
    }
    let lang = lang_of(&cfg, b.lang.as_deref());
    let input = json!({"prompt": prompt, "findingId": b.finding_id, "lang": lang});
    let id: Uuid = sqlx::query_scalar("insert into ai_jobs (project_id, user_id, kind, status, input, instance) values ($1, $2, $3, 'queued', $4, $5) returning id")
        .bind(project_id)
        .bind(user.id)
        .bind(&b.kind)
        .bind(&input)
        .bind(&state.boot.instance_id)
        .fetch_one(&state.pool)
        .await?;
    let shown = if b.kind == "fix" { format!("fix:{}", b.finding_id.clone().unwrap_or_default()) } else { prompt.clone() };
    sqlx::query("insert into chat_messages (project_id, role, content, user_id, job_id) values ($1, 'user', $2, $3, $4)").bind(project_id).bind(&shown).bind(user.id).bind(id).execute(&state.pool).await?;
    audit::log(&state.pool, Some(&user), "ai.job", "project", &project_id.to_string(), json!({"kind": b.kind, "jobId": id}), &client_ip(&headers, Some(addr), state.boot.trust_proxy)).await;
    state.jobs.spawn(state.clone(), id);
    Ok(Json(json!({"jobId": id})))
}

fn job_json(r: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": r.get::<Uuid, _>("id"), "projectId": r.get::<Option<Uuid>, _>("project_id"), "kind": r.get::<String, _>("kind"), "status": r.get::<String, _>("status"),
        "steps": r.get::<Value, _>("steps"), "result": r.get::<Option<Value>, _>("result"), "error": r.get::<Option<String>, _>("error"), "errorCode": r.get::<Option<String>, _>("error_code"),
        "createdAt": r.get::<DateTime<Utc>, _>("created_at"), "finishedAt": r.get::<Option<DateTime<Utc>>, _>("finished_at"),
    })
}

pub async fn get_job(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let r = sqlx::query("select id, project_id, kind, status, steps, result, error, error_code, created_at, finished_at from ai_jobs where id = $1").bind(id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("job not found"))?;
    Ok(Json(job_json(&r)))
}

pub async fn cancel_job(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let r = sqlx::query("update ai_jobs set cancel_requested = true where id = $1 and status in ('queued','running')").bind(id).execute(&state.pool).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("no running job with this id"));
    }
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct ChatQuery {
    #[serde(default)]
    pub after: Option<i64>,
}

pub async fn chat_history(State(state): State<AppState>, _user: AuthUser, Path(project_id): Path<Uuid>, Query(q): Query<ChatQuery>) -> ApiResult<Json<Value>> {
    let rows = sqlx::query(
        "select m.id, m.role, m.content, m.proposal_id, m.created_at, p.status as proposal_status
         from chat_messages m left join proposals p on p.id = m.proposal_id
         where m.project_id = $1 and m.id > $2 order by m.id asc limit 200",
    )
    .bind(project_id)
    .bind(q.after.unwrap_or(0))
    .fetch_all(&state.pool)
    .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| json!({"id": r.get::<i64, _>("id"), "role": r.get::<String, _>("role"), "content": r.get::<String, _>("content"), "proposalId": r.get::<Option<Uuid>, _>("proposal_id"), "proposalStatus": r.get::<Option<String>, _>("proposal_status"), "createdAt": r.get::<DateTime<Utc>, _>("created_at")}))
        .collect();
    let running: Option<(Uuid,)> = sqlx::query_as("select id from ai_jobs where project_id = $1 and status in ('queued','running') and heartbeat_at > now() - interval '120 seconds' order by created_at desc limit 1").bind(project_id).fetch_optional(&state.pool).await?;
    Ok(Json(json!({"messages": items, "runningJobId": running.map(|r| r.0)})))
}

pub async fn clear_chat(State(state): State<AppState>, user: AuthUser, Path(project_id): Path<Uuid>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    sqlx::query("delete from chat_messages where project_id = $1").bind(project_id).execute(&state.pool).await?;
    Ok(Json(json!({"ok": true})))
}

pub async fn get_proposal(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let r = sqlx::query("select id, project_id, base_rev, change, rationale, preview, status, created_at from proposals where id = $1").bind(id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("proposal not found"))?;
    let change: Value = r.get("change");
    // the full architecture of a "replace" proposal is large; the client gets it on demand
    Ok(Json(json!({
        "id": r.get::<Uuid, _>("id"), "projectId": r.get::<Uuid, _>("project_id"), "baseRev": r.get::<i64, _>("base_rev"), "status": r.get::<String, _>("status"),
        "mode": change["mode"], "ops": change.get("ops"), "architecture": change.get("architecture"),
        "rationale": r.get::<String, _>("rationale"), "preview": r.get::<Value, _>("preview"), "createdAt": r.get::<DateTime<Utc>, _>("created_at"),
    })))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApplyProposal {
    #[serde(default)]
    pub expected_rev: Option<i64>,
}

/// Apply a pending proposal to the project's working copy. A snapshot of the previous state is
/// stored as a version first, so the change can always be rolled back.
pub async fn apply_proposal(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: Option<Json<ApplyProposal>>,
) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let expected = body.and_then(|b| b.0.expected_rev);
    let mut tx = state.pool.begin().await?;
    let p = sqlx::query("select project_id, change, rationale, status from proposals where id = $1 for update").bind(id).fetch_optional(&mut *tx).await?.ok_or_else(|| ApiError::not_found("proposal not found"))?;
    let project_id: Uuid = p.get("project_id");
    let status: String = p.get("status");
    if status != "pending" {
        return Err(ApiError::conflict("proposal_closed", format!("the proposal is already {status}")));
    }
    let change: Value = p.get("change");
    let rationale: String = p.get("rationale");
    let pr = sqlx::query("select draft, draft_rev, head_version from projects where id = $1 and deleted_at is null for update").bind(project_id).fetch_optional(&mut *tx).await?.ok_or_else(|| ApiError::not_found("project not found"))?;
    let draft: Value = pr.get("draft");
    let rev: i64 = pr.get("draft_rev");
    let head: i32 = pr.get("head_version");
    if let Some(e) = expected {
        if e != rev {
            return Err(ApiError::conflict("conflict", "the project was changed after the proposal was made; review it again").with_details(json!({"currentRev": rev})));
        }
    }
    let base: Architecture = parse_arch(draft.clone())?;
    let new_arch: Architecture = match change["mode"].as_str() {
        Some("replace") => parse_arch(change["architecture"].clone())?,
        Some("ops") => {
            let ops: Vec<Op> = serde_json::from_value(change["ops"].clone())?;
            match apply_ops(&base, &ops, &cfg.catalog, &limits(&cfg)) {
                Ok(o) => o.architecture,
                Err(e) => {
                    sqlx::query("update proposals set status = 'stale', decided_at = now() where id = $1").bind(id).execute(&mut *tx).await?;
                    tx.commit().await?;
                    return Err(ApiError::conflict("proposal_stale", "the proposal no longer applies to the current architecture").with_details(json!({"issues": e.issues})));
                }
            }
        }
        _ => return Err(ApiError::internal("proposal has an unknown mode")),
    };

    // snapshot the state before the change (only if it differs from the latest version)
    let last: Option<(Value,)> = sqlx::query_as("select architecture from versions where project_id = $1 and n = $2").bind(project_id).bind(head).fetch_optional(&mut *tx).await?;
    let mut n = head;
    let waivers = engine_waivers(&state, project_id).await?;
    if last.map(|l| l.0 != draft).unwrap_or(true) {
        n += 1;
        let rep = run_evaluate(cfg.clone(), base.clone(), waivers.clone(), cfg.settings.str("app.defaultLanguage")).await?;
        sqlx::query("insert into versions (project_id, n, message, author_id, author, architecture, summary, engine_version) values ($1, $2, $3, $4, $5, $6, $7, $8)")
            .bind(project_id).bind(n).bind("Before AI change").bind(user.id).bind(&user.email).bind(&draft)
            .bind(serde_json::to_value(summarize(&rep))?).bind(engine::evaluate::ENGINE_VERSION as i32)
            .execute(&mut *tx).await?;
    }
    n += 1;
    let rep = run_evaluate(cfg.clone(), new_arch.clone(), waivers, cfg.settings.str("app.defaultLanguage")).await?;
    let msg = format!("AI: {}", rationale.chars().take(80).collect::<String>());
    sqlx::query("insert into versions (project_id, n, message, author_id, author, architecture, summary, engine_version) values ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(project_id).bind(n).bind(msg).bind(user.id).bind(&user.email).bind(serde_json::to_value(&new_arch)?)
        .bind(serde_json::to_value(summarize(&rep))?).bind(engine::evaluate::ENGINE_VERSION as i32)
        .execute(&mut *tx).await?;
    let (new_rev,): (i64,) = sqlx::query_as("update projects set draft = $2, draft_rev = draft_rev + 1, head_version = $3, updated_at = now() where id = $1 returning draft_rev")
        .bind(project_id).bind(serde_json::to_value(&new_arch)?).bind(n).fetch_one(&mut *tx).await?;
    sqlx::query("update proposals set status = 'applied', decided_at = now() where id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    audit::log(&state.pool, Some(&user), "ai.apply", "project", &project_id.to_string(), json!({"proposal": id}), &client_ip(&headers, Some(addr), state.boot.trust_proxy)).await;
    Ok(Json(json!({"rev": new_rev, "architecture": new_arch, "version": n})))
}

pub async fn reject_proposal(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let r = sqlx::query("update proposals set status = 'rejected', decided_at = now() where id = $1 and status = 'pending'").bind(id).execute(&state.pool).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::conflict("proposal_closed", "the proposal is not pending"));
    }
    Ok(Json(json!({"ok": true})))
}
