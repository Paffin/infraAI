//! Authentication and authorisation.
//!
//! * Passwords: Argon2id. Sessions: signed stateless tokens in an HttpOnly, SameSite cookie, so
//!   any replica can serve any request; a per-user `session_version` revokes them.
//! * Roles: `viewer` < `editor` < `admin`.
//! * Brute force: a per-IP throttle and a lockout per (account, IP) with a higher ceiling for the account as a whole, both counted in PostgreSQL so that
//!   every replica sees the same count; a sign-in costs the same work whether or not the account exists and answers the same.
//! * CSRF: SameSite=Lax cookie + `Origin` check on state-changing requests.

use crate::audit;
use crate::config::BootConfig;
use crate::crypto::{dummy_hash, sha256_hex};
use crate::error::{ApiError, ApiResult};
use crate::json::Json;
use crate::ratelimit::{self, Charge, Limit};
use crate::state::AppState;
use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method};
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const COOKIE: &str = "ia_session";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Editor,
    Admin,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "viewer" => Some(Role::Viewer),
            "editor" => Some(Role::Editor),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Editor => "editor",
            Role::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub must_change_password: bool,
}

#[derive(Clone)]
pub struct CachedUser {
    pub user: AuthUser,
    pub disabled: bool,
    pub session_version: i32,
}

impl AuthUser {
    pub fn require(&self, min: Role) -> ApiResult<()> {
        if self.role >= min {
            Ok(())
        } else {
            Err(ApiError::forbidden("insufficient role"))
        }
    }
    pub fn require_editor(&self) -> ApiResult<()> {
        self.require(Role::Editor)
    }
    pub fn require_admin(&self) -> ApiResult<()> {
        self.require(Role::Admin)
    }
}

#[derive(Serialize, Deserialize)]
struct SessionPayload {
    u: Uuid,
    e: i64,
    v: i32,
}

pub async fn load_user(state: &AppState, id: Uuid) -> ApiResult<Option<CachedUser>> {
    // how long a replica trusts a user's role and lock state (`auth.userCacheSeconds`; 0 = ask on every request)
    let ttl = Duration::from_secs_f64(state.cfg().await?.settings.num("auth.userCacheSeconds").max(0.0));
    if let Some((u, at)) = state.user_cache.lock().unwrap().get(&id) {
        if at.elapsed() < ttl {
            return Ok(Some(u.clone()));
        }
    }
    let row: Option<(Uuid, String, String, String, bool, bool, i32)> =
        sqlx::query_as("select id, email, name, role, disabled, must_change_password, session_version from users where id = $1").bind(id).fetch_optional(&state.pool).await?;
    let Some((id, email, name, role, disabled, must_change, ver)) = row else { return Ok(None) };
    let cu = CachedUser { user: AuthUser { id, email, name, role: Role::parse(&role).unwrap_or(Role::Viewer), must_change_password: must_change }, disabled, session_version: ver };
    state.user_cache.lock().unwrap().insert(id, (cu.clone(), Instant::now()));
    Ok(Some(cu))
}

pub fn forget_user(state: &AppState, id: Uuid) {
    state.user_cache.lock().unwrap().remove(&id);
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar.get(COOKIE).map(|c| c.value().to_string()).ok_or_else(|| ApiError::unauthorized("not signed in"))?;
        let payload = state.keys.verify(&token).ok_or_else(|| ApiError::unauthorized("invalid session"))?;
        let p: SessionPayload = serde_json::from_slice(&payload).map_err(|_| ApiError::unauthorized("invalid session"))?;
        if p.e < chrono::Utc::now().timestamp() {
            return Err(ApiError::unauthorized("session expired"));
        }
        let cu = load_user(state, p.u).await?.ok_or_else(|| ApiError::unauthorized("unknown user"))?;
        if cu.disabled || cu.session_version != p.v {
            return Err(ApiError::unauthorized("session revoked"));
        }
        // A temporary password only unlocks the endpoints needed to replace it.
        if cu.user.must_change_password {
            let path = parts.uri.path();
            if !["/auth/me", "/auth/password", "/auth/logout"].iter().any(|allowed| path.ends_with(allowed)) {
                return Err(ApiError::forbidden_code("password_change_required", "the password must be changed first"));
            }
        }
        Ok(cu.user)
    }
}

