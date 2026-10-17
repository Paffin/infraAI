//! Release tables of the catalog (`catalog.versions`): the clock the engine is given, the refresh from endoflife.date
//! and the periodic refresh. The engine itself never reads a clock and never touches the network.

use crate::audit;
use crate::auth::AuthUser;
use crate::config_store::{bump_rev, K_VERSIONS};
use crate::outbound;
use crate::state::AppState;
use engine::lifecycle::{format_date, from_endoflife, parse_date, VersionSet};
use futures::stream::{self, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const META_AT: &str = "versions_sync_at";
const META_RESULT: &str = "versions_sync_result";
/// How many redirects one fetch follows before it gives up. A bound of the protocol, not a tunable: a source that sends a client round more than a few
/// addresses is not answering. Every hop is checked against the allowed bases, so this only bounds the loop.
const MAX_REDIRECTS: usize = 5;

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
/// below the base path - below it as a directory: `/api` admits `/api` and `/api/x.json`, not `/api-evil/x.json`), and never with credentials in
/// them: an administrator can edit a table's `source`, and the server must not become a way to reach internal services.
pub fn source_allowed(url: &str, bases: &[String]) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let below = |base: &str, path: &str| if base.ends_with('/') { path.starts_with(base) } else { path == base || path.strip_prefix(base).is_some_and(|rest| rest.starts_with('/')) };
    bases
        .iter()
        .filter_map(|b| reqwest::Url::parse(b).ok())
        .any(|b| b.scheme() == u.scheme() && b.host_str() == u.host_str() && b.port_or_known_default() == u.port_or_known_default() && below(b.path(), u.path()))
}

