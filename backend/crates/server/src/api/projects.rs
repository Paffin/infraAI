//! Projects: working copy (draft) with optimistic concurrency, immutable versions, diff,
//! accepted risks (waivers).

use crate::api_support::{blocking, ensure_valid, evaluated_summary_held, lang_of, limits, parse_arch, parse_valid_arch, run_evaluate_held, summarize, text, TextField};
use crate::audit;
use crate::auth::{client_ip, AuthUser, Role};
use crate::error::{ApiError, ApiResult};
use crate::heavy::Held;
use crate::json::{Json, Path, Query};
use crate::state::AppState;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, SecondsFormat, Utc};
use engine::config::Waiver;
use engine::model::Architecture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool, Row};
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

#[derive(Deserialize)]
pub struct ListQuery {
    /// How many projects at most (1 to `limits.listPageMax`, which is also the default).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Where the page starts: the `next` of the previous one. Without it, the first page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// A position in the list, handed out as `next` and read back as `cursor`: the last project of a page, by the two things the list is ordered by. Opaque to
/// the client (base64 of `updated_at|id`); microseconds are kept, because the row's own time is what the next page compares with.
fn encode_cursor(at: DateTime<Utc>, id: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!("{}|{id}", at.to_rfc3339_opts(SecondsFormat::Micros, true)))
}

fn decode_cursor(cursor: &str) -> Option<(DateTime<Utc>, Uuid)> {
    let raw = String::from_utf8(URL_SAFE_NO_PAD.decode(cursor).ok()?).ok()?;
    let (at, id) = raw.split_once('|')?;
    Some((DateTime::parse_from_rfc3339(at).ok()?.with_timezone(&Utc), Uuid::parse_str(id).ok()?))
}

/// The projects, newest change first (ties by id, so that a page boundary never splits or repeats a row), a page at a time: `{projects, next}`; `next` is
/// `null` on the last page. Without parameters it answers what it always did - the first `limits.listPageMax` (500) - and `next` tells whether there are more.
/// The description is cut to `limits.listDescriptionChars`: a card shows a couple of lines, and one project with a description of megabytes made the list of
/// everybody's projects megabytes (`GET /projects/{id}` has all of it). The thumbnails (`limits.listThumbnails`) come with the first page only: they are
/// for the most recently changed projects, and a later page is, by definition, not those.
pub async fn list(State(state): State<AppState>, _user: AuthUser, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    let cursor = match q.cursor.as_deref() {
        Some(c) => Some(decode_cursor(c).ok_or_else(|| ApiError::bad_request("invalid_cursor", "the cursor is not one that this list gave out; start again without it"))?),
        None => None,
    };
    let page_max = (cfg.settings.num("limits.listPageMax") as i64).max(1);
    let limit = q.limit.unwrap_or(page_max).clamp(1, page_max);
    let thumbs = if cursor.is_some() { 0 } else { cfg.settings.num("limits.listThumbnails") as i64 };
    let description_chars = cfg.settings.num("limits.listDescriptionChars") as i32;
    // the window function is only there when a thumbnail is: it makes the database number every row before the page is cut
    let thumb = if thumbs > 0 { format!("case when row_number() over (order by p.updated_at desc, p.id desc) <= $5 then {THUMB_SQL} end") } else { "null::jsonb".to_string() };
    let sql = format!(
        "select p.id, p.name, left(p.description, $1) as description, u.email as owner, p.updated_at, p.created_at, p.head_version, p.draft_rev,
                jsonb_array_length(coalesce(p.draft->'nodes', '[]'::jsonb)) as node_count,
                (select v.summary from versions v where v.project_id = p.id and v.n = p.head_version) as summary,
                {thumb} as thumb
         from projects p left join users u on u.id = p.owner_id
         where p.deleted_at is null and ($2::timestamptz is null or (p.updated_at <= $2 and (p.updated_at, p.id) < ($2, $3)))
         order by p.updated_at desc, p.id desc limit $4"
    );
    let mut query = sqlx::query(&sql).bind(description_chars).bind(cursor.map(|c| c.0)).bind(cursor.map(|c| c.1)).bind(limit + 1);
    if thumbs > 0 {
        query = query.bind(thumbs);
    }
    let mut rows = query.fetch_all(&state.pool).await?;
    let more = rows.len() as i64 > limit;
    rows.truncate(limit as usize);
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
    let next = if more { items.last().map(|p| encode_cursor(p.updated_at, p.id)) } else { None };
    Ok(Json(json!({"projects": items, "next": next})))
}

