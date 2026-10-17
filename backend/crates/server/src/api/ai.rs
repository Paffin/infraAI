//! AI endpoints: jobs, chat history and proposals (preview → explicit apply / reject).

use crate::api::projects::{append_version, engine_waivers, load_draft, lock_project, VersionEntry, REREAD_ATTEMPTS};
use crate::api_support::{evaluated_summary, lang_of, limits, parse_arch, text, TextField};
use crate::audit;
use crate::auth::{client_ip, AuthUser};
use crate::config_store::Loaded;
use crate::error::{ApiError, ApiResult};
use crate::json::{Json, Path, Query};
use crate::state::AppState;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use engine::model::Architecture;
use engine::ops::{apply_ops, Op};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::net::SocketAddr;
use std::sync::Arc;
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
    let asked = prompt.chars().count();
    if asked > max_chars {
        return Err(ApiError::bad_request("prompt_too_long", format!("the request is longer than {max_chars} characters")).with_details(json!({"field": "prompt", "max": max_chars, "actual": asked})));
    }
    if b.kind == "fix" && b.finding_id.as_deref().unwrap_or("").is_empty() {
        return Err(ApiError::bad_request("missing_finding", "findingId is required"));
    }
    // the id is echoed into the chat history and the prompt: bounded like the rest of what a person writes
    if let Some(f) = &b.finding_id {
        text(&cfg, TextField::Id, "findingId", f)?;
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
    let busy: Option<(Uuid,)> = sqlx::query_as("select id from ai_jobs where project_id = $1 and status in ('queued','running') and heartbeat_at > now() - interval '120 seconds' limit 1")
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await?;
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
    sqlx::query("insert into chat_messages (project_id, role, content, user_id, job_id) values ($1, 'user', $2, $3, $4)")
        .bind(project_id)
        .bind(&shown)
        .bind(user.id)
        .bind(id)
        .execute(&state.pool)
        .await?;
    audit::log(&state.pool, Some(&user), "ai.job", "project", &project_id.to_string(), json!({"kind": b.kind, "jobId": id}), &client_ip(&headers, Some(addr), &state.boot)).await;
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

/// The visible jobs: those of live projects (a job without a project stays visible). A deleted project takes its jobs, proposals and chat with it.
pub async fn get_job(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let r = sqlx::query(
        "select j.id, j.project_id, j.kind, j.status, j.steps, j.result, j.error, j.error_code, j.created_at, j.finished_at
         from ai_jobs j left join projects p on p.id = j.project_id where j.id = $1 and (j.project_id is null or p.deleted_at is null)",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("job not found"))?;
    Ok(Json(job_json(&r)))
}

pub async fn cancel_job(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let r = sqlx::query(
        "update ai_jobs set cancel_requested = true where id = $1 and status in ('queued','running')
         and (project_id is null or exists (select 1 from projects p where p.id = ai_jobs.project_id and p.deleted_at is null))",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;
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
    // the project is part of the query: no row at all means it is gone (deleted, or never made); a live project without messages is one row with nulls
    let rows = sqlx::query(
        "select m.id, m.role, m.content, m.proposal_id, m.created_at, pr.status as proposal_status
         from projects p left join chat_messages m on m.project_id = p.id and m.id > $2 left join proposals pr on pr.id = m.proposal_id
         where p.id = $1 and p.deleted_at is null order by m.id asc limit 200",
    )
    .bind(project_id)
    .bind(q.after.unwrap_or(0))
    .fetch_all(&state.pool)
    .await?;
    if rows.is_empty() {
        return Err(ApiError::not_found("project not found"));
    }
    let items: Vec<Value> = rows
        .into_iter()
        .filter(|r| r.get::<Option<i64>, _>("id").is_some())
        .map(|r| json!({"id": r.get::<i64, _>("id"), "role": r.get::<String, _>("role"), "content": r.get::<String, _>("content"), "proposalId": r.get::<Option<Uuid>, _>("proposal_id"), "proposalStatus": r.get::<Option<String>, _>("proposal_status"), "createdAt": r.get::<DateTime<Utc>, _>("created_at")}))
        .collect();
    let running: Option<(Uuid,)> =
        sqlx::query_as("select id from ai_jobs where project_id = $1 and status in ('queued','running') and heartbeat_at > now() - interval '120 seconds' order by created_at desc limit 1")
            .bind(project_id)
            .fetch_optional(&state.pool)
            .await?;
    Ok(Json(json!({"messages": items, "runningJobId": running.map(|r| r.0)})))
}

pub async fn clear_chat(State(state): State<AppState>, user: AuthUser, Path(project_id): Path<Uuid>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    // one statement: the delete runs whether or not the final select reads it (a data-modifying `with` always runs to completion), and the
    // count says whether there was a live project to delete from
    let alive: i64 = sqlx::query_scalar(
        "with p as (select id from projects where id = $1 and deleted_at is null),
              gone as (delete from chat_messages m using p where m.project_id = p.id)
         select count(*) from p",
    )
    .bind(project_id)
    .fetch_one(&state.pool)
    .await?;
    if alive == 0 {
        return Err(ApiError::not_found("project not found"));
    }
    Ok(Json(json!({"ok": true})))
}

pub async fn get_proposal(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let r = sqlx::query(
        "select pr.id, pr.project_id, pr.base_rev, pr.change, pr.rationale, pr.preview, pr.status, pr.created_at
         from proposals pr join projects p on p.id = pr.project_id and p.deleted_at is null where pr.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("proposal not found"))?;
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

/// What one pass of an apply came to.
enum Attempt {
    Applied {
        project_id: Uuid,
        rev: i64,
        architecture: Value,
        version: i32,
    },
    /// The draft or its history moved while the pass was evaluating; nothing was written.
    Moved(Uuid),
}

fn proposal_closed(status: &str) -> ApiError {
    ApiError::conflict("proposal_closed", format!("the proposal is already {status}"))
}

fn draft_moved(rev: i64) -> ApiError {
    ApiError::conflict("conflict", "the project was changed after the proposal was made; review it again").with_details(json!({"currentRev": rev}))
}

/// Apply a pending proposal to the project's working copy. A snapshot of the previous state is
/// stored as a version first, so the change can always be rolled back.
///
/// The slow part (evaluating the old and the new architecture) holds neither a transaction nor a lock: a transaction waiting for CPU keeps a
/// pooled connection for seconds, and a burst of applies then leaves the whole API without a database. Everything is read through the pool,
/// evaluated, and only then a short transaction takes the locks and checks that nothing moved. If the draft did move: with `expectedRev` that
/// is the conflict the client asked to be told about; without it the operations are rebased, i.e. the pass starts over on the draft that is there now.
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
    let mut moved = None;
    for _ in 0..REREAD_ATTEMPTS {
        match apply_once(&state, &cfg, &user, id, expected).await? {
            Attempt::Applied { project_id, rev, architecture, version } => {
                audit::log(&state.pool, Some(&user), "ai.apply", "project", &project_id.to_string(), json!({"proposal": id}), &client_ip(&headers, Some(addr), &state.boot)).await;
                return Ok(Json(json!({"rev": rev, "architecture": architecture, "version": version})));
            }
            Attempt::Moved(project_id) => moved = Some(project_id),
        }
    }
    let rev: Option<i64> = match moved {
        Some(p) => sqlx::query_scalar("select draft_rev from projects where id = $1").bind(p).fetch_optional(&state.pool).await?,
        None => None,
    };
    Err(ApiError::conflict("conflict", "the project keeps changing; review the proposal again").with_details(json!({"currentRev": rev})))
}

async fn apply_once(state: &AppState, cfg: &Arc<Loaded>, user: &AuthUser, id: Uuid, expected: Option<i64>) -> ApiResult<Attempt> {
    // 1. read, through the pool, with no transaction open
    let p = sqlx::query("select project_id, change, rationale, status from proposals where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("proposal not found"))?;
    let project_id: Uuid = p.get("project_id");
    let status: String = p.get("status");
    if status != "pending" {
        return Err(proposal_closed(&status));
    }
    let change: Value = p.get("change");
    let rationale: String = p.get("rationale");
    let st = load_draft(&state.pool, project_id).await?;
    if expected.is_some_and(|e| e != st.rev) {
        return Err(draft_moved(st.rev));
    }
    let waivers = engine_waivers(state, project_id).await?;
    let lang = cfg.settings.str("app.defaultLanguage");

    // 2. compute the new architecture and the summaries kept with the versions, still holding nothing
    let base: Architecture = parse_arch(st.draft.clone())?;
    let mut new_arch: Architecture = match change["mode"].as_str() {
        Some("replace") => parse_arch(change["architecture"].clone())?,
        Some("ops") => {
            let ops: Vec<Op> = serde_json::from_value(change["ops"].clone())?;
            match apply_ops(&base, &ops, &cfg.catalog, &limits(cfg)) {
                Ok(o) => o.architecture,
                Err(e) => {
                    // stale: but only on the evidence of the draft that was read; if it has moved since, that is not yet known
                    let marked = sqlx::query(
                        "update proposals set status = 'stale', decided_at = now() where id = $1 and status = 'pending'
                         and exists (select 1 from projects where id = $2 and draft_rev = $3 and deleted_at is null)",
                    )
                    .bind(id)
                    .bind(project_id)
                    .bind(st.rev)
                    .execute(&state.pool)
                    .await?;
                    if marked.rows_affected() == 0 {
                        return Ok(Attempt::Moved(project_id));
                    }
                    return Err(ApiError::conflict("proposal_stale", "the proposal no longer applies to the current architecture").with_details(json!({"issues": e.issues})));
                }
            }
        }
        _ => return Err(ApiError::internal("proposal has an unknown mode")),
    };
    for n in new_arch.nodes.iter_mut() {
        n.proposed = false;
    }
    let before_summary = if st.unsaved { Some(evaluated_summary(cfg, base, waivers.clone(), lang.clone()).await?) } else { None };
    let after_summary = evaluated_summary(cfg, new_arch.clone(), waivers, lang).await?;
    let new_json = serde_json::to_value(&new_arch)?;
    let keep = cfg.settings.num("limits.maxVersionsPerProject") as i32;

    // 3. a short transaction that only writes. Lock order: the proposal, then the project (nobody takes them the other way round)
    let mut tx = state.pool.begin().await?;
    let status: String =
        sqlx::query_scalar("select status from proposals where id = $1 for update").bind(id).fetch_optional(&mut *tx).await?.ok_or_else(|| ApiError::not_found("proposal not found"))?;
    if status != "pending" {
        return Err(proposal_closed(&status)); // somebody decided it while this pass was evaluating
    }
    let (rev, head) = lock_project(&mut tx, project_id).await?;
    if rev != st.rev && expected.is_some() {
        return Err(draft_moved(rev));
    }
    if rev != st.rev || head != st.head {
        return Ok(Attempt::Moved(project_id));
    }
    let mut n = head;
    if let Some(summary) = &before_summary {
        n = append_version(&mut tx, project_id, n, keep, user, VersionEntry { message: "Before AI change", architecture: &st.draft, summary }).await?;
    }
    let message = format!("AI: {}", rationale.chars().take(80).collect::<String>());
    n = append_version(&mut tx, project_id, n, keep, user, VersionEntry { message: &message, architecture: &new_json, summary: &after_summary }).await?;
    let (new_rev,): (i64,) =
        sqlx::query_as("update projects set draft = $2, draft_rev = draft_rev + 1, updated_at = now() where id = $1 returning draft_rev").bind(project_id).bind(&new_json).fetch_one(&mut *tx).await?;
    sqlx::query("update proposals set status = 'applied', decided_at = now() where id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Attempt::Applied { project_id, rev: new_rev, architecture: new_json, version: n })
}

pub async fn reject_proposal(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let r = sqlx::query(
        "update proposals set status = 'rejected', decided_at = now() where id = $1 and status = 'pending'
         and exists (select 1 from projects p where p.id = proposals.project_id and p.deleted_at is null)",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;
    if r.rows_affected() == 0 {
        // not found (or its project is gone), or already decided: two different answers
        let known: Option<(i32,)> =
            sqlx::query_as("select 1 from proposals pr join projects p on p.id = pr.project_id and p.deleted_at is null where pr.id = $1").bind(id).fetch_optional(&state.pool).await?;
        return Err(if known.is_some() { ApiError::conflict("proposal_closed", "the proposal is not pending") } else { ApiError::not_found("proposal not found") });
    }
    Ok(Json(json!({"ok": true})))
}
