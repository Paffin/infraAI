//! Release tables of the catalog (`catalog.versions`): the clock the engine is given, the refresh from endoflife.date
//! and the periodic refresh. The engine itself never reads a clock and never touches the network.

use crate::audit;
use crate::auth::AuthUser;
use crate::config_store::{bump_rev, K_VERSIONS};
use crate::state::AppState;
use engine::lifecycle::{format_date, from_endoflife, parse_date, VersionSet};
use futures::stream::{self, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const META_AT: &str = "versions_sync_at";
const META_RESULT: &str = "versions_sync_result";
/// The most one answer of a source may weigh: endoflife.date answers with a few kilobytes.
const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;

/// Today (UTC) as `YYYY-MM-DD`: the clock the engine is handed.
pub fn today_utc() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    format_date(secs.div_euclid(86_400))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    pub id: String,
    pub ok: bool,
    pub cycles: usize,
    pub as_of: Option<String>,
    pub error: Option<String>,
}

impl SyncOutcome {
    fn failed(id: &str, error: impl Into<String>) -> Self {
        SyncOutcome { id: id.to_string(), ok: false, cycles: 0, as_of: None, error: Some(error.into()) }
    }
}

/// May the server fetch `url`? Only addresses under one of the configured bases (same scheme, host and port, path
/// below the base path), and never with credentials in them: an administrator can edit a table's `source`, and the
/// server must not become a way to reach internal services.
pub fn source_allowed(url: &str, bases: &[String]) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    bases
        .iter()
        .filter_map(|b| reqwest::Url::parse(b).ok())
        .any(|b| b.scheme() == u.scheme() && b.host_str() == u.host_str() && b.port_or_known_default() == u.port_or_known_default() && u.path().starts_with(b.path()))
}

