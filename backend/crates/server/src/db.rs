//! Connection pool, migrations and start-up serialisation between replicas.

use crate::config::BootConfig;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::{Duration, Instant};

/// Connect with exponential backoff: the database may come up after the application.
pub async fn connect(cfg: &BootConfig) -> anyhow::Result<PgPool> {
    let deadline = Instant::now() + Duration::from_secs(cfg.db_connect_timeout_secs);
    let mut delay = Duration::from_millis(500);
    loop {
        let attempt = PgPoolOptions::new()
            .max_connections(cfg.db_max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(|conn, _| {
                Box::pin(async move {
                    sqlx::query("set statement_timeout = '30s'").execute(&mut *conn).await?;
                    sqlx::query("set idle_in_transaction_session_timeout = '60s'").execute(&mut *conn).await?;
                    Ok(())
                })
            })
            .connect(&cfg.database_url)
            .await;
        match attempt {
            Ok(p) => return Ok(p),
            Err(e) if Instant::now() < deadline => {
                tracing::warn!(error = %e, retry_in_ms = delay.as_millis() as u64, "database not reachable yet");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(8));
            }
            Err(e) => return Err(anyhow::anyhow!("cannot connect to the database: {e}")),
        }
    }
}

/// The encoding of the database the pool is connected to (`UTF8`, `SQL_ASCII`, ...).
pub async fn encoding(pool: &PgPool) -> sqlx::Result<String> {
    sqlx::query_scalar("select pg_encoding_to_char(encoding) from pg_database where datname = current_database()").fetch_one(pool).await
}

/// The server keeps and cuts text by characters, and the SQL that cuts (`left(description, n)` in the list of projects) counts characters only in a UTF8
/// database: in `SQL_ASCII` (what `initdb` makes under the `C` locale) it counts bytes, and a cut in the middle of a Cyrillic letter is an invalid sequence that
/// PostgreSQL refuses to send - `GET /api/projects` answered 500. So a database of another encoding is named at start, with what to do. Returns the encoding
/// when it is not UTF8.
pub async fn warn_unless_utf8(pool: &PgPool) -> Option<String> {
    match encoding(pool).await {
        Ok(enc) if enc.eq_ignore_ascii_case("UTF8") => None,
        Ok(enc) => {
            tracing::warn!(encoding = %enc, "the database is not UTF8: text is cut by bytes, and the list of projects fails once a description is cut inside a letter. Create the database with ENCODING 'UTF8' (docs/DEPLOYMENT.md)");
            Some(enc)
        }
        Err(e) => {
            tracing::warn!(error = %e, "the encoding of the database could not be read");
            None
        }
    }
}

pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    // sqlx takes a Postgres advisory lock while migrating: concurrent replicas are safe
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

const STARTUP_LOCK: i64 = 0x1A_FA_A4_C1;

/// Run `f` while holding a cluster-wide advisory lock, so seeding and bootstrap happen once
/// even when many replicas start simultaneously.
pub async fn with_startup_lock<F, Fut, T>(pool: &PgPool, f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let mut conn = pool.acquire().await?;
    sqlx::query("select pg_advisory_lock($1)").bind(STARTUP_LOCK).execute(&mut *conn).await?;
    let r = f().await;
    let _ = sqlx::query("select pg_advisory_unlock($1)").bind(STARTUP_LOCK).execute(&mut *conn).await;
    r
}
