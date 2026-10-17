//! Bootstrap configuration from the environment. Only what is needed *before* the database is
//! reachable lives here (and secrets that must not be stored in the database). Everything else
//! is a runtime setting edited in the admin panel (see `settings.rs`).

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BootConfig {
    pub bind: SocketAddr,
    pub database_url: String,
    pub db_max_connections: u32,
    pub db_connect_timeout_secs: u64,
    pub production: bool,
    pub app_secret: String,
    pub web_dir: PathBuf,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub openrouter_api_key: Option<String>,
    pub cookie_secure: bool,
    pub instance_id: String,
    pub metrics_token: Option<String>,
    pub trust_proxy: bool,
    pub shutdown_grace_secs: u64,
    pub log_json: bool,
    /// `SEED_DEMO_PROJECT`: overrides the setting `app.seedDemoProject` (useful before the first start, when no admin has set it).
    pub seed_demo_project: Option<bool>,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_bool(key: &str, default: bool) -> bool {
    env(key).map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on")).unwrap_or(default)
}

fn env_num<T: std::str::FromStr>(key: &str, default: T) -> T {
    env(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl BootConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let production = !matches!(env("APP_ENV").as_deref(), Some("development") | Some("dev") | Some("test"));
        let bind: SocketAddr =
            format!("{}:{}", env("HOST").unwrap_or_else(|| "0.0.0.0".into()), env("PORT").unwrap_or_else(|| "8080".into())).parse().map_err(|e| anyhow::anyhow!("invalid HOST/PORT: {e}"))?;
        let database_url = env("DATABASE_URL").ok_or_else(|| anyhow::anyhow!("DATABASE_URL is required (PostgreSQL connection string)"))?;
        let app_secret = match env("APP_SECRET") {
            Some(s) if s.len() >= 32 => s,
            Some(_) => anyhow::bail!("APP_SECRET must be at least 32 characters (generate one with: openssl rand -base64 48)"),
            None if production => anyhow::bail!("APP_SECRET is required in production (generate one with: openssl rand -base64 48). Set APP_ENV=development to use an ephemeral secret."),
            None => {
                tracing::warn!("APP_SECRET is not set: using an ephemeral development secret; sessions and encrypted settings will not survive a restart");
                use rand::RngCore;
                let mut b = [0u8; 48];
                rand::rngs::OsRng.fill_bytes(&mut b);
                base64_url(&b)
            }
        };
        Ok(BootConfig {
            bind,
            database_url,
            db_max_connections: pool_size(env_num("DB_MAX_CONNECTIONS", 10)),
            db_connect_timeout_secs: env_num("DB_CONNECT_TIMEOUT_SECONDS", 120),
            production,
            app_secret,
            web_dir: PathBuf::from(env("WEB_DIR").unwrap_or_else(|| "/app/web".into())),
            admin_email: env("ADMIN_EMAIL"),
            admin_password: env("ADMIN_PASSWORD"),
            openrouter_api_key: env("OPENROUTER_API_KEY"),
            cookie_secure: env_bool("COOKIE_SECURE", production),
            instance_id: env("INSTANCE_ID").unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string()),
            metrics_token: env("METRICS_TOKEN"),
            trust_proxy: env_bool("TRUST_PROXY", false),
            shutdown_grace_secs: env_num("SHUTDOWN_GRACE_SECONDS", 25),
            log_json: env_bool("LOG_JSON", production),
            seed_demo_project: env("SEED_DEMO_PROJECT").map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on")),
        })
    }
}

/// The least the pool can be: start-up holds one connection for the cluster-wide lock (`db::with_startup_lock`) and seeds through another,
/// so a pool of one never gets past the start.
pub const MIN_DB_CONNECTIONS: u32 = 2;

/// `DB_MAX_CONNECTIONS` as the pool uses it: never below [`MIN_DB_CONNECTIONS`].
pub fn pool_size(requested: u32) -> u32 {
    if requested < MIN_DB_CONNECTIONS {
        tracing::warn!(requested, used = MIN_DB_CONNECTIONS, "DB_MAX_CONNECTIONS is too small for the start-up (it needs two connections): using the minimum");
        return MIN_DB_CONNECTIONS;
    }
    requested
}

pub fn base64_url(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_is_never_smaller_than_the_start_up_needs() {
        assert_eq!((pool_size(0), pool_size(1), pool_size(2), pool_size(10)), (2, 2, 2, 10));
    }
}
