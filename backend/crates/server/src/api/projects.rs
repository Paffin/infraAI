//! Projects: working copy (draft) with optimistic concurrency, immutable versions, diff,
//! accepted risks (waivers).

use crate::api_support::{lang_of, parse_valid_arch, run_evaluate, summarize};
use crate::audit;
use crate::auth::{client_ip, AuthUser, Role};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use engine::config::Waiver;
use engine::model::Architecture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use std::net::SocketAddr;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    id: Uuid,
    name: String,
    description: String,
    owner: Option<String>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    head_version: i32,
    draft_rev: i64,
    node_count: i64,
    summary: Option<Value>,
    /// Layout of the working copy, just enough to draw a thumbnail (only for the most recently changed projects, `limits.listThumbnails`; `null` = load the project): `zones` (`i` id, `x y w h`, `t` type, `p` parent),
    /// `nodes` (`i` id, `x y`, `k` kind, `z` zone) and `edges` (`[source, target, type]`). Coordinates are `null` until the
    /// project has been laid out in the editor.
    thumb: Option<Value>,
}

/// SQL that boils the draft down to the thumbnail (see `ProjectRow::thumb`) without shipping the whole document.
const THUMB_SQL: &str = "(select jsonb_build_object(
        'zones', coalesce((select jsonb_agg(jsonb_build_object('i', z->'id', 'x', z#>'{ui,x}', 'y', z#>'{ui,y}', 'w', z#>'{ui,w}', 'h', z#>'{ui,h}', 't', z->'type', 'p', z->'parentId'))
                             from jsonb_array_elements(coalesce(p.draft->'zones', '[]'::jsonb)) z), '[]'::jsonb),
        'nodes', coalesce((select jsonb_agg(jsonb_build_object('i', n->'id', 'x', n#>'{ui,x}', 'y', n#>'{ui,y}', 'k', n->'kind', 'z', n->'zoneId'))
                             from jsonb_array_elements(coalesce(p.draft->'nodes', '[]'::jsonb)) n), '[]'::jsonb),
        'edges', coalesce((select jsonb_agg(jsonb_build_array(e->'source', e->'target', e->'type'))
                             from jsonb_array_elements(coalesce(p.draft->'edges', '[]'::jsonb)) e), '[]'::jsonb)))";

pub async fn list(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Value>> {
    // the thumbnail comes with the list only for the most recently changed projects (`limits.listThumbnails`);
    // the web UI loads the rest when their cards scroll into view, so the list stays light for a long project list
    let thumbs = state.cfg().await?.settings.num("limits.listThumbnails") as i64;
    let sql = format!(
        "select p.id, p.name, p.description, u.email as owner, p.updated_at, p.created_at, p.head_version, p.draft_rev,
                jsonb_array_length(coalesce(p.draft->'nodes', '[]'::jsonb)) as node_count,
                (select v.summary from versions v where v.project_id = p.id and v.n = p.head_version) as summary,
                case when row_number() over (order by p.updated_at desc) <= $1 then {THUMB_SQL} end as thumb
         from projects p left join users u on u.id = p.owner_id
         where p.deleted_at is null order by p.updated_at desc limit 500"
    );
    let rows = sqlx::query(&sql).bind(thumbs).fetch_all(&state.pool).await?;
    let items: Vec<ProjectRow> = rows
        .into_iter()
        .map(|r| ProjectRow {
            id: r.get("id"),
            name: r.get("name"),
            description: r.get("description"),
            owner: r.get("owner"),
            updated_at: r.get("updated_at"),
            created_at: r.get("created_at"),
            head_version: r.get("head_version"),
            draft_rev: r.get("draft_rev"),
            node_count: r.get::<Option<i32>, _>("node_count").unwrap_or(0) as i64,
            summary: r.get("summary"),
            thumb: r.get("thumb"),
        })
        .collect();
    Ok(Json(json!({"projects": items})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProject {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub architecture: Option<Value>,
    /// Generate the architecture from a placement pattern (see `/api/placement/preview`).
    #[serde(default)]
    pub placement: Option<engine::placement::PlacementRequest>,
    #[serde(default)]
    pub lang: Option<String>,
}

pub async fn create(State(state): State<AppState>, user: AuthUser, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(b): Json<CreateProject>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(ApiError::bad_request("invalid_name", "project name must be 1–200 characters"));
    }
    let (count,): (i64,) = sqlx::query_as("select count(*) from projects where owner_id = $1 and deleted_at is null").bind(user.id).fetch_one(&state.pool).await?;
    if count >= cfg.settings.num("limits.maxProjectsPerUser") as i64 {
        return Err(ApiError::conflict("project_limit", "project limit reached"));
    }
    let lang = lang_of(&cfg, b.lang.as_deref());
    let from_placement = b.placement.is_some();
    let arch: Architecture = if let Some(t) = &b.template_id {
        cfg.templates.iter().find(|x| &x.id == t).ok_or_else(|| ApiError::not_found("template not found"))?.architecture.clone()
    } else if let Some(mut req) = b.placement {
        req.lang = Some(lang.clone());
        req.max_nodes = Some(crate::api_support::limits(&cfg).max_nodes);
        let ec = cfg.engine_config(&lang);
        tokio::task::spawn_blocking(move || engine::placement::generate(&ec, &ec.placement, &req))
            .await
            .map_err(ApiError::internal)?
            .map_err(|e| ApiError::bad_request("placement_failed", e.message.clone()).with_details(json!({"code": e.code})))?
            .architecture
    } else if let Some(a) = b.architecture {
        parse_valid_arch(&cfg, a)?
    } else {
        Architecture::default()
    };
    let report = run_evaluate(cfg.clone(), arch.clone(), vec![], lang).await?;
    let summary = serde_json::to_value(summarize(&report)).map_err(ApiError::internal)?;
    let mut tx = state.pool.begin().await?;
    let (id,): (Uuid,) = sqlx::query_as("insert into projects (name, description, owner_id, draft, head_version) values ($1, $2, $3, $4, 1) returning id")
        .bind(name)
        .bind(&b.description)
        .bind(user.id)
        .bind(serde_json::to_value(&arch)?)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("insert into versions (project_id, n, message, author_id, author, architecture, summary, engine_version) values ($1, 1, $2, $3, $4, $5, $6, $7)")
        .bind(id)
        .bind(if b.template_id.is_some() {
            "Created from template"
        } else if from_placement {
            "Created from a placement pattern"
        } else {
            "Created"
        })
        .bind(user.id)
        .bind(&user.email)
        .bind(serde_json::to_value(&arch)?)
        .bind(summary)
        .bind(engine::evaluate::ENGINE_VERSION as i32)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit::log(&state.pool, Some(&user), "project.create", "project", &id.to_string(), json!({"name": name, "template": b.template_id}), &client_ip(&headers, Some(addr), state.boot.trust_proxy))
        .await;
    Ok(Json(json!({"id": id})))
}

async fn active_waivers(state: &AppState, id: Uuid) -> ApiResult<Vec<Value>> {
    let rows = sqlx::query(
        "select id, fingerprint, rule_id, reason, created_by, created_at, expires_at from waivers
         where project_id = $1 and revoked_at is null and (expires_at is null or expires_at > now()) order by created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "fingerprint": r.get::<String, _>("fingerprint"),
                "ruleId": r.get::<String, _>("rule_id"),
                "reason": r.get::<String, _>("reason"),
                "createdBy": r.get::<String, _>("created_by"),
                "createdAt": r.get::<DateTime<Utc>, _>("created_at"),
                "expiresAt": r.get::<Option<DateTime<Utc>>, _>("expires_at"),
            })
        })
        .collect())
}

/// Waivers in the form the engine consumes.
pub async fn engine_waivers(state: &AppState, id: Uuid) -> ApiResult<Vec<Waiver>> {
    Ok(active_waivers(state, id)
        .await?
        .into_iter()
        .map(|w| Waiver {
            fingerprint: w["fingerprint"].as_str().unwrap_or("").to_string(),
            reason: w["reason"].as_str().unwrap_or("").to_string(),
            by: w["createdBy"].as_str().map(String::from),
            at: w["createdAt"].as_str().map(String::from),
        })
        .collect())
}

pub async fn get(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let r = sqlx::query(
        "select p.id, p.name, p.description, p.draft, p.draft_rev, p.head_version, p.updated_at, p.created_at, u.email as owner
         from projects p left join users u on u.id = p.owner_id where p.id = $1 and p.deleted_at is null",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("project not found"))?;
    let waivers = active_waivers(&state, id).await?;
    Ok(Json(json!({
        "project": {
            "id": r.get::<Uuid, _>("id"), "name": r.get::<String, _>("name"), "description": r.get::<String, _>("description"),
            "owner": r.get::<Option<String>, _>("owner"), "headVersion": r.get::<i32, _>("head_version"),
            "updatedAt": r.get::<DateTime<Utc>, _>("updated_at"), "createdAt": r.get::<DateTime<Utc>, _>("created_at"),
        },
        "architecture": r.get::<Value, _>("draft"),
        "rev": r.get::<i64, _>("draft_rev"),
        "waivers": waivers,
    })))
}

#[derive(Deserialize)]
pub struct Patch {
    pub name: Option<String>,
    pub description: Option<String>,
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(b): Json<Patch>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    if let Some(n) = &b.name {
        if n.trim().is_empty() || n.chars().count() > 200 {
            return Err(ApiError::bad_request("invalid_name", "project name must be 1–200 characters"));
        }
    }
    let r = sqlx::query("update projects set name = coalesce($2, name), description = coalesce($3, description), updated_at = now() where id = $1 and deleted_at is null")
        .bind(id)
        .bind(b.name.map(|n| n.trim().to_string()))
        .bind(b.description)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("project not found"));
    }
    Ok(Json(json!({"ok": true})))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let owner: Option<(Option<Uuid>,)> = sqlx::query_as("select owner_id from projects where id = $1 and deleted_at is null").bind(id).fetch_optional(&state.pool).await?;
    let Some((owner,)) = owner else { return Err(ApiError::not_found("project not found")) };
    if user.role != Role::Admin && owner != Some(user.id) {
        return Err(ApiError::forbidden("only the owner or an admin can delete a project"));
    }
    // soft delete: recoverable by an administrator until the retention job purges it
    sqlx::query("update projects set deleted_at = now() where id = $1").bind(id).execute(&state.pool).await?;
    audit::log(&state.pool, Some(&user), "project.delete", "project", &id.to_string(), json!({}), &client_ip(&headers, Some(addr), state.boot.trust_proxy)).await;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDraft {
    pub architecture: Value,
    pub expected_rev: i64,
}

/// Save the working copy. Fails with 409 if somebody else saved in the meantime.
pub async fn save_draft(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(b): Json<SaveDraft>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let arch = parse_valid_arch(&cfg, b.architecture)?;
    let row: Option<(i64,)> =
        sqlx::query_as("update projects set draft = $2, draft_rev = draft_rev + 1, updated_at = now() where id = $1 and draft_rev = $3 and deleted_at is null returning draft_rev")
            .bind(id)
            .bind(serde_json::to_value(&arch)?)
            .bind(b.expected_rev)
            .fetch_optional(&state.pool)
            .await?;
    match row {
        Some((rev,)) => Ok(Json(json!({"rev": rev}))),
        None => {
            let cur: Option<(i64,)> = sqlx::query_as("select draft_rev from projects where id = $1 and deleted_at is null").bind(id).fetch_optional(&state.pool).await?;
            match cur {
                None => Err(ApiError::not_found("project not found")),
                Some((rev,)) => Err(ApiError::conflict("conflict", "the project was changed by someone else").with_details(json!({"currentRev": rev}))),
            }
        }
    }
}

pub async fn list_versions(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("select n, message, author, summary, created_at from versions where project_id = $1 order by n desc").bind(id).fetch_all(&state.pool).await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| json!({"n": r.get::<i32, _>("n"), "message": r.get::<String, _>("message"), "author": r.get::<String, _>("author"), "summary": r.get::<Value, _>("summary"), "createdAt": r.get::<DateTime<Utc>, _>("created_at")}))
        .collect();
    Ok(Json(json!({"versions": items})))
}

