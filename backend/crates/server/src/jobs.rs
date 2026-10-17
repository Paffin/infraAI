//! AI job runner. Jobs are rows in PostgreSQL, so any replica can serve the status endpoint and a
//! lost replica never leaves a job "running" forever: a heartbeat marks it, a sweeper fails stale
//! ones. Concurrency per instance is bounded by a semaphore (`jobs.maxConcurrent`).

use crate::llm::pipeline;
use crate::state::AppState;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;

pub struct JobRunner {
    permits: Arc<Semaphore>,
}

impl JobRunner {
    pub fn new(max_concurrent: usize) -> Self {
        JobRunner { permits: Arc::new(Semaphore::new(max_concurrent.max(1))) }
    }

    pub fn spawn(&self, state: AppState, job_id: Uuid) {
        let permits = self.permits.clone();
        tokio::spawn(async move {
            let permit = match permits.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
            run_job(state, job_id).await;
            drop(permit);
        });
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: Uuid,
    pub project_id: Uuid,
    pub user_id: Option<Uuid>,
    pub kind: String,
    pub input: Value,
}

/// Records progress steps and answers "was I cancelled?".
#[derive(Clone)]
pub struct Progress {
    pub pool: PgPool,
    pub job_id: Uuid,
}

impl Progress {
    pub async fn step(&self, code: &str, detail: Value) {
        let step = json!({"code": code, "detail": detail, "at": chrono::Utc::now()});
        let _ = sqlx::query("update ai_jobs set steps = steps || $2::jsonb, heartbeat_at = now() where id = $1").bind(self.job_id).bind(json!([step])).execute(&self.pool).await;
    }

