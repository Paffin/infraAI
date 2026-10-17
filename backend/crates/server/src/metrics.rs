//! Prometheus metrics (served on `/metrics`).

use prometheus::{Counter, Encoder, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, Opts, Registry, TextEncoder};

pub struct Metrics {
    pub registry: Registry,
    pub http_requests: IntCounterVec,
    pub http_duration: HistogramVec,
    pub llm_calls: IntCounterVec,
    pub llm_cost_usd: Counter,
    pub jobs_running: IntGauge,
    pub jobs_finished: IntCounterVec,
    pub evaluations: IntCounterVec,
    /// Sign-ins by outcome: `ok`, `failed`, `locked`, `throttled`. **Only** these four: the endpoint can be open (`METRICS_TOKEN` empty), and a label that
    /// said *why* a sign-in failed (no such account, disabled, wrong password) would tell anyone which accounts exist. The reason is in the audit log.
    pub auth_logins: IntCounterVec,
    /// Argon2 verifications. A sign-in that is refused before the password is looked at does not count, one that is looked at counts exactly once.
    pub auth_password_checks: IntCounter,
    /// Heavy requests (the engine's seconds of CPU: `heavy::HEAVY_ROUTES`) running on this replica now, until the work ends and not the request.
    pub heavy_in_flight: IntGauge,
    /// Heavy requests refused with `429`, by the limit that said no: `user` or `replica`.
    pub heavy_rejected: IntCounterVec,
}

impl Metrics {
    pub fn new(instance: &str) -> Self {
        let registry = Registry::new_custom(Some("infra".into()), Some([("instance".to_string(), instance.to_string())].into())).expect("registry");
        let http_requests = IntCounterVec::new(Opts::new("http_requests_total", "HTTP requests"), &["method", "route", "status"]).unwrap();
        let http_duration = HistogramVec::new(
            HistogramOpts::new("http_request_duration_seconds", "HTTP request duration").buckets(vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["method", "route"],
        )
        .unwrap();
        let llm_calls = IntCounterVec::new(Opts::new("llm_calls_total", "LLM calls"), &["purpose", "status"]).unwrap();
        let llm_cost_usd = Counter::new("llm_cost_usd_total", "LLM spend in USD").unwrap();
        let jobs_running = IntGauge::new("ai_jobs_running", "AI jobs running on this instance").unwrap();
        let jobs_finished = IntCounterVec::new(Opts::new("ai_jobs_finished_total", "Finished AI jobs"), &["kind", "status"]).unwrap();
        let evaluations = IntCounterVec::new(Opts::new("engine_evaluations_total", "Engine evaluations"), &["kind"]).unwrap();
        let auth_logins = IntCounterVec::new(Opts::new("auth_logins_total", "Sign-in attempts by outcome (ok, failed, locked, throttled)"), &["outcome"]).unwrap();
        let auth_password_checks = IntCounter::new("auth_password_checks_total", "Argon2 password verifications").unwrap();
        let heavy_in_flight = IntGauge::new("heavy_in_flight", "Heavy requests (engine work) running on this instance").unwrap();
        let heavy_rejected = IntCounterVec::new(Opts::new("heavy_rejected_total", "Heavy requests refused with 429, by the limit that said no (user, replica)"), &["scope"]).unwrap();
        for c in [
            Box::new(http_requests.clone()) as Box<dyn prometheus::core::Collector>,
            Box::new(http_duration.clone()),
            Box::new(llm_calls.clone()),
            Box::new(llm_cost_usd.clone()),
            Box::new(jobs_running.clone()),
            Box::new(jobs_finished.clone()),
            Box::new(evaluations.clone()),
            Box::new(auth_logins.clone()),
            Box::new(auth_password_checks.clone()),
            Box::new(heavy_in_flight.clone()),
            Box::new(heavy_rejected.clone()),
        ] {
            registry.register(c).unwrap();
        }
        Metrics { registry, http_requests, http_duration, llm_calls, llm_cost_usd, jobs_running, jobs_finished, evaluations, auth_logins, auth_password_checks, heavy_in_flight, heavy_rejected }
    }

    pub fn render(&self) -> String {
        let mut buf = vec![];
        TextEncoder::new().encode(&self.registry.gather(), &mut buf).ok();
        String::from_utf8(buf).unwrap_or_default()
    }
}
