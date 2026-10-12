//! OpenRouter client. Everything that can vary – base URL, models, fallbacks, provider routing,
//! privacy flags, structured-output mode, retries, timeouts – comes from runtime settings.
//!
//! API facts relied upon (verified against the OpenRouter documentation):
//! * `POST {base}/chat/completions`, Bearer auth, optional `HTTP-Referer` / `X-Title` headers;
//! * `models: [...]` provides ordered fallbacks; `provider` holds routing preferences
//!   (`data_collection`, `zdr`, `require_parameters`, `allow_fallbacks`, `sort`, `only`, `ignore`);
//! * `response_format` with `json_schema` / `json_object`; support is per model/endpoint and is
//!   advertised in `GET /models` → `supported_parameters`;
//! * every response carries `usage` including `cost` (credits ≈ USD).

use crate::config_store::Loaded;
use crate::crypto::sha256_hex;
use crate::error::ApiError;
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    Planner,
    Fast,
}

#[derive(Debug, Clone)]
pub struct Msg {
    pub role: &'static str,
    pub content: String,
}

impl Msg {
    pub fn system(c: impl Into<String>) -> Self {
        Msg { role: "system", content: c.into() }
    }
    pub fn user(c: impl Into<String>) -> Self {
        Msg { role: "user", content: c.into() }
    }
    pub fn assistant(c: impl Into<String>) -> Self {
        Msg { role: "assistant", content: c.into() }
    }
}

pub struct LlmRequest {
    pub purpose: &'static str,
    pub role: ModelRole,
    pub messages: Vec<Msg>,
    /// JSON Schema of the expected answer (`name`, schema). `None` = free text.
    pub schema: Option<(&'static str, Value)>,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("AI is not configured: set the OpenRouter API key in the admin panel")]
    NotConfigured,
    #[error("AI is disabled by the administrator")]
    Disabled,
    #[error("monthly AI budget exhausted")]
    Budget,
    #[error("{message}")]
    Http { status: u16, message: String },
    #[error("the model did not answer within the timeout")]
    Timeout,
    #[error("the model returned an empty answer or refused")]
    Empty,
    #[error("the answer was cut off (max output tokens reached)")]
    Truncated,
    #[error("{0}")]
    Other(String),
}

impl LlmError {
    pub fn code(&self) -> &'static str {
        match self {
            LlmError::NotConfigured => "llm_not_configured",
            LlmError::Disabled => "llm_disabled",
            LlmError::Budget => "llm_budget",
            LlmError::Http { status: 401, .. } | LlmError::Http { status: 403, .. } => "llm_auth",
            LlmError::Http { status: 402, .. } => "llm_credits",
            LlmError::Http { status: 429, .. } => "llm_rate_limited",
            LlmError::Http { .. } => "llm_http",
            LlmError::Timeout => "llm_timeout",
            LlmError::Empty => "llm_empty",
            LlmError::Truncated => "llm_truncated",
            LlmError::Other(_) => "llm_error",
        }
    }
}

pub struct CallCtx {
    pub job_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
}

/// Request parameters the model advertises (`None` = unknown, e.g. the catalog is unreachable).
/// With `provider.require_parameters` an unsupported parameter makes routing fail ("No endpoints
/// found"), so optional parameters are only sent when the model is known to accept them.
async fn model_params(state: &AppState, cfg: &Loaded, model: &str) -> Option<Vec<String>> {
    let list = fetch_models(state, cfg, false).await.ok()?;
    let m = list.as_array()?.iter().find(|m| m["id"] == model)?;
    Some(m["supportedParameters"].as_array()?.iter().filter_map(|p| p.as_str().map(String::from)).collect())
}

fn accepts(params: &Option<Vec<String>>, wanted: &[&str]) -> bool {
    match params {
        None => true,
        Some(p) => wanted.iter().any(|w| p.iter().any(|x| x == w)),
    }
}

