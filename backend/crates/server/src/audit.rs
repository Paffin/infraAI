//! Append-only audit log. Every administrative change and security-relevant event is recorded
//! with the actor, target and details (never secrets).

use crate::auth::AuthUser;
use serde_json::Value;
use sqlx::PgPool;

pub async fn log(pool: &PgPool, actor: Option<&AuthUser>, action: &str, target_type: &str, target_id: &str, details: Value, ip: &str) {
    let r = sqlx::query("insert into audit_log (actor_id, actor, action, target_type, target_id, details, ip) values ($1, $2, $3, $4, $5, $6, $7)")
        .bind(actor.map(|a| a.id))
        .bind(actor.map(|a| a.email.clone()).unwrap_or_default())
        .bind(action)
        .bind(target_type)
        .bind(target_id)
        .bind(details)
        .bind(ip)
        .execute(pool)
        .await;
    if let Err(e) = r {
        tracing::error!(error = %e, action, "failed to write audit log");
    }
}