#[derive(Deserialize)]
pub struct NewVersion {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub lang: Option<String>,
}

pub async fn create_version(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(b): Json<NewVersion>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let row: Option<(Value, i32)> = sqlx::query_as("select draft, head_version from projects where id = $1 and deleted_at is null").bind(id).fetch_optional(&state.pool).await?;
    let Some((draft, _)) = row else { return Err(ApiError::not_found("project not found")) };
    let arch = crate::api_support::parse_arch(draft.clone())?;
    let waivers = engine_waivers(&state, id).await?;
    let report = run_evaluate(cfg.clone(), arch, waivers, lang_of(&cfg, b.lang.as_deref())).await?;
    let summary = serde_json::to_value(summarize(&report))?;
    let mut tx = state.pool.begin().await?;
    // lock the project row so concurrent snapshots get consecutive numbers
    let (head,): (i32,) = sqlx::query_as("select head_version from projects where id = $1 for update").bind(id).fetch_one(&mut *tx).await?;
    let n = head + 1;
    sqlx::query("insert into versions (project_id, n, message, author_id, author, architecture, summary, engine_version) values ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(id)
        .bind(n)
        .bind(b.message.trim())
        .bind(user.id)
        .bind(&user.email)
        .bind(&draft)
        .bind(summary)
        .bind(engine::evaluate::ENGINE_VERSION as i32)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update projects set head_version = $2 where id = $1").bind(id).bind(n).execute(&mut *tx).await?;
    // retention: keep the newest `limits.maxVersionsPerProject`
    let max = cfg.settings.num("limits.maxVersionsPerProject") as i32;
    sqlx::query("delete from versions where project_id = $1 and n <= $2").bind(id).bind(n - max).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"n": n})))
}