/// The address of the client. Without `TRUST_PROXY` that is the peer of the TCP connection, whatever the request says. Behind proxies it is the entry of
/// `X-Forwarded-For` that the nearest trusted proxy wrote: `TRUST_PROXY_HOPS` entries from the **right**, because every proxy appends the address it saw,
/// so the entries on the left are whatever the client chose to send and the first one is the easiest to forge. All the header lines count (a proxy such as
/// HAProxy adds a second line instead of extending the first). A header that does not have that many entries, or whose entry is not an address, was not
/// written by the proxies that were counted on: the peer it is.
pub fn client_ip(headers: &HeaderMap, addr: Option<SocketAddr>, boot: &BootConfig) -> String {
    let peer = addr.map(|a| a.ip().to_canonical().to_string()).unwrap_or_default();
    if !boot.trust_proxy {
        return peer;
    }
    forwarded_client(headers, boot.trust_proxy_hops).unwrap_or(peer)
}

fn forwarded_client(headers: &HeaderMap, hops: usize) -> Option<String> {
    let mut entries: Vec<&str> = vec![];
    for line in headers.get_all("x-forwarded-for") {
        entries.extend(line.to_str().ok()?.split(',').map(str::trim));
    }
    let entry = entries.get(entries.len().checked_sub(hops.max(1))?)?;
    parse_forwarded_address(entry).map(|ip| ip.to_string())
}

/// `1.2.3.4`, `1.2.3.4:5678`, `2001:db8::1`, `[2001:db8::1]`, `[2001:db8::1]:443` (an IPv4 address written as IPv6 is the IPv4 address).
fn parse_forwarded_address(entry: &str) -> Option<IpAddr> {
    if entry.len() > 64 {
        return None; // longer than any address with a port: not one, and not worth parsing
    }
    let ip =
        entry.parse::<IpAddr>().ok().or_else(|| entry.parse::<SocketAddr>().ok().map(|a| a.ip())).or_else(|| entry.strip_prefix('[')?.strip_suffix(']')?.parse::<Ipv6Addr>().ok().map(IpAddr::V6))?;
    Some(ip.to_canonical())
}

/// Reject cross-origin state-changing requests (CSRF). Requests without an `Origin` header
/// (curl, server-to-server) carry no ambient browser credentials and are allowed.
pub async fn csrf_guard(State(state): State<AppState>, req: Request, next: Next) -> Result<Response, ApiError> {
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        if let Some(origin) = req.headers().get("origin").and_then(|v| v.to_str().ok()) {
            let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
            let origin_host = origin.split("://").nth(1).unwrap_or(origin);
            let public = state.cfg().await.map(|c| c.settings.str("app.publicUrl")).unwrap_or_default();
            let public_host = public.split("://").nth(1).unwrap_or(&public).trim_end_matches('/').to_string();
            let fwd_host = req.headers().get("x-forwarded-host").and_then(|v| v.to_str().ok()).unwrap_or("");
            let ok = origin_host == host || (!public_host.is_empty() && origin_host == public_host) || (state.boot.trust_proxy && !fwd_host.is_empty() && origin_host == fwd_host);
            if !ok {
                return Err(ApiError::forbidden("cross-origin request rejected"));
            }
        }
    }
    Ok(next.run(req).await)
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

fn session_cookie(state: &AppState, token: String, ttl_hours: f64) -> Cookie<'static> {
    Cookie::build((COOKIE, token)).http_only(true).secure(state.boot.cookie_secure).same_site(SameSite::Lax).path("/").max_age(time::Duration::seconds((ttl_hours * 3600.0) as i64)).build()
}

/// The longest e-mail address anything here takes: what `users_create` accepts, and what a sign-in looks at (a longer one is not an account's, and would
/// otherwise be written to the audit log as the target of the attempt).
pub const MAX_EMAIL_BYTES: usize = 200;

/// Whether `email` (already trimmed) is something to look an account up by: not empty, not longer than [`MAX_EMAIL_BYTES`], no control characters. A NUL
/// byte is the one that matters: PostgreSQL cannot store it in text, so it would be counted, then fail in the lookup as a 500 instead of the 400 that the
/// rest of the bad input gets.
pub fn is_acceptable_email(email: &str) -> bool {
    !email.is_empty() && email.len() <= MAX_EMAIL_BYTES && !email.chars().any(char::is_control)
}

fn locked(state: &AppState, retry_after_secs: u64) -> ApiError {
    state.metrics.auth_logins.with_label_values(&["locked"]).inc();
    ApiError::too_many("locked", "account is temporarily locked after repeated failures").with_retry_after(retry_after_secs)
}

/// The key under which the wrong current passwords of one signed-in user are counted.
pub fn pwchange_key(user: Uuid) -> String {
    format!("login:pwchange:{user}")
}

/// The key under which the failures of an account are counted: a digest of the address, whether or not there is such an account (so a sign-in for an
/// address that has none is counted, and locked, the same way, and the lock tells nothing), and short enough not to be the address itself.
pub fn failure_key(email: &str) -> String {
    format!("login:fail:{}", &sha256_hex(email.as_bytes())[..32])
}

