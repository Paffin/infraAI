//! Prometheus metrics (served on `/metrics`).

use prometheus::{Counter, Encoder, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry, TextEncoder};

pub struct Metrics {
    pub registry: Registry,
    pub http_requests: IntCounterVec,
    pub http_duration: HistogramVec,
    pub llm_calls: IntCounterVec,
    pub llm_cost_usd: Counter,
    pub jobs_running: IntGauge,
    pub jobs_finished: IntCounterVec,
    pub evaluations: IntCounterVec,
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
        for c in [
            Box::new(http_requests.clone()) as Box<dyn prometheus::core::Collector>,
            Box::new(http_duration.clone()),
            Box::new(llm_calls.clone()),
            Box::new(llm_cost_usd.clone()),
            Box::new(jobs_running.clone()),
            Box::new(jobs_finished.clone()),
            Box::new(evaluations.clone()),
        ] {
            registry.register(c).unwrap();
        }
        Metrics { registry, http_requests, http_duration, llm_calls, llm_cost_usd, jobs_running, jobs_finished, evaluations }
    }

    pub fn render(&self) -> String {
        let mut buf = vec![];
        TextEncoder::new().encode(&self.registry.gather(), &mut buf).ok();
        String::from_utf8(buf).unwrap_or_default()
    }
}
