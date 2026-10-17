//! One-time start-up work, serialised across replicas by an advisory lock: migrations, default
//! configuration, the first administrator and the demo project.

use crate::api_support::{run_evaluate, summarize};
use crate::config::BootConfig;
use crate::config_store::{seed_defaults, ConfigCache};
use crate::crypto::{dummy_hash, hash_password, random_password, Keys, PasswordGate};
use crate::db;
use crate::jobs::JobRunner;
use crate::metrics::Metrics;
use crate::state::{AppState, CONFIG_CACHE_TTL};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

async fn ensure_admin(pool: &PgPool, boot: &BootConfig) -> anyhow::Result<()> {
    let n: i64 = sqlx::query_scalar("select count(*) from users").fetch_one(pool).await?;
    if n > 0 {
        return Ok(());
    }
    let email = boot.admin_email.clone().unwrap_or_else(|| crate::config::DEFAULT_ADMIN_EMAIL.to_string()).to_lowercase();
    let (password, generated) = match &boot.admin_password {
        Some(p) => (p.clone(), false),
        None => (random_password(), true),
    };
    if !generated && password.chars().count() < 12 {
        tracing::warn!("ADMIN_PASSWORD is shorter than 12 characters: change it after the first sign-in");
    }
    sqlx::query("insert into users (email, name, role, password_hash, must_change_password) values ($1, 'Administrator', 'admin', $2, $3)")
        .bind(&email)
        .bind(hash_password(&password)?)
        .bind(generated)
        .execute(pool)
        .await?;
    if generated {
        // shown exactly once; the operator is expected to change it on first sign-in
        tracing::warn!("====================================================================");
        tracing::warn!("First start: administrator account created");
        tracing::warn!("  email:    {}", email);
        tracing::warn!("  password: {}   (you must change it after signing in)", password);
        tracing::warn!("Set ADMIN_EMAIL / ADMIN_PASSWORD to choose the credentials yourself.");
        tracing::warn!("====================================================================");
    } else {
        tracing::info!(%email, "administrator account created from ADMIN_EMAIL/ADMIN_PASSWORD");
    }
    Ok(())
}

async fn seed_demo(state: &AppState) -> anyhow::Result<()> {
    let cfg = state.cfg().await.map_err(|e| anyhow::anyhow!("{}", e.message))?;
    if !state.boot.seed_demo_project.unwrap_or_else(|| cfg.settings.boolean("app.seedDemoProject")) {
        return Ok(());
    }
    let any: i64 = sqlx::query_scalar("select count(*) from projects").fetch_one(&state.pool).await?;
    if any > 0 {
        return Ok(());
    }
    let owner: Option<Uuid> = sqlx::query_scalar("select id from users where role = 'admin' order by created_at limit 1").fetch_optional(&state.pool).await?;
    let demo = engine::defaults::demo_project();
    let lang = cfg.settings.str("app.defaultLanguage");
    let mut arch = demo.architecture.clone();
    arch.meta.description = Some(demo.description.get(&lang));
    let report = run_evaluate(cfg.clone(), arch.clone(), vec![], lang).await.map_err(|e| anyhow::anyhow!("{}", e.message))?;
    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar("insert into projects (name, description, owner_id, draft, head_version) values ($1, $2, $3, $4, 1) returning id")
        .bind(&demo.name)
        .bind(demo.description.get(&cfg.settings.str("app.defaultLanguage")))
        .bind(owner)
        .bind(serde_json::to_value(&arch)?)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("insert into versions (project_id, n, message, author, architecture, summary, engine_version) values ($1, 1, 'Demo project', 'system', $2, $3, $4)")
        .bind(id)
        .bind(serde_json::to_value(&arch)?)
        .bind(serde_json::to_value(summarize(&report))?)
        .bind(engine::evaluate::ENGINE_VERSION as i32)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!(project = %id, "demo project created");
    Ok(())
}

/// Build the application state: connect, migrate, seed, bootstrap.
pub async fn init_state(boot: BootConfig) -> anyhow::Result<AppState> {
    let pool = db::connect(&boot).await?;
    db::migrate(&pool).await?;
    let keys = Arc::new(Keys::derive(&boot.app_secret));
    let metrics = Arc::new(Metrics::new(&boot.instance_id));
    let http = reqwest::Client::builder().user_agent(concat!("infra-architect/", env!("CARGO_PKG_VERSION"))).connect_timeout(Duration::from_secs(10)).build()?;
    let ttl = std::env::var("CONFIG_CACHE_TTL_MS").ok().and_then(|v| v.parse::<u64>().ok()).map(Duration::from_millis).unwrap_or(CONFIG_CACHE_TTL);
    let config = Arc::new(ConfigCache::new(ttl));

    db::with_startup_lock(&pool, || async {
        let changed = seed_defaults(&pool).await?;
        if changed {
            tracing::info!("default configuration seeded/updated");
        }
        ensure_admin(&pool, &boot).await?;
        Ok(())
    })
    .await?;

    let loaded = config.get(&pool, &keys).await?;
    for (k, id, e) in &loaded.errors {
        tracing::error!(kind = %k, id = %id, error = %e, "invalid configuration item (skipped)");
    }
    let max_jobs = loaded.settings.num("jobs.maxConcurrent") as usize;
    // how many passwords are hashed at the same moment (`auth.hashConcurrency`, 0 = the processors there are); read once, like jobs.maxConcurrent
    let hash_slots = match loaded.settings.num("auth.hashConcurrency") as usize {
        0 => std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        n => n,
    };
    dummy_hash(); // made now, not by the first sign-in of an account that does not exist
    let passwords = Arc::new(PasswordGate::new(hash_slots, metrics.auth_password_checks.clone()));
    let state = AppState {
        pool,
        boot: Arc::new(boot),
        keys,
        config,
        http,
        fetch_http: crate::outbound::sync_client()?,
        metrics,
        jobs: Arc::new(JobRunner::new(max_jobs)),
        shutting_down: Arc::new(AtomicBool::new(false)),
        started_at: Instant::now(),
        user_cache: Arc::new(Mutex::new(HashMap::new())),
        passwords,
        models_cache: Arc::new(tokio::sync::Mutex::new(None)),
    };
    let st = state.clone();
    db::with_startup_lock(&state.pool, || async move { seed_demo(&st).await }).await?;
    Ok(state)
}