pub async fn login(State(state): State<AppState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, jar: CookieJar, Json(body): Json<LoginBody>) -> ApiResult<(CookieJar, Json<Value>)> {
    let ip = client_ip(&headers, Some(addr), &state.boot);
    let email = body.email.trim().to_lowercase();
    // before anything is counted or written: an address that long, or with a control character in it, is nobody's
    if !is_acceptable_email(&email) {
        return Err(ApiError::bad_request("invalid_email", "invalid email"));
    }
    let cfg = state.cfg().await?;
    let s = &cfg.settings;
    let outcome = &state.metrics.auth_logins;

    // 1. Every attempt from an address counts, whoever it is for and however it ends (`auth.loginsPerMinutePerIp`).
    let per_ip = Limit { window_secs: 60.0, max: s.num("auth.loginsPerMinutePerIp") as i32, lock_secs: 0.0 };
    if let Charge::Refused { retry_after_secs } = ratelimit::charge(&state.pool, &format!("login:ip:{ip}"), per_ip).await? {
        outcome.with_label_values(&["throttled"]).inc();
        return Err(ApiError::too_many("rate_limited", "too many sign-in attempts, try again later").with_retry_after(retry_after_secs));
    }

    // 2. The attempt is paid for BEFORE the password is looked at and given back if it was right: that is what makes "at most N guesses per window" true
    //    when twenty come at the same moment (counting the failures afterwards lets all twenty be checked). A failure of one address against one account
    //    locks that pair; the account as a whole has a higher ceiling that only guesses from many addresses reach. An address that is locked is refused
    //    without being charged to the account, or one address could use up the account's ceiling alone.
    let max_failed = s.num("auth.maxFailedAttempts") as i32;
    let lock_secs = s.num("auth.lockoutMinutes") * 60.0;
    let per_account_and_ip = s.str("auth.lockoutScope") != "account";
    let account_key = failure_key(&email);
    let pair_key = per_account_and_ip.then(|| format!("{account_key}:{ip}"));
    let mut tripped = None; // the lock this attempt closed, if it did
    if let Some(key) = &pair_key {
        match ratelimit::charge(&state.pool, key, Limit { window_secs: lock_secs, max: max_failed, lock_secs }).await? {
            Charge::Refused { retry_after_secs } => return Err(locked(&state, retry_after_secs)),
            Charge::Admitted { tripped: true, .. } => tripped = Some("address"),
            Charge::Admitted { .. } => {}
        }
    }
    let ceiling = if pair_key.is_some() { (s.num("auth.maxFailedAttemptsPerAccount") as i32).max(max_failed) } else { max_failed };
    match ratelimit::charge(&state.pool, &account_key, Limit { window_secs: lock_secs, max: ceiling, lock_secs }).await? {
        Charge::Refused { retry_after_secs } => return Err(locked(&state, retry_after_secs)),
        Charge::Admitted { tripped: true, .. } => tripped = Some("account"),
        Charge::Admitted { .. } => {}
    }

    // 3. One password check, with the same cost for every answer: the hash of the account, or a dummy of the same cost when there is no such account; a
    //    disabled account is checked too (and refused whatever the password is).
    type UserRow = (Uuid, String, String, String, bool, bool, i32);
    let row: Option<UserRow> =
        sqlx::query_as("select id, password_hash, name, role, disabled, must_change_password, session_version from users where lower(email) = $1").bind(&email).fetch_optional(&state.pool).await?;
    let hash = row.as_ref().map(|r| r.1.clone()).unwrap_or_else(|| dummy_hash().to_string());
    let password_ok = state.passwords.verify(body.password, hash).await;
    let reason = match &row {
        None => "unknown",
        Some(r) if r.4 => "disabled",
        Some(_) => "password",
    };

    let Some((id, _, name, role, _, must_change, ver)) = row.filter(|_| password_ok && reason == "password") else {
        outcome.with_label_values(&["failed"]).inc();
        audit::log(&state.pool, None, "auth.login_failed", "user", &email, json!({"reason": reason}), &ip).await;
        if let Some(scope) = tripped {
            // the administrator sees it in the list of users (the lock itself is in `rate_limits`; for an address with no account there is nothing to mirror)
            let mirrored = sqlx::query("update users set locked_until = greatest(coalesce(locked_until, now()), now() + make_interval(secs => $2::float8)) where lower(email) = $1")
                .bind(&email)
                .bind(lock_secs)
                .execute(&state.pool)
                .await;
            if let Err(e) = mirrored {
                tracing::warn!(error = %e, "could not show the lock in the list of users");
            }
            audit::log(&state.pool, None, "auth.locked", "user", &email, json!({"scope": scope, "minutes": lock_secs / 60.0}), &ip).await;
        }
        return Err(ApiError::unauthorized("invalid email or password"));
    };

    // 4. Right: the failures of this address are forgotten; the account's ceiling gets this attempt back but is not reset, or every sign-in of the owner
    //    would wipe the guesses somebody else made (with the account as the scope there is only one counter, and it starts again, as it always did).
    match &pair_key {
        Some(key) => {
            ratelimit::clear(&state.pool, key).await?;
            ratelimit::refund(&state.pool, &account_key, ceiling).await?;
        }
        None => ratelimit::clear(&state.pool, &account_key).await?,
    }
    sqlx::query("update users set last_login_at = now() where id = $1").bind(id).execute(&state.pool).await?;
    outcome.with_label_values(&["ok"]).inc();
    let ttl = cfg.settings.num("auth.sessionTtlHours").max(1.0);
    let payload = serde_json::to_vec(&SessionPayload { u: id, e: chrono::Utc::now().timestamp() + (ttl * 3600.0) as i64, v: ver }).map_err(ApiError::internal)?;
    let token = state.keys.sign(&payload);
    let user = AuthUser { id, email: email.clone(), name, role: Role::parse(&role).unwrap_or(Role::Viewer), must_change_password: must_change };
    audit::log(&state.pool, Some(&user), "auth.login", "user", &id.to_string(), json!({}), &ip).await;
    Ok((jar.add(session_cookie(&state, token, ttl)), Json(json!({"user": user}))))
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> (CookieJar, Json<Value>) {
    let _ = &state;
    (jar.remove(Cookie::build((COOKIE, "")).path("/").build()), Json(json!({"ok": true})))
}

pub async fn me(user: AuthUser) -> Json<Value> {
    Json(json!({"user": user}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePassword {
    pub current: String,
    pub new: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(b): Json<ChangePassword>,
) -> ApiResult<(CookieJar, Json<Value>)> {
    let cfg = state.cfg().await?;
    check_password_policy(&cfg.settings, &b.new)?;
    // somebody holding a session (a stolen cookie, a screen left open) must not be able to guess the current password for ever: the wrong ones are counted
    // like the failures of a sign-in and lock this, not the sign-in itself, for as long
    let lock_secs = cfg.settings.num("auth.lockoutMinutes") * 60.0;
    let key = pwchange_key(user.id);
    if let Charge::Refused { retry_after_secs } = ratelimit::charge(&state.pool, &key, Limit { window_secs: lock_secs, max: cfg.settings.num("auth.maxFailedAttempts") as i32, lock_secs }).await? {
        return Err(ApiError::too_many("locked", "too many wrong passwords, try again later").with_retry_after(retry_after_secs));
    }
    let (hash,): (String,) = sqlx::query_as("select password_hash from users where id = $1").bind(user.id).fetch_one(&state.pool).await?;
    if !state.passwords.verify(b.current, hash).await {
        return Err(ApiError::bad_request("wrong_password", "current password is incorrect"));
    }
    ratelimit::clear(&state.pool, &key).await?;
    let new_hash = state.passwords.hash(b.new).await?;
    let (ver,): (i32,) = sqlx::query_as("update users set password_hash = $2, must_change_password = false, session_version = session_version + 1 where id = $1 returning session_version")
        .bind(user.id)
        .bind(new_hash)
        .fetch_one(&state.pool)
        .await?;
    forget_user(&state, user.id);
    let ttl = cfg.settings.num("auth.sessionTtlHours").max(1.0);
    let payload = serde_json::to_vec(&SessionPayload { u: user.id, e: chrono::Utc::now().timestamp() + (ttl * 3600.0) as i64, v: ver }).map_err(ApiError::internal)?;
    let token = state.keys.sign(&payload);
    audit::log(&state.pool, Some(&user), "auth.password_changed", "user", &user.id.to_string(), json!({}), &client_ip(&headers, Some(addr), &state.boot)).await;
    Ok((jar.add(session_cookie(&state, token, ttl)), Json(json!({"ok": true}))))
}

pub fn check_password_policy(settings: &crate::settings::Settings, pw: &str) -> ApiResult<()> {
    let min = settings.num("auth.minPasswordLength") as usize;
    if pw.chars().count() < min {
        return Err(ApiError::bad_request("weak_password", format!("password must be at least {min} characters")));
    }
    let first = pw.chars().next();
    if pw.chars().all(|c| Some(c) == first) {
        return Err(ApiError::bad_request("weak_password", "password is too simple"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn boot(trust: bool, hops: usize) -> BootConfig {
        let vars = [("DATABASE_URL", "postgres://x@localhost/x".to_string()), ("APP_ENV", "test".to_string()), ("TRUST_PROXY", trust.to_string()), ("TRUST_PROXY_HOPS", hops.to_string())];
        BootConfig::from_lookup(&|k| vars.iter().find(|(name, _)| *name == k).map(|(_, v)| v.clone())).unwrap()
    }

    fn peer() -> Option<SocketAddr> {
        Some("10.0.0.7:55555".parse().unwrap())
    }

    fn forwarded(lines: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for l in lines {
            h.append("x-forwarded-for", HeaderValue::from_str(l).unwrap());
        }
        h
    }

    #[test]
    fn client_ip_is_the_peer_unless_a_proxy_is_trusted() {
        // a forged header means nothing to a server that is not behind a proxy
        assert_eq!(client_ip(&forwarded(&["1.1.1.1"]), peer(), &boot(false, 1)), "10.0.0.7");
        assert_eq!(client_ip(&HeaderMap::new(), peer(), &boot(true, 1)), "10.0.0.7", "no header: the peer");
        assert_eq!(client_ip(&HeaderMap::new(), None, &boot(false, 1)), "");
    }

    #[test]
    fn client_ip_counts_entries_from_the_right_never_from_the_left() {
        let h = forwarded(&["1.1.1.1, 2.2.2.2"]);
        assert_eq!(client_ip(&h, peer(), &boot(true, 1)), "2.2.2.2", "one proxy: the address it saw is the last entry");
        assert_eq!(client_ip(&h, peer(), &boot(true, 2)), "1.1.1.1", "two proxies");
        // the client writes whatever it likes in front of what the proxy appended
        assert_eq!(client_ip(&forwarded(&["6.6.6.6, 7.7.7.7, 2.2.2.2"]), peer(), &boot(true, 1)), "2.2.2.2");
        assert_eq!(client_ip(&forwarded(&["6.6.6.6"]), peer(), &boot(true, 1)), "6.6.6.6", "a proxy that replaces the header leaves one entry");
    }

    #[test]
    fn client_ip_joins_the_header_lines() {
        // HAProxy `option forwardfor` adds a line; the client's own line comes first
        let h = forwarded(&["6.6.6.6", "2.2.2.2"]);
        assert_eq!(client_ip(&h, peer(), &boot(true, 1)), "2.2.2.2");
        assert_eq!(client_ip(&h, peer(), &boot(true, 2)), "6.6.6.6");
    }

    #[test]
    fn client_ip_falls_back_to_the_peer_when_the_header_is_not_what_the_proxies_wrote() {
        let b = boot(true, 2);
        assert_eq!(client_ip(&forwarded(&["2.2.2.2"]), peer(), &b), "10.0.0.7", "fewer entries than proxies");
        assert_eq!(client_ip(&forwarded(&["1.1.1.1, not-an-ip"]), peer(), &boot(true, 1)), "10.0.0.7", "not an address");
        assert_eq!(client_ip(&forwarded(&[&format!("1.1.1.1, {}", "9".repeat(5000))]), peer(), &boot(true, 1)), "10.0.0.7", "a 5000-character entry");
        assert_eq!(client_ip(&forwarded(&["1.1.1.1, "]), peer(), &boot(true, 1)), "10.0.0.7", "an empty entry");
        let mut raw = HeaderMap::new();
        raw.insert("x-forwarded-for", HeaderValue::from_bytes(b"1.1.1.1, \xff\xfe").unwrap());
        assert_eq!(client_ip(&raw, peer(), &boot(true, 1)), "10.0.0.7", "not text");
    }

    #[test]
    fn client_ip_reads_ports_and_brackets_and_writes_one_form_per_address() {
        let one = boot(true, 1);
        assert_eq!(client_ip(&forwarded(&["2.2.2.2:443"]), peer(), &one), "2.2.2.2");
        assert_eq!(client_ip(&forwarded(&["[2001:db8::1]:443"]), peer(), &one), "2001:db8::1");
        assert_eq!(client_ip(&forwarded(&["[2001:db8::1]"]), peer(), &one), "2001:db8::1");
        assert_eq!(client_ip(&forwarded(&["2001:DB8:0:0:0:0:0:1"]), peer(), &one), "2001:db8::1", "the same address is the same key however it is written");
        assert_eq!(client_ip(&forwarded(&["::ffff:2.2.2.2"]), peer(), &one), "2.2.2.2", "an IPv4 address written as IPv6");
    }
}
