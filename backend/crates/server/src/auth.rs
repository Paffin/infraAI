//! Authentication and authorisation.
//!
//! * Passwords: Argon2id. Sessions: signed stateless tokens in an HttpOnly, SameSite cookie, so
//!   any replica can serve any request; a per-user `session_version` revokes them.
//! * Roles: `viewer` < `editor` < `admin`.
//! * Brute force: per-account lockout (settings) + per-IP throttle on the login endpoint.
//! * CSRF: SameSite=Lax cookie + `Origin` check on state-changing requests.

use crate::audit;
use crate::crypto::{hash_password, verify_password};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method};
use axum::middleware::Next;
use axum::response::Response;
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
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

pub fn client_ip(headers: &HeaderMap, addr: Option<SocketAddr>, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = v.split(',').next() {
                let ip = first.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }
    addr.map(|a| a.ip().to_string()).unwrap_or_default()
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

/// At most `limit` sign-in attempts per IP and minute (`auth.loginsPerMinutePerIp`).
fn throttle_ip(state: &AppState, ip: &str, limit: u32) -> ApiResult<()> {
    let mut m = state.login_attempts.lock().unwrap();
    let now = Instant::now();
    m.retain(|_, (_, at)| now.duration_since(*at) < Duration::from_secs(60));
    let e = m.entry(ip.to_string()).or_insert((0, now));
    e.0 += 1;
    if e.0 > limit {
        return Err(ApiError::too_many("rate_limited", "too many sign-in attempts, try again later"));
    }
    Ok(())
}

pub async fn login(State(state): State<AppState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, jar: CookieJar, Json(body): Json<LoginBody>) -> ApiResult<(CookieJar, Json<Value>)> {
    let ip = client_ip(&headers, Some(addr), state.boot.trust_proxy);
    let cfg = state.cfg().await?;
    throttle_ip(&state, &ip, cfg.settings.num("auth.loginsPerMinutePerIp") as u32)?;
    let email = body.email.trim().to_lowercase();
    let row: Option<(Uuid, String, String, String, bool, bool, i32, i32, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("select id, password_hash, name, role, disabled, must_change_password, session_version, failed_attempts, locked_until from users where lower(email) = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await?;

    let Some((id, hash, name, role, disabled, must_change, ver, failed, locked)) = row else {
        // equalise timing for unknown accounts
        let _ = verify_password(&body.password, "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$ZHVtbXlkdW1teWR1bW15ZHVtbXlkdW1teWR1bW15ZHU");
        audit::log(&state.pool, None, "auth.login_failed", "user", &email, json!({"reason": "unknown"}), &ip).await;
        return Err(ApiError::unauthorized("invalid email or password"));
    };
    if let Some(until) = locked {
        if until > chrono::Utc::now() {
            return Err(ApiError::too_many("locked", "account is temporarily locked after repeated failures"));
        }
    }
    if disabled || !verify_password(&body.password, &hash) {
        let max = cfg.settings.num("auth.maxFailedAttempts") as i32;
        let lock_min = cfg.settings.num("auth.lockoutMinutes") as i64;
        let new_failed = failed + 1;
        if new_failed >= max {
            sqlx::query("update users set failed_attempts = 0, locked_until = now() + ($2 || ' minutes')::interval where id = $1").bind(id).bind(lock_min.to_string()).execute(&state.pool).await?;
        } else {
            sqlx::query("update users set failed_attempts = $2 where id = $1").bind(id).bind(new_failed).execute(&state.pool).await?;
        }
        audit::log(&state.pool, None, "auth.login_failed", "user", &email, json!({"reason": if disabled { "disabled" } else { "password" }}), &ip).await;
        return Err(ApiError::unauthorized("invalid email or password"));
    }
    sqlx::query("update users set failed_attempts = 0, locked_until = null, last_login_at = now() where id = $1").bind(id).execute(&state.pool).await?;
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
    let (hash,): (String,) = sqlx::query_as("select password_hash from users where id = $1").bind(user.id).fetch_one(&state.pool).await?;
    if !verify_password(&b.current, &hash) {
        return Err(ApiError::bad_request("wrong_password", "current password is incorrect"));
    }
    let new_hash = hash_password(&b.new)?;
    let (ver,): (i32,) = sqlx::query_as("update users set password_hash = $2, must_change_password = false, session_version = session_version + 1 where id = $1 returning session_version")
        .bind(user.id)
        .bind(new_hash)
        .fetch_one(&state.pool)
        .await?;
    forget_user(&state, user.id);
    let ttl = cfg.settings.num("auth.sessionTtlHours").max(1.0);
    let payload = serde_json::to_vec(&SessionPayload { u: user.id, e: chrono::Utc::now().timestamp() + (ttl * 3600.0) as i64, v: ver }).map_err(ApiError::internal)?;
    let token = state.keys.sign(&payload);
    audit::log(&state.pool, Some(&user), "auth.password_changed", "user", &user.id.to_string(), json!({}), &client_ip(&headers, Some(addr), state.boot.trust_proxy)).await;
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