fn extract_message(v: &Value) -> Option<String> {
    let c = &v["choices"][0]["message"]["content"];
    match c {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Array(parts) => {
            let t: String = parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("");
            if t.trim().is_empty() { None } else { Some(t) }
        }
        _ => None,
    }
}

async fn month_spend(state: &AppState) -> f64 {
    sqlx::query_scalar::<_, Option<f64>>("select sum(cost_usd) from llm_calls where created_at >= date_trunc('month', now())")
        .fetch_one(&state.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(0.0)
}

pub async fn call(state: &AppState, cfg: &Loaded, ctx: &CallCtx, req: LlmRequest) -> Result<LlmResponse, LlmError> {
    let s = &cfg.settings;
    if !s.boolean("llm.enabled") {
        return Err(LlmError::Disabled);
    }
    let key = state.llm_key(cfg).ok_or(LlmError::NotConfigured)?;
    let budget = s.num("llm.monthlyBudgetUsd");
    if budget > 0.0 && month_spend(state).await >= budget {
        return Err(LlmError::Budget);
    }

    let primary = match req.role {
        ModelRole::Planner => s.str("llm.model.planner"),
        ModelRole::Fast => s.str("llm.model.fast"),
    };
    let fallbacks = s.list("llm.model.fallbacks");
    let supported = model_params(state, cfg, &primary).await;
    let build = |degraded: bool| -> Value {
        let mut body = json!({
            "messages": req.messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
            "stream": false,
        });
        if accepts(&supported, &["max_tokens", "max_completion_tokens"]) {
            body["max_tokens"] = json!(s.num("llm.maxOutputTokens") as i64);
        }
        if !degraded && accepts(&supported, &["temperature"]) {
            body["temperature"] = json!(s.num("llm.temperature"));
        }
        if fallbacks.is_empty() {
            body["model"] = json!(primary);
        } else {
            let mut all = vec![primary.clone()];
            all.extend(fallbacks.iter().filter(|f| *f != &primary).cloned());
            body["models"] = json!(all);
        }
        // structured output: json_schema > json_object > prompt-only, subject to model support
        if let (Some((name, schema)), false) = (&req.schema, degraded) {
            let mode = s.str("llm.structuredOutput");
            if mode == "json_schema" && accepts(&supported, &["structured_outputs"]) {
                body["response_format"] = json!({"type": "json_schema", "json_schema": {"name": name, "strict": false, "schema": schema}});
            } else if mode != "prompt" && accepts(&supported, &["response_format"]) {
                body["response_format"] = json!({"type": "json_object"});
            }
        }
        let effort = s.str("llm.reasoningEffort");
        if !degraded && !effort.is_empty() && accepts(&supported, &["reasoning", "reasoning_effort"]) {
            body["reasoning"] = json!({"effort": effort});
        }
        let mut provider = json!({
            "data_collection": s.str("llm.provider.dataCollection"),
            "require_parameters": s.boolean("llm.provider.requireParameters") && !degraded,
            "allow_fallbacks": s.boolean("llm.provider.allowFallbacks"),
        });
        if s.boolean("llm.provider.zdr") {
            provider["zdr"] = json!(true);
        }
        let sort = s.str("llm.provider.sort");
        if !sort.is_empty() {
            provider["sort"] = json!(sort);
        }
        let only = s.list("llm.provider.only");
        if !only.is_empty() {
            provider["only"] = json!(only);
        }
        let ignore = s.list("llm.provider.ignore");
        if !ignore.is_empty() {
            provider["ignore"] = json!(ignore);
        }
        body["provider"] = provider;
        body
    };
    let mut degraded = false;
    let mut body = build(false);

    let url = format!("{}/chat/completions", s.str("llm.baseUrl").trim_end_matches('/'));
    let timeout = Duration::from_secs(s.num("llm.timeoutSeconds") as u64);
    let retries = s.num("llm.retries") as u32;
    let prompt_hash = sha256_hex(serde_json::to_string(&body["messages"]).unwrap_or_default().as_bytes());
    let started = Instant::now();

    let mut attempt = 0u32;
    let outcome: Result<LlmResponse, LlmError> = loop {
        let mut rb = state.http.post(&url).bearer_auth(&key).timeout(timeout).json(&body);
        let referer = s.str("llm.appReferer");
        if !referer.is_empty() {
            rb = rb.header("HTTP-Referer", referer);
        }
        let title = s.str("llm.appTitle");
        if !title.is_empty() {
            rb = rb.header("X-Title", title);
        }
        let res = rb.send().await;
        let step: Result<LlmResponse, (LlmError, bool)> = match res {
            Err(e) if e.is_timeout() => Err((LlmError::Timeout, true)),
            Err(e) => Err((LlmError::Other(format!("network error: {}", e.without_url())), true)),
            Ok(r) => {
                let status = r.status().as_u16();
                let text = r.text().await.unwrap_or_default();
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if status >= 400 || v.get("error").map(|e| !e.is_null()).unwrap_or(false) {
                    let msg = v["error"]["message"].as_str().map(String::from).unwrap_or_else(|| text.chars().take(300).collect());
                    let code = if status >= 400 { status } else { v["error"]["code"].as_u64().unwrap_or(502) as u16 };
                    let retryable = matches!(code, 408 | 429 | 500 | 502 | 503 | 504);
                    Err((LlmError::Http { status: code, message: msg }, retryable))
                } else {
                    match extract_message(&v) {
                        None => Err((LlmError::Empty, false)),
                        Some(content) => {
                            if v["choices"][0]["finish_reason"].as_str() == Some("length") {
                                Err((LlmError::Truncated, false))
                            } else {
                                Ok(LlmResponse {
                                    content,
                                    model: v["model"].as_str().unwrap_or(&primary).to_string(),
                                    prompt_tokens: v["usage"]["prompt_tokens"].as_i64().unwrap_or(0),
                                    completion_tokens: v["usage"]["completion_tokens"].as_i64().unwrap_or(0),
                                    cost_usd: v["usage"]["cost"].as_f64(),
                                })
                            }
                        }
                    }
                }
            }
        };
        match step {
            Ok(r) => break Ok(r),
            Err((e, retryable)) => {
                // no endpoint can serve the optional parameters: retry once without them
                // (privacy routing such as data_collection stays in force)
                if !degraded && matches!(&e, LlmError::Http { status: 404, message } if message.contains("No endpoints found")) {
                    tracing::warn!("no endpoint accepts the optional request parameters; retrying in degraded mode");
                    degraded = true;
                    body = build(true);
                    continue;
                }
                if retryable && attempt < retries {
                    attempt += 1;
                    let delay = Duration::from_millis(1000 * (1 << (attempt - 1)).min(8) + (rand::random::<u64>() % 400));
                    tracing::warn!(error = %e, attempt, "LLM call failed, retrying");
                    tokio::time::sleep(delay).await;
                    continue;
                }
                break Err(e);
            }
        }
    };

    let latency = started.elapsed().as_millis() as i32;
    let (status, model, pt, ct, cost, err) = match &outcome {
        Ok(r) => ("ok", Some(r.model.clone()), Some(r.prompt_tokens as i32), Some(r.completion_tokens as i32), r.cost_usd, None),
        Err(e) => ("error", None, None, None, None, Some(e.to_string())),
    };
    state.metrics.llm_calls.with_label_values(&[req.purpose, status]).inc();
    if let Some(c) = cost {
        state.metrics.llm_cost_usd.inc_by(c);
    }
    let _ = sqlx::query(
        "insert into llm_calls (job_id, project_id, user_id, purpose, requested_model, model, status, http_status, prompt_tokens, completion_tokens, cost_usd, latency_ms, error, prompt_hash)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(ctx.job_id)
    .bind(ctx.project_id)
    .bind(ctx.user_id)
    .bind(req.purpose)
    .bind(&primary)
    .bind(model)
    .bind(status)
    .bind(match &outcome {
        Err(LlmError::Http { status, .. }) => Some(*status as i32),
        _ => None,
    })
    .bind(pt)
    .bind(ct)
    .bind(cost)
    .bind(latency)
    .bind(err)
    .bind(prompt_hash)
    .execute(&state.pool)
    .await;
    outcome
}

/// Compact model list from `GET {base}/models` (public endpoint), cached.
pub async fn fetch_models(state: &AppState, cfg: &Loaded, refresh: bool) -> Result<Value, ApiError> {
    let ttl = Duration::from_secs(60 * cfg.settings.num("llm.modelsCacheMinutes").max(1.0) as u64);
    let mut guard = state.models_cache.lock().await;
    if !refresh {
        if let Some((at, v)) = guard.as_ref() {
            if at.elapsed() < ttl {
                return Ok(v.clone());
            }
        }
    }
    let url = format!("{}/models", cfg.settings.str("llm.baseUrl").trim_end_matches('/'));
    let res = state.http.get(&url).timeout(Duration::from_secs(30)).send().await.map_err(|e| ApiError::unavailable("llm_unreachable", format!("cannot reach the model catalog: {}", e.without_url())))?;
    if !res.status().is_success() {
        // serve a stale list rather than nothing
        if let Some((_, v)) = guard.as_ref() {
            return Ok(v.clone());
        }
        return Err(ApiError::unavailable("llm_unreachable", format!("model catalog returned HTTP {}", res.status())));
    }
    let raw: Value = res.json().await.map_err(|e| ApiError::unavailable("llm_unreachable", e.to_string()))?;
    let per_million = |v: &Value| v.as_str().and_then(|s| s.parse::<f64>().ok()).map(|x| x * 1_000_000.0);
    let list: Vec<Value> = raw["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|m| m["architecture"]["output_modalities"].as_array().map(|o| o.iter().any(|x| x == "text")).unwrap_or(true))
                .map(|m| {
                    let sp: Vec<Value> = m["supported_parameters"].as_array().cloned().unwrap_or_default();
                    json!({
                        "id": m["id"], "name": m["name"], "contextLength": m["context_length"],
                        "promptPerMillion": per_million(&m["pricing"]["prompt"]), "completionPerMillion": per_million(&m["pricing"]["completion"]),
                        "structuredOutputs": sp.iter().any(|p| p == "structured_outputs"),
                        "responseFormat": sp.iter().any(|p| p == "response_format"),
                        "supportedParameters": sp,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let v = Value::Array(list);
    *guard = Some((Instant::now(), v.clone()));
    Ok(v)
}

/// Pull the JSON object out of a model answer (handles code fences and chatter around it).
pub fn extract_json(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Some(v);
    }
    let unfenced = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```JSON"))
        .or_else(|| t.strip_prefix("```"))
        .map(|s| s.trim_end().trim_end_matches("```").trim())
        .unwrap_or(t);
    if let Ok(v) = serde_json::from_str::<Value>(unfenced) {
        return Some(v);
    }
    let start = unfenced.find('{')?;
    let end = unfenced.rfind('}')?;
    if end > start {
        serde_json::from_str(&unfenced[start..=end]).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fences_and_chatter() {
        assert_eq!(extract_json("{\"a\":1}").unwrap()["a"], 1);
        assert_eq!(extract_json("```json\n{\"a\":2}\n```").unwrap()["a"], 2);
        assert_eq!(extract_json("Sure! Here you go:\n{\"a\":3}\nHope it helps").unwrap()["a"], 3);
        assert!(extract_json("no json here").is_none());
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(LlmError::Http { status: 401, message: "x".into() }.code(), "llm_auth");
        assert_eq!(LlmError::Http { status: 402, message: "x".into() }.code(), "llm_credits");
        assert_eq!(LlmError::Budget.code(), "llm_budget");
    }
}
