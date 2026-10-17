//! Shared application state. Everything here is either immutable, a pool, or a cache that is safe
//! to lose: instances are stateless and can be added or removed at any time.

use crate::auth::CachedUser;
use crate::config::BootConfig;
use crate::config_store::{ConfigCache, Loaded};
use crate::crypto::{Keys, PasswordGate};
use crate::error::ApiError;
use crate::jobs::JobRunner;
use crate::metrics::Metrics;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub boot: Arc<BootConfig>,
    pub keys: Arc<Keys>,
    pub config: Arc<ConfigCache>,
    /// The model provider's client (default redirect policy).
    pub http: reqwest::Client,
    /// The client of the release-table sync: no redirects of its own (`outbound::sync_client`).
    pub fetch_http: reqwest::Client,
    pub metrics: Arc<Metrics>,
    pub jobs: Arc<JobRunner>,
    pub shutting_down: Arc<AtomicBool>,
    pub started_at: Instant,
    pub user_cache: Arc<Mutex<HashMap<Uuid, (CachedUser, Instant)>>>,
    /// Where passwords are hashed and checked: off the async workers, a bounded number at a time.
    pub passwords: Arc<PasswordGate>,
    pub models_cache: Arc<tokio::sync::Mutex<Option<(Instant, Value)>>>,
}

impl AppState {
    pub async fn cfg(&self) -> Result<Arc<Loaded>, ApiError> {
        self.config.get(&self.pool, &self.keys).await.map_err(ApiError::internal)
    }

    /// The effective OpenRouter API key: database setting first, then the environment.
    pub fn llm_key(&self, cfg: &Loaded) -> Option<String> {
        let k = cfg.settings.str("llm.apiKey");
        if !k.is_empty() {
            return Some(k);
        }
        self.boot.openrouter_api_key.clone()
    }

    pub fn env_secret_present(&self, key: &str) -> bool {
        match key {
            "llm.apiKey" => self.boot.openrouter_api_key.is_some(),
            _ => false,
        }
    }
}

pub const CONFIG_CACHE_TTL: Duration = Duration::from_secs(5);