async fn fetch(http: &reqwest::Client, url: &str, timeout: Duration) -> Result<Value, String> {
    let res = http.get(url).header("accept", "application/json").timeout(timeout).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status().as_u16()));
    }
    if res.content_length().map(|n| n as usize > MAX_ANSWER_BYTES).unwrap_or(false) {
        return Err("the answer is too large".into());
    }
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_ANSWER_BYTES {
        return Err("the answer is too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("not JSON: {e}"))
}

/// Replace the cycles of one table with what the source reported, and make every replica notice. A bad answer never
/// empties a table.
pub async fn apply_answer(state: &AppState, id: &str, rows: &Value, today: &str, max_age_years: f64) -> SyncOutcome {
    let out = store_answer(state, id, rows, today, max_age_years).await;
    if out.ok && bump_rev(&state.pool).await.is_ok() {
        state.config.invalidate().await;
    }
    out
}

/// The same without telling the replicas: a refresh of many tables does that once, at the end.
async fn store_answer(state: &AppState, id: &str, rows: &Value, today: &str, max_age_years: f64) -> SyncOutcome {
    let Some(day) = parse_date(today) else { return SyncOutcome::failed(id, "no valid date") };
    let cur: Option<Value> = match sqlx::query_scalar("select data from config_items where kind = $1 and id = $2").bind(K_VERSIONS).bind(id).fetch_optional(&state.pool).await {
        Ok(v) => v,
        Err(e) => return SyncOutcome::failed(id, e.to_string()),
    };
    let Some(mut set) = cur.and_then(|d| serde_json::from_value::<VersionSet>(d).ok()) else { return SyncOutcome::failed(id, "unknown table") };
    let cycles = from_endoflife(rows, day, max_age_years);
    if cycles.is_empty() {
        return SyncOutcome::failed(id, "the source reported no release lines");
    }
    set.cycles = cycles;
    set.as_of = today.to_string();
    let data = match serde_json::to_value(&set) {
        Ok(v) => v,
        Err(e) => return SyncOutcome::failed(id, e.to_string()),
    };
    // the fresher table becomes this installation's default: shipped (older) tables do not overwrite it any more
    let h = crate::crypto::sha256_hex(serde_json::to_string(&data).unwrap_or_default().as_bytes());
    let r = sqlx::query("update config_items set data = $3, default_hash = case when origin = 'default' and not modified then $4 else default_hash end, update_available = false, updated_at = now(), updated_by = 'versions-sync' where kind = $1 and id = $2")
        .bind(K_VERSIONS)
        .bind(id)
        .bind(&data)
        .bind(h)
        .execute(&state.pool)
        .await;
    match r {
        Ok(_) => SyncOutcome { id: id.to_string(), ok: true, cycles: set.cycles.len(), as_of: Some(today.to_string()), error: None },
        Err(e) => SyncOutcome::failed(id, e.to_string()),
    }
}

/// Fetch and store the tables `ids` (all tables that have an allowed source when empty).
pub async fn sync(state: &AppState, ids: &[String]) -> Result<Vec<SyncOutcome>, crate::error::ApiError> {
    let cfg = state.cfg().await?;
    let bases = cfg.settings.list("versions.sourceBases");
    let timeout = Duration::from_secs_f64(cfg.settings.num("versions.syncTimeoutSeconds").max(1.0));
    let max_age = cfg.settings.num("versions.maxAgeYears");
    let parallel = cfg.settings.num("versions.syncParallel").max(1.0) as usize;
    let today = today_utc();
    let jobs: Vec<(String, String)> = cfg.versions.iter().filter(|v| ids.is_empty() || ids.contains(&v.id)).map(|v| (v.id.clone(), v.source.clone())).collect();
    let out: Vec<SyncOutcome> = stream::iter(jobs)
        .map(|(id, source)| {
            let http = state.http.clone();
            let bases = bases.clone();
            let today = today.clone();
            async move {
                if !source.starts_with("http") {
                    return None; // a table kept by hand
                }
                if !source_allowed(&source, &bases) {
                    return Some(SyncOutcome::failed(&id, "the source is not under an allowed address (setting versions.sourceBases)"));
                }
                Some(match fetch(&http, &source, timeout).await {
                    Ok(rows) => store_answer(state, &id, &rows, &today, max_age).await,
                    Err(e) => SyncOutcome::failed(&id, e),
                })
            }
        })
        .buffer_unordered(parallel)
        .filter_map(|x| async move { x })
        .collect()
        .await;
    if out.iter().any(|o| o.ok) {
        bump_rev(&state.pool).await?;
        state.config.invalidate().await;
    }
    let mut out = out;
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// A manual refresh from the admin panel: audited, and remembered as the last one.
pub async fn sync_by(state: &AppState, user: &AuthUser, ids: &[String], ip: &str) -> Result<Vec<SyncOutcome>, crate::error::ApiError> {
    let out = sync(state, ids).await?;
    remember(state, &out, "manual").await;
    audit::log(
        &state.pool,
        Some(user),
        "versions.sync",
        K_VERSIONS,
        "",
        json!({"requested": ids.len(), "ok": out.iter().filter(|o| o.ok).count(), "failed": out.iter().filter(|o| !o.ok).map(|o| o.id.clone()).collect::<Vec<_>>()}),
        ip,
    )
    .await;
    Ok(out)
}

async fn remember(state: &AppState, out: &[SyncOutcome], by: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    let result = json!({"at": now, "by": by, "ok": out.iter().filter(|o| o.ok).count(), "failed": out.iter().filter(|o| !o.ok).map(|o| json!({"id": o.id, "error": o.error})).collect::<Vec<_>>()});
    for (k, v) in [(META_AT, now), (META_RESULT, result.to_string())] {
        let _ = sqlx::query("insert into meta (key, value) values ($1, $2) on conflict (key) do update set value = excluded.value").bind(k).bind(v).execute(&state.pool).await;
    }
}

/// When the tables were last refreshed and how it went.
pub async fn last_sync(state: &AppState) -> Value {
    let v: Option<String> = sqlx::query_scalar("select value from meta where key = $1").bind(META_RESULT).fetch_optional(&state.pool).await.ok().flatten();
    v.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
}

/// One replica of many claims the right to refresh: the claim is a conditional write of a timestamp.
async fn claim(state: &AppState, every_days: f64) -> bool {
    let now = chrono::Utc::now().to_rfc3339();
    let r: Result<Option<String>, _> = sqlx::query_scalar(
        "insert into meta (key, value) values ($1, $2) on conflict (key) do update set value = excluded.value
         where meta.value::timestamptz <= now() - make_interval(secs => $3) returning value",
    )
    .bind(META_AT)
    .bind(&now)
    .bind(every_days * 86_400.0)
    .fetch_optional(&state.pool)
    .await;
    matches!(r, Ok(Some(_)))
}

/// Refresh the tables every `versions.autoSyncDays` days (0 = never). Failures are remembered and shown in the admin
/// panel; they never stop the server.
pub async fn auto_sync(state: AppState) {
    loop {
        let poll = match state.cfg().await {
            Ok(cfg) => {
                let days = cfg.settings.num("versions.autoSyncDays");
                if days > 0.0 && claim(&state, days).await {
                    match sync(&state, &[]).await {
                        Ok(out) => {
                            let failed = out.iter().filter(|o| !o.ok).count();
                            tracing::info!(tables = out.len(), failed, "release tables refreshed");
                            remember(&state, &out, "auto").await;
                            audit::log(&state.pool, None, "versions.sync", K_VERSIONS, "", json!({"auto": true, "ok": out.len() - failed, "failed": failed}), "").await;
                        }
                        Err(e) => tracing::warn!(error = %e.message, "release tables could not be refreshed"),
                    }
                }
                cfg.settings.num("versions.autoSyncCheckMinutes").max(1.0)
            }
            Err(_) => 1.0,
        };
        tokio::time::sleep(Duration::from_secs_f64(poll * 60.0)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_addresses_under_a_configured_base_are_fetched() {
        let bases = vec!["https://endoflife.date/api/".to_string()];
        assert!(source_allowed("https://endoflife.date/api/postgresql.json", &bases));
        assert!(!source_allowed("https://endoflife.date.evil.example/api/x.json", &bases), "a longer host is another host");
        assert!(!source_allowed("https://endoflife.date/other/x.json", &bases));
        assert!(!source_allowed("http://endoflife.date/api/x.json", &bases), "another scheme");
        assert!(!source_allowed("https://user:pass@endoflife.date/api/x.json", &bases), "credentials in the address");
        assert!(!source_allowed("https://endoflife.date:8443/api/x.json", &bases), "another port");
        assert!(!source_allowed("file:///etc/passwd", &bases));
        assert!(!source_allowed("not a url", &bases));
        assert!(!source_allowed("https://endoflife.date/api/x.json", &[]), "no base, no fetch");
    }

    #[test]
    fn today_is_a_valid_date() {
        let t = today_utc();
        assert!(parse_date(&t).is_some(), "{t}");
        assert_eq!(format_date(parse_date(&t).unwrap()), t);
    }
}