pub async fn get_version(State(state): State<AppState>, _user: AuthUser, Path((id, n)): Path<(Uuid, i32)>) -> ApiResult<Json<Value>> {
    let r = sqlx::query("select n, message, author, architecture, summary, created_at from versions where project_id = $1 and n = $2")
        .bind(id)
        .bind(n)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("version not found"))?;
    Ok(Json(json!({
        "n": r.get::<i32, _>("n"), "message": r.get::<String, _>("message"), "author": r.get::<String, _>("author"),
        "architecture": r.get::<Value, _>("architecture"), "summary": r.get::<Value, _>("summary"), "createdAt": r.get::<DateTime<Utc>, _>("created_at"),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Restore {
    pub expected_rev: i64,
}

pub async fn restore_version(State(state): State<AppState>, user: AuthUser, Path((id, n)): Path<(Uuid, i32)>, Json(b): Json<Restore>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let (arch,): (Value,) = sqlx::query_as("select architecture from versions where project_id = $1 and n = $2")
        .bind(id)
        .bind(n)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("version not found"))?;
    let row: Option<(i64,)> =
        sqlx::query_as("update projects set draft = $2, draft_rev = draft_rev + 1, updated_at = now() where id = $1 and draft_rev = $3 and deleted_at is null returning draft_rev")
            .bind(id)
            .bind(arch)
            .bind(b.expected_rev)
            .fetch_optional(&state.pool)
            .await?;
    match row {
        Some((rev,)) => Ok(Json(json!({"rev": rev}))),
        None => Err(ApiError::conflict("conflict", "the project was changed by someone else")),
    }
}

#[derive(Deserialize)]
pub struct DiffQuery {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub ui: bool,
}

async fn arch_of(state: &AppState, id: Uuid, which: &str) -> ApiResult<Architecture> {
    let v: Value = if which == "draft" {
        sqlx::query_as::<_, (Value,)>("select draft from projects where id = $1 and deleted_at is null")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("project not found"))?
            .0
    } else {
        let n: i32 = which.parse().map_err(|_| ApiError::bad_request("invalid_ref", "use 'draft' or a version number"))?;
        sqlx::query_as::<_, (Value,)>("select architecture from versions where project_id = $1 and n = $2")
            .bind(id)
            .bind(n)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("version not found"))?
            .0
    };
    crate::api_support::parse_arch(v)
}

pub async fn diff(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>, Query(q): Query<DiffQuery>) -> ApiResult<Json<Value>> {
    let a = arch_of(&state, id, &q.from).await?;
    let b = arch_of(&state, id, &q.to).await?;
    Ok(Json(serde_json::to_value(engine::ops::diff(&a, &b, q.ui))?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewWaiver {
    pub fingerprint: String,
    pub reason: String,
    #[serde(default)]
    pub expires_in_days: Option<i64>,
    #[serde(default)]
    pub lang: Option<String>,
}

pub async fn create_waiver(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<NewWaiver>,
) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let min = cfg.settings.num("limits.waiverMinReasonChars") as usize;
    if b.reason.trim().chars().count() < min {
        return Err(ApiError::bad_request("reason_too_short", format!("justification must be at least {min} characters")));
    }
    // only real, currently open critical/risk findings can be accepted
    let arch = arch_of(&state, id, "draft").await?;
    let waivers = engine_waivers(&state, id).await?;
    let report = run_evaluate(cfg.clone(), arch, waivers, lang_of(&cfg, b.lang.as_deref())).await?;
    let f = report.findings.iter().find(|f| f.id == b.fingerprint).ok_or_else(|| ApiError::bad_request("unknown_finding", "no such finding in the current architecture"))?;
    if !matches!(f.status, engine::evaluate::Status::Critical | engine::evaluate::Status::Risk) {
        return Err(ApiError::bad_request("not_acceptable", "only critical or risk findings can be accepted"));
    }
    let max_days = cfg.settings.num("limits.waiverMaxDays") as i64;
    let days = b.expires_in_days.unwrap_or(max_days).clamp(1, max_days);
    let rule_id = f.rule_id.clone();
    let wid: Uuid = sqlx::query_scalar(
        "insert into waivers (project_id, fingerprint, rule_id, reason, created_by, expires_at) values ($1, $2, $3, $4, $5, now() + ($6 || ' days')::interval)
         on conflict (project_id, fingerprint) where revoked_at is null do update set reason = excluded.reason, created_by = excluded.created_by, created_at = now(), expires_at = excluded.expires_at
         returning id",
    )
    .bind(id)
    .bind(&b.fingerprint)
    .bind(&rule_id)
    .bind(b.reason.trim())
    .bind(&user.email)
    .bind(days.to_string())
    .fetch_one(&state.pool)
    .await?;
    audit::log(
        &state.pool,
        Some(&user),
        "waiver.create",
        "project",
        &id.to_string(),
        json!({"fingerprint": b.fingerprint, "rule": rule_id, "days": days}),
        &client_ip(&headers, Some(addr), state.boot.trust_proxy),
    )
    .await;
    Ok(Json(json!({"id": wid})))
}

pub async fn revoke_waiver(State(state): State<AppState>, user: AuthUser, Path((id, wid)): Path<(Uuid, Uuid)>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let r = sqlx::query("update waivers set revoked_at = now(), revoked_by = $3 where id = $2 and project_id = $1 and revoked_at is null")
        .bind(id)
        .bind(wid)
        .bind(&user.email)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("waiver not found"));
    }
    audit::log(&state.pool, Some(&user), "waiver.revoke", "project", &id.to_string(), json!({"waiver": wid}), "").await;
    Ok(Json(json!({"ok": true})))
}