    pub async fn cancelled(&self) -> bool {
        sqlx::query_scalar::<_, bool>("select cancel_requested from ai_jobs where id = $1").bind(self.job_id).fetch_one(&self.pool).await.unwrap_or(false)
    }
}

async fn run_job(state: AppState, job_id: Uuid) {
    let row: Option<(Uuid, Option<Uuid>, Option<Uuid>, String, Value, String)> = sqlx::query_as(
        "update ai_jobs set status = 'running', started_at = now(), heartbeat_at = now(), instance = $2 where id = $1 and status = 'queued' returning id, project_id, user_id, kind, input, status",
    )
    .bind(job_id)
    .bind(&state.boot.instance_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);
    let Some((id, project_id, user_id, kind, input, _)) = row else { return };
    let Some(project_id) = project_id else { return };
    state.metrics.jobs_running.inc();
    let job = Job { id, project_id, user_id, kind: kind.clone(), input };
    let progress = Progress { pool: state.pool.clone(), job_id: id };

    // heartbeat + cancellation watcher
    let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let hb_pool = state.pool.clone();
    let poll = Duration::from_secs_f64(state.cfg().await.map(|c| c.settings.num("jobs.pollSeconds")).unwrap_or(2.0).max(0.5));
    let watcher = tokio::spawn(async move {
        let mut tick = tokio::time::interval(poll);
        let mut n = 0u32;
        loop {
            tick.tick().await;
            n += 1;
            if n % 3 == 0 {
                let _ = sqlx::query("update ai_jobs set heartbeat_at = now() where id = $1").bind(id).execute(&hb_pool).await;
            }
            let c: bool = sqlx::query_scalar("select cancel_requested from ai_jobs where id = $1").bind(id).fetch_one(&hb_pool).await.unwrap_or(false);
            if c {
                let _ = cancel_tx.send(true);
                break;
            }
        }
    });

    let result = tokio::select! {
        r = pipeline::run(&state, &job, &progress) => Some(r),
        _ = cancel_rx.changed() => None,
    };
    watcher.abort();

    let status;
    match result {
        None => {
            status = "cancelled";
            let _ = sqlx::query("update ai_jobs set status = 'cancelled', finished_at = now() where id = $1").bind(id).execute(&state.pool).await;
        }
        Some(Ok(out)) => {
            status = "succeeded";
            let _ = sqlx::query("update ai_jobs set status = 'succeeded', result = $2, finished_at = now() where id = $1").bind(id).bind(&out.result).execute(&state.pool).await;
            let _ = sqlx::query("insert into chat_messages (project_id, role, content, proposal_id, job_id) values ($1, 'assistant', $2, $3, $4)")
                .bind(project_id)
                .bind(&out.message)
                .bind(out.proposal_id)
                .bind(id)
                .execute(&state.pool)
                .await;
        }
        Some(Err(e)) => {
            status = "failed";
            tracing::warn!(job = %id, code = %e.code, error = %e.message, "AI job failed");
            let _ =
                sqlx::query("update ai_jobs set status = 'failed', error = $2, error_code = $3, finished_at = now() where id = $1").bind(id).bind(&e.message).bind(&e.code).execute(&state.pool).await;
            let _ = sqlx::query("insert into chat_messages (project_id, role, content, job_id) values ($1, 'system', $2, $3)")
                .bind(project_id)
                .bind(format!("{}|{}", e.code, e.message))
                .bind(id)
                .execute(&state.pool)
                .await;
        }
    }
    state.metrics.jobs_running.dec();
    state.metrics.jobs_finished.with_label_values(&[&kind, status]).inc();
}

/// Fail jobs whose owner instance vanished (no heartbeat), and purge old rows.
pub async fn sweeper(state: AppState) {
    loop {
        if state.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let stale = state.cfg().await.map(|c| c.settings.num("jobs.staleSeconds")).unwrap_or(90.0);
        let r = sqlx::query(
            "update ai_jobs set status = 'failed', error = 'The instance that was running this job stopped; please retry.', error_code = 'interrupted', finished_at = now()
             where status in ('queued', 'running') and heartbeat_at < now() - ($1 || ' seconds')::interval",
        )
        .bind((stale as i64).to_string())
        .execute(&state.pool)
        .await;
        if let Ok(r) = r {
            if r.rows_affected() > 0 {
                tracing::warn!(count = r.rows_affected(), "marked stale AI jobs as failed");
            }
        }
        // sweeps once at start (a crashed instance's jobs are failed at once), then every `jobs.sweepSeconds` – read again each round
        let every = state.cfg().await.map(|c| c.settings.num("jobs.sweepSeconds")).unwrap_or(30.0).max(1.0);
        tokio::time::sleep(Duration::from_secs_f64(every)).await;
    }
}

/// Retention: audit log, LLM call log, finished jobs, purged projects.
pub async fn retention(state: AppState) {
    loop {
        if state.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let Ok(cfg) = state.cfg().await else {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        };
        let s = &cfg.settings;
        let q = |sql: &'static str, days: f64| {
            let pool = state.pool.clone();
            async move {
                let _ = sqlx::query(sql).bind((days as i64).to_string()).execute(&pool).await;
            }
        };
        q("delete from audit_log where at < now() - ($1 || ' days')::interval", s.num("retention.auditDays")).await;
        q("delete from llm_calls where created_at < now() - ($1 || ' days')::interval", s.num("retention.llmCallsDays")).await;
        q("delete from ai_jobs where finished_at is not null and finished_at < now() - ($1 || ' days')::interval", s.num("retention.jobsDays")).await;
        q("delete from projects where deleted_at is not null and deleted_at < now() - ($1 || ' days')::interval", s.num("retention.deletedProjectsDays")).await;
        // the counters of the sign-in limits whose window is over (a lock keeps its row until it runs out); `auth.lockoutMinutes` is the longest window there is
        let _ = crate::ratelimit::sweep(&state.pool, (s.num("auth.lockoutMinutes") + 1.0) * 60.0).await;
        // cleans once at start, then every `retention.sweepMinutes`
        tokio::time::sleep(Duration::from_secs_f64(s.num("retention.sweepMinutes").max(1.0) * 60.0)).await;
    }
}
