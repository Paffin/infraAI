//! Limits that are counted in PostgreSQL, so that every replica sees the same count (ADR-0004: replicas keep nothing).
//!
//! A limit is a key with a count and a window that starts at the first hit. [`charge`] is one statement: it admits the attempt and counts it, or refuses
//! it, atomically, whatever the number of replicas and concurrent requests; so "at most `max` per window" holds for the number of attempts that are
//! *let through*, not just for the number that are counted afterwards. A limit with a lock (`lock_secs > 0`) locks the key from the hit that reaches
//! `max`, for `lock_secs`; a locked key refuses everything without a new version of its row being made (PostgreSQL still takes the row lock to look), so a
//! flood of refused attempts costs a lock, not an update with its dead tuple and its index entries.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// How many attempts, in what time, and for how long a key is locked once they are spent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit {
    /// The window starts at the first attempt; when it is over the count starts again.
    pub window_secs: f64,
    /// Attempts let through per window.
    pub max: i32,
    /// How long the key refuses everything after the attempt that reaches `max`; 0 = no lock, the key just refuses until the window is over.
    pub lock_secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Charge {
    /// Let through; `count` attempts in the window now, `tripped` when this one used the last (and the key is locked from now on).
    Admitted { count: i32, tripped: bool },
    /// Over the limit or locked. The count was not touched. Try again in `retry_after_secs`.
    Refused { retry_after_secs: u64 },
}

/// The window is over (it started `window` ago) or the lock has run out: the count starts again.
const FRESH: &str = "(r.window_start <= now() - make_interval(secs => $2::float8) or coalesce(r.locked_until <= now(), false))";

/// Count one attempt on `key` if the limit lets it through.
pub async fn charge(pool: &PgPool, key: &str, limit: Limit) -> Result<Charge, sqlx::Error> {
    let after = format!("(case when {FRESH} then 1 else r.count + 1 end)");
    let sql = format!(
        "insert into rate_limits as r (key, window_start, count, locked_until)
         values ($1, now(), 1, case when $3::int4 <= 1 and $4::float8 > 0 then now() + make_interval(secs => $4::float8) end)
         on conflict (key) do update set
           count = {after},
           window_start = case when {FRESH} then now() else r.window_start end,
           locked_until = case when {after} >= $3::int4 and $4::float8 > 0 then now() + make_interval(secs => $4::float8)
                               when {FRESH} then null
                               else r.locked_until end
         where not coalesce(r.locked_until > now(), false) and ({FRESH} or r.count < $3::int4)
         returning count, locked_until"
    );
    let row: Option<(i32, Option<DateTime<Utc>>)> = sqlx::query_as(&sql).bind(key).bind(limit.window_secs).bind(limit.max).bind(limit.lock_secs).fetch_optional(pool).await?;
    if let Some((count, locked_until)) = row {
        return Ok(Charge::Admitted { count, tripped: limit.lock_secs > 0.0 && locked_until.is_some() && count >= limit.max });
    }
    // refused: how long until it is not (a read)
    let wait: Option<i64> =
        sqlx::query_scalar("select greatest(1, ceil(extract(epoch from (coalesce(locked_until, window_start + make_interval(secs => $2::float8)) - now()))))::bigint from rate_limits where key = $1")
            .bind(key)
            .bind(limit.window_secs)
            .fetch_optional(pool)
            .await?;
    Ok(Charge::Refused { retry_after_secs: wait.unwrap_or(1).max(1) as u64 })
}

/// Give back one attempt on `key` (a success is not a failure; an attempt that was charged and then not made).
pub async fn refund(pool: &PgPool, key: &str) -> Result<(), sqlx::Error> {
    sqlx::query("update rate_limits set count = greatest(count - 1, 0) where key = $1").bind(key).execute(pool).await?;
    Ok(())
}

/// Forget `key`: the count and the lock.
pub async fn clear(pool: &PgPool, key: &str) -> Result<(), sqlx::Error> {
    sqlx::query("delete from rate_limits where key = $1").bind(key).execute(pool).await?;
    Ok(())
}

/// Forget every key that starts with `prefix` (all the addresses an account was counted from). A prefix, not a pattern: `_` and `%` mean themselves.
pub async fn clear_prefix(pool: &PgPool, prefix: &str) -> Result<(), sqlx::Error> {
    sqlx::query("delete from rate_limits where starts_with(key, $1)").bind(prefix).execute(pool).await?;
    Ok(())
}

/// Delete the rows whose window is over (it started more than `older_than_secs` ago) and that are not locked. Returns how many.
pub async fn sweep(pool: &PgPool, older_than_secs: f64) -> Result<u64, sqlx::Error> {
    let r = sqlx::query("delete from rate_limits where window_start < now() - make_interval(secs => $1::float8) and (locked_until is null or locked_until < now())")
        .bind(older_than_secs)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}