/// `invalid_name` when a project name is empty or longer than `limits.maxNameChars` (the name is already trimmed). The code is the one the editor knows;
/// `details {field, max, actual}` says the bound.
fn check_project_name(cfg: &crate::config_store::Loaded, name: &str) -> ApiResult<()> {
    let max = cfg.settings.num("limits.maxNameChars") as usize;
    let actual = name.chars().count();
    if actual == 0 || actual > max {
        return Err(ApiError::bad_request("invalid_name", format!("project name must be 1–{max} characters")).with_details(json!({"field": "name", "max": max, "actual": actual})));
    }
    Ok(())
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

pub async fn create(State(state): State<AppState>, user: AuthUser, held: Held, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(b): Json<CreateProject>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let name = b.name.trim();
    check_project_name(&cfg, name)?;
    let description = text(&cfg, TextField::LongText, "description", &b.description)?;
    let (count,): (i64,) = sqlx::query_as("select count(*) from projects where owner_id = $1 and deleted_at is null").bind(user.id).fetch_one(&state.pool).await?;
    if count >= cfg.settings.num("limits.maxProjectsPerUser") as i64 {
        return Err(ApiError::conflict("project_limit", "project limit reached"));
    }
    let lang = lang_of(&cfg, b.lang.as_deref());
    let from_placement = b.placement.is_some();
    let arch: Architecture = if let Some(t) = &b.template_id {
        let a = cfg.templates.iter().find(|x| &x.id == t).ok_or_else(|| ApiError::not_found("template not found"))?.architecture.clone();
        ensure_valid(&cfg, &a, &limits(&cfg))?;
        a
    } else if let Some(mut req) = b.placement {
        req.lang = Some(lang.clone());
        req.max_nodes = Some(limits(&cfg).max_nodes);
        let ec = cfg.engine_config(&lang);
        let a = blocking(&held, move || engine::placement::generate(&ec, &ec.placement, &req))
            .await?
            .map_err(|e| ApiError::bad_request("placement_failed", e.message.clone()).with_details(json!({"code": e.code})))?
            .architecture;
        // what the generator makes is held to the same rules as what a person writes: a design stored out of range would fail the next save
        ensure_valid(&cfg, &a, &limits(&cfg))?;
        a
    } else if let Some(a) = b.architecture {
        parse_valid_arch(&cfg, a)?
    } else {
        Architecture::default()
    };
    let report = run_evaluate_held(cfg.clone(), arch.clone(), vec![], lang, &held).await?;
    let summary = serde_json::to_value(summarize(&report)).map_err(ApiError::internal)?;
    let mut tx = state.pool.begin().await?;
    let (id,): (Uuid,) = sqlx::query_as("insert into projects (name, description, owner_id, draft, head_version) values ($1, $2, $3, $4, 1) returning id")
        .bind(name)
        .bind(description)
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
    audit::log(&state.pool, Some(&user), "project.create", "project", &id.to_string(), json!({"name": name, "template": b.template_id}), &client_ip(&headers, Some(addr), &state.boot)).await;
    Ok(Json(json!({"id": id})))
}

/// The project's accepted risks that are in force. A project that is gone (deleted, or never made) is `404`: whatever is asked about it
/// - an evaluation with its waivers, an export - there is nothing to answer, and an empty list would pass for "no accepted risks".
async fn active_waivers(state: &AppState, id: Uuid) -> ApiResult<Vec<Value>> {
    let rows = sqlx::query(
        "select w.id, w.fingerprint, w.rule_id, w.reason, w.created_by, w.created_at, w.expires_at
         from projects p left join waivers w on w.project_id = p.id and w.revoked_at is null and (w.expires_at is null or w.expires_at > now())
         where p.id = $1 and p.deleted_at is null order by w.created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    if rows.is_empty() {
        return Err(ApiError::not_found("project not found"));
    }
    Ok(rows
        .into_iter()
        .filter(|r| r.get::<Option<Uuid>, _>("id").is_some())
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

/// How many times a write that evaluated the draft (seconds of CPU, no lock held) starts over because the draft moved meanwhile before
/// it gives up with `409`. A protocol bound, not a tunable: a draft that moves under three evaluations in a row is being edited by someone who should be asked.
pub const REREAD_ATTEMPTS: usize = 3;

/// Where a project's working copy and history stand, read without a lock.
pub struct DraftState {
    pub draft: Value,
    pub rev: i64,
    pub head: i32,
    /// The working copy differs from the newest version (or there is none): overwriting it loses something unless it is saved first.
    pub unsaved: bool,
}

/// Read the working copy of a live project through the pool. `404` when the project is deleted or never existed.
pub async fn load_draft(pool: &PgPool, id: Uuid) -> ApiResult<DraftState> {
    let r = sqlx::query(
        "select p.draft, p.draft_rev, p.head_version, (v.architecture is distinct from p.draft) as unsaved
         from projects p left join versions v on v.project_id = p.id and v.n = p.head_version
         where p.id = $1 and p.deleted_at is null",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("project not found"))?;
    Ok(DraftState { draft: r.get("draft"), rev: r.get("draft_rev"), head: r.get("head_version"), unsaved: r.get("unsaved") })
}

/// Take the project's row lock for the rest of the transaction and read where it stands now: `(draft_rev, head_version)`.
/// Whoever writes the draft or the history locks first, so that concurrent writers queue instead of numbering versions twice.
/// `404` when the project is gone.
pub async fn lock_project(tx: &mut PgConnection, id: Uuid) -> ApiResult<(i64, i32)> {
    sqlx::query_as("select draft_rev, head_version from projects where id = $1 and deleted_at is null for update")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("project not found"))
}

/// A version to append to a project's history.
pub struct VersionEntry<'a> {
    pub message: &'a str,
    pub architecture: &'a Value,
    pub summary: &'a Value,
}

/// Append a version after `head` inside the caller's transaction (which holds the project's row lock, see [`lock_project`]): the next number,
/// `head_version` moved to it and the history trimmed to the newest `keep` versions (`limits.maxVersionsPerProject`, at least 10, so the head
/// is never trimmed). Every way of writing a version goes through here, so none of them can forget the retention. Returns the number.
pub async fn append_version(tx: &mut PgConnection, project_id: Uuid, head: i32, keep: i32, author: &AuthUser, v: VersionEntry<'_>) -> Result<i32, sqlx::Error> {
    let n = head + 1;
    sqlx::query("insert into versions (project_id, n, message, author_id, author, architecture, summary, engine_version) values ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(project_id)
        .bind(n)
        .bind(v.message)
        .bind(author.id)
        .bind(&author.email)
        .bind(v.architecture)
        .bind(v.summary)
        .bind(engine::evaluate::ENGINE_VERSION as i32)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update projects set head_version = $2 where id = $1").bind(project_id).bind(n).execute(&mut *tx).await?;
    sqlx::query("delete from versions where project_id = $1 and n <= $2").bind(project_id).bind(n - keep).execute(&mut *tx).await?;
    Ok(n)
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
    let cfg = state.cfg().await?;
    if let Some(n) = &b.name {
        check_project_name(&cfg, n.trim())?;
    }
    if let Some(d) = &b.description {
        text(&cfg, TextField::LongText, "description", d)?;
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
    audit::log(&state.pool, Some(&user), "project.delete", "project", &id.to_string(), json!({}), &client_ip(&headers, Some(addr), &state.boot)).await;
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
    // the project is part of the query: no row at all means it is gone, one row without a version would be a project without history
    let rows = sqlx::query(
        "select v.n, v.message, v.author, v.summary, v.created_at from projects p left join versions v on v.project_id = p.id
         where p.id = $1 and p.deleted_at is null order by v.n desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    if rows.is_empty() {
        return Err(ApiError::not_found("project not found"));
    }
    let items: Vec<Value> = rows
        .into_iter()
        .filter(|r| r.get::<Option<i32>, _>("n").is_some())
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

pub async fn create_version(State(state): State<AppState>, user: AuthUser, held: Held, Path(id): Path<Uuid>, Json(b): Json<NewVersion>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let message = text(&cfg, TextField::LongText, "message", b.message.trim())?;
    // everything that takes a connection or CPU happens before the transaction: it only writes
    let st = load_draft(&state.pool, id).await?;
    let waivers = engine_waivers(&state, id).await?;
    let summary = evaluated_summary_held(&cfg, parse_arch(st.draft.clone())?, waivers, lang_of(&cfg, b.lang.as_deref()), &held).await?;
    let keep = cfg.settings.num("limits.maxVersionsPerProject") as i32;
    let mut tx = state.pool.begin().await?;
    // lock the project row so concurrent snapshots get consecutive numbers
    let (_, head) = lock_project(&mut tx, id).await?;
    let n = append_version(&mut tx, id, head, keep, &user, VersionEntry { message, architecture: &st.draft, summary: &summary }).await?;
    tx.commit().await?;
    Ok(Json(json!({"n": n})))
}

pub async fn get_version(State(state): State<AppState>, _user: AuthUser, Path((id, n)): Path<(Uuid, i32)>) -> ApiResult<Json<Value>> {
    let r = sqlx::query(
        "select v.n, v.message, v.author, v.architecture, v.summary, v.created_at from projects p left join versions v on v.project_id = p.id and v.n = $2
         where p.id = $1 and p.deleted_at is null",
    )
    .bind(id)
    .bind(n)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("project not found"))?;
    if r.get::<Option<i32>, _>("n").is_none() {
        return Err(ApiError::not_found("version not found"));
    }
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

/// Replace the working copy with a version. What is being replaced is saved first, as a version of its own ("Before restore of vN"), unless
/// it already is the newest one: restoring never destroys an edit that was not in a version. `{rev, snapshot}`: `snapshot` is that version's
/// number, or `null` when there was nothing to save.
pub async fn restore_version(
    State(state): State<AppState>,
    user: AuthUser,
    held: Held,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((id, n)): Path<(Uuid, i32)>,
    Json(b): Json<Restore>,
) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let keep = cfg.settings.num("limits.maxVersionsPerProject") as i32;
    let conflict = |rev: i64| ApiError::conflict("conflict", "the project was changed by someone else").with_details(json!({"currentRev": rev}));
    for _ in 0..REREAD_ATTEMPTS {
        // read and evaluate with no transaction open (the evaluation is CPU, and the accepted risks come from the pool)
        let st = load_draft(&state.pool, id).await?;
        let (target,): (Value,) = sqlx::query_as("select architecture from versions where project_id = $1 and n = $2")
            .bind(id)
            .bind(n)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("version not found"))?;
        if st.rev != b.expected_rev {
            return Err(conflict(st.rev));
        }
        let snapshot_summary = if st.unsaved {
            let waivers = engine_waivers(&state, id).await?;
            Some(evaluated_summary_held(&cfg, parse_arch(st.draft.clone())?, waivers, cfg.settings.str("app.defaultLanguage"), &held).await?)
        } else {
            None
        };

        // then a short transaction that only writes: it takes the lock and checks that the draft is still the one that was read
        let mut tx = state.pool.begin().await?;
        let (rev, head) = lock_project(&mut tx, id).await?;
        if rev != b.expected_rev {
            return Err(conflict(rev));
        }
        if head != st.head {
            continue; // a version was cut meanwhile: whether the draft is saved has to be looked at again
        }
        let snapshot = match &snapshot_summary {
            Some(summary) => {
                let message = format!("Before restore of v{n}");
                Some(append_version(&mut tx, id, head, keep, &user, VersionEntry { message: &message, architecture: &st.draft, summary }).await?)
            }
            None => None,
        };
        let (new_rev,): (i64,) =
            sqlx::query_as("update projects set draft = $2, draft_rev = draft_rev + 1, updated_at = now() where id = $1 returning draft_rev").bind(id).bind(&target).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        audit::log(&state.pool, Some(&user), "project.restore", "project", &id.to_string(), json!({"version": n, "snapshot": snapshot}), &client_ip(&headers, Some(addr), &state.boot)).await;
        return Ok(Json(json!({"rev": new_rev, "snapshot": snapshot})));
    }
    Err(ApiError::conflict("conflict", "the project keeps changing; try again"))
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
        let r = sqlx::query("select v.architecture from projects p left join versions v on v.project_id = p.id and v.n = $2 where p.id = $1 and p.deleted_at is null")
            .bind(id)
            .bind(n)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("project not found"))?;
        r.get::<Option<Value>, _>("architecture").ok_or_else(|| ApiError::not_found("version not found"))?
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
    held: Held,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<NewWaiver>,
) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let cfg = state.cfg().await?;
    let min = cfg.settings.num("limits.waiverMinReasonChars") as usize;
    let reason = text(&cfg, TextField::LongText, "reason", b.reason.trim())?;
    if reason.chars().count() < min {
        return Err(ApiError::bad_request("reason_too_short", format!("justification must be at least {min} characters")));
    }
    // a fingerprint that no finding can have is refused before the engine is started to find that out
    text(&cfg, TextField::Id, "fingerprint", &b.fingerprint)?;
    // only real, currently open critical/risk findings can be accepted
    let arch = arch_of(&state, id, "draft").await?;
    let waivers = engine_waivers(&state, id).await?;
    let report = run_evaluate_held(cfg.clone(), arch, waivers, lang_of(&cfg, b.lang.as_deref()), &held).await?;
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
    .bind(reason)
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
        &client_ip(&headers, Some(addr), &state.boot),
    )
    .await;
    Ok(Json(json!({"id": wid})))
}

pub async fn revoke_waiver(State(state): State<AppState>, user: AuthUser, Path((id, wid)): Path<(Uuid, Uuid)>) -> ApiResult<Json<Value>> {
    user.require_editor()?;
    let r = sqlx::query(
        "update waivers w set revoked_at = now(), revoked_by = $3 where w.id = $2 and w.project_id = $1 and w.revoked_at is null
         and exists (select 1 from projects p where p.id = w.project_id and p.deleted_at is null)",
    )
    .bind(id)
    .bind(wid)
    .bind(&user.email)
    .execute(&state.pool)
    .await?;
    if r.rows_affected() == 0 {
        // say which of the two is missing; the project is the more useful answer when it is the one that is gone
        let alive: Option<(i32,)> = sqlx::query_as("select 1 from projects where id = $1 and deleted_at is null").bind(id).fetch_optional(&state.pool).await?;
        return Err(ApiError::not_found(if alive.is_some() { "waiver not found" } else { "project not found" }));
    }
    audit::log(&state.pool, Some(&user), "waiver.revoke", "project", &id.to_string(), json!({"waiver": wid}), "").await;
    Ok(Json(json!({"ok": true})))
}