/// Fetch one source and parse it as JSON. `http` must be a client that does not follow redirects by itself ([`outbound::sync_client`]): this function does,
/// and it holds **every** address - the first and each `Location` - to `bases` before a request goes to it, because a host that is allowed may answer
/// `302` to one that is not (an internal service, a metadata endpoint). The whole fetch, redirects included, has `timeout`; the answer weighs at most
/// `max_bytes` (counted as it arrives, so a chunked answer without a length is cut off too).
pub async fn fetch(http: &reqwest::Client, url: &str, timeout: Duration, bases: &[String], max_bytes: usize) -> Result<Value, String> {
    let started = Instant::now();
    let mut current = reqwest::Url::parse(url).map_err(|_| "the source is not an address".to_string())?;
    for _ in 0..=MAX_REDIRECTS {
        if !source_allowed(current.as_str(), bases) {
            return Err("the source is not under an allowed address (setting versions.sourceBases)".into());
        }
        let left = timeout.checked_sub(started.elapsed()).filter(|d| !d.is_zero()).ok_or_else(|| "timed out".to_string())?;
        let res = http.get(current.clone()).header("accept", "application/json").timeout(left).send().await.map_err(|e| e.to_string())?;
        let status = res.status();
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            let location = res.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).ok_or_else(|| format!("HTTP {} without a Location", status.as_u16()))?;
            let next = current.join(location).map_err(|_| format!("HTTP {} to something that is not an address", status.as_u16()))?;
            if !source_allowed(next.as_str(), bases) {
                let at = format!("{}{}", next.host_str().unwrap_or("?"), next.port().map(|p| format!(":{p}")).unwrap_or_default());
                return Err(format!("the source redirected to {at}, which is outside the allowed addresses (setting versions.sourceBases)"));
            }
            current = next; // the answer of the redirect is dropped unread
            continue;
        }
        if !status.is_success() {
            return Err(format!("HTTP {}", status.as_u16()));
        }
        let bytes = outbound::read_capped(res, max_bytes).await?;
        return serde_json::from_slice(&bytes).map_err(|e| format!("not JSON: {e}"));
    }
    Err(format!("more than {MAX_REDIRECTS} redirects"))
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
    let max_bytes = outbound::answer_cap(&cfg.settings);
    let parallel = cfg.settings.num("versions.syncParallel").max(1.0) as usize;
    let today = today_utc();
    let jobs: Vec<(String, String)> = cfg.versions.iter().filter(|v| ids.is_empty() || ids.contains(&v.id)).map(|v| (v.id.clone(), v.source.clone())).collect();
    let out: Vec<SyncOutcome> = stream::iter(jobs)
        .map(|(id, source)| {
            let http = state.fetch_http.clone();
            let bases = bases.clone();
            let today = today.clone();
            async move {
                if !source.starts_with("http") {
                    return None; // a table kept by hand
                }
                if !source_allowed(&source, &bases) {
                    return Some(SyncOutcome::failed(&id, "the source is not under an allowed address (setting versions.sourceBases)"));
                }
                Some(match fetch(&http, &source, timeout, &bases, max_bytes).await {
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
    fn a_base_is_a_directory_not_a_prefix_of_names() {
        let no_slash = vec!["https://mirror.internal/eol/api".to_string()];
        assert!(source_allowed("https://mirror.internal/eol/api/postgresql.json", &no_slash));
        assert!(source_allowed("https://mirror.internal/eol/api", &no_slash));
        assert!(!source_allowed("https://mirror.internal/eol/api-evil/x.json", &no_slash), "a base that was written without its slash does not admit a sibling that starts like it");
        assert!(!source_allowed("https://mirror.internal/eol/apix.json", &no_slash));
        let slash = vec!["https://mirror.internal/eol/api/".to_string()];
        assert!(!source_allowed("https://mirror.internal/eol/api-evil/x.json", &slash));
        assert!(!source_allowed("https://mirror.internal/eol/api/../admin/x.json", &slash), "dot segments are resolved before the comparison");
        assert!(!source_allowed("https://mirror.internal/eol/api/%2e%2e/admin/x.json", &slash), "and so are their encoded forms");
    }

    #[test]
    fn today_is_a_valid_date() {
        let t = today_utc();
        assert!(parse_date(&t).is_some(), "{t}");
        assert_eq!(format_date(parse_date(&t).unwrap()), t);
    }

    // ---- the fetch against sockets that misbehave on purpose (outbound::hostile): no real source can be asked to redirect where it should not

    use crate::outbound::hostile::{answer, json, listen, redirect, Reply};
    use crate::outbound::TOO_LARGE;

    const SECOND: Duration = Duration::from_secs(5);
    const MEGABYTE: usize = 1 << 20;

    fn client() -> reqwest::Client {
        crate::outbound::sync_client().unwrap()
    }

    /// An allowed host answers `302` to an address that is not in `versions.sourceBases` (an internal service, a metadata endpoint). Before the
    /// redirect was held to the allowlist the client followed it: the internal listener got a request, and the answer was read as a table.
    #[tokio::test]
    async fn a_redirect_out_of_the_allowed_bases_is_refused() {
        let internal = listen(|_| json("[]")).await;
        let to = internal.url("/latest/meta-data");
        let allowed = listen(move |_| redirect(302, &to)).await;
        let bases = vec![allowed.url("/api/")];

        let err = fetch(&client(), &allowed.url("/api/postgresql.json"), SECOND, &bases, MEGABYTE).await.unwrap_err();
        assert!(err.contains("versions.sourceBases") && err.contains(&format!("127.0.0.1:{}", internal.port)), "{err}");
        assert_eq!(allowed.hits(), 1, "the allowed host was asked once");
        assert_eq!(internal.hits(), 0, "and the address it pointed to was not asked at all");

        // the control: the listeners do lead there. A client that follows redirects, as the one this replaces did, reaches the internal one
        let followed = reqwest::Client::new().get(allowed.url("/api/postgresql.json")).send().await.unwrap();
        assert!(followed.status().is_success(), "{}", followed.status());
        assert_eq!(internal.hits(), 1, "without the check the redirect is followed (one hit); with it, none");
    }

    #[tokio::test]
    async fn a_redirect_inside_the_bases_is_followed() {
        let host = listen(|path| match path {
            "/api/old.json" => redirect(301, "/api/new.json"),
            "/api/new.json" => json(r#"[{"cycle":"1"}]"#),
            _ => answer("404 Not Found", &[], ""),
        })
        .await;
        let bases = vec![host.url("/api/")];
        let rows = fetch(&client(), &host.url("/api/old.json"), SECOND, &bases, MEGABYTE).await.unwrap();
        assert_eq!(rows, json!([{"cycle": "1"}]));
        assert_eq!(host.hits(), 2, "the redirect and the table");
    }

    #[tokio::test]
    async fn a_relative_location_is_resolved_against_the_current_url() {
        let host = listen(|path| match path {
            "/api/v1/old.json" => redirect(302, "../v2/new.json"),
            "/api/v2/new.json" => json("[1]"),
            "/api/v1/climb.json" => redirect(302, "../../admin/secret.json"),
            "/api/v1/other-host.json" => redirect(302, "//127.0.0.1:1/api/v2/new.json"),
            "/api/v1/other-scheme.json" => redirect(302, "https://127.0.0.1/api/v2/new.json"),
            _ => answer("404 Not Found", &[], ""),
        })
        .await;
        let bases = vec![host.url("/api/")];
        assert_eq!(fetch(&client(), &host.url("/api/v1/old.json"), SECOND, &bases, MEGABYTE).await.unwrap(), json!([1]));
        let hits = host.hits();
        for (path, why) in [("/api/v1/climb.json", "climbs out of the base"), ("/api/v1/other-host.json", "another port"), ("/api/v1/other-scheme.json", "another scheme")] {
            let err = fetch(&client(), &host.url(path), SECOND, &bases, MEGABYTE).await.unwrap_err();
            assert!(err.contains("versions.sourceBases"), "{why}: {err}");
        }
        assert_eq!(host.hits(), hits + 3, "each refused redirect cost the one request that carried it");
    }

    #[tokio::test]
    async fn the_first_address_is_held_to_the_bases_too() {
        let host = listen(|_| json("[]")).await;
        let err = fetch(&client(), &host.url("/other/x.json"), SECOND, &[host.url("/api/")], MEGABYTE).await.unwrap_err();
        assert!(err.contains("versions.sourceBases"), "{err}");
        assert_eq!(host.hits(), 0, "not even asked");
    }

    #[tokio::test]
    async fn redirects_that_never_end_stop_at_the_bound() {
        let host = listen(|_| redirect(302, "/api/again.json")).await;
        let err = fetch(&client(), &host.url("/api/again.json"), SECOND, &[host.url("/api/")], MEGABYTE).await.unwrap_err();
        assert!(err.contains("redirects"), "{err}");
        assert_eq!(host.hits(), MAX_REDIRECTS + 1, "the first request and one per redirect followed, and no more");
        let none = listen(|_| answer("302 Found", &[], "")).await;
        let err = fetch(&client(), &none.url("/api/x.json"), SECOND, &[none.url("/api/")], MEGABYTE).await.unwrap_err();
        assert!(err.contains("without a Location"), "{err}");
    }

    #[tokio::test]
    async fn an_oversized_chunked_answer_is_cut_off_at_the_cap() {
        let endless = listen(|_| Reply::Endless).await;
        let started = Instant::now();
        let err = fetch(&client(), &endless.url("/api/x.json"), Duration::from_secs(30), &[endless.url("/api/")], 1024).await.unwrap_err();
        assert_eq!(err, TOO_LARGE);
        assert!(started.elapsed() < Duration::from_secs(2), "stopped by the cap, not by the 30 s timeout: {:?}", started.elapsed());
        // within the cap the same shape of answer is a table
        let finite = listen(|_| Reply::Chunked(vec![b"[1,".to_vec(), b"2]".to_vec()])).await;
        assert_eq!(fetch(&client(), &finite.url("/api/x.json"), SECOND, &[finite.url("/api/")], 1024).await.unwrap(), json!([1, 2]));
    }

    #[tokio::test]
    async fn a_failed_status_and_a_page_that_is_not_json_are_errors_not_tables() {
        let host = listen(|path| if path.ends_with("404.json") { answer("404 Not Found", &[], "{}") } else { json("<html>") }).await;
        let bases = [host.url("/api/")];
        assert_eq!(fetch(&client(), &host.url("/api/404.json"), SECOND, &bases, MEGABYTE).await.unwrap_err(), "HTTP 404");
        assert!(fetch(&client(), &host.url("/api/page.json"), SECOND, &bases, MEGABYTE).await.unwrap_err().starts_with("not JSON"));
    }
}
