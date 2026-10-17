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
    /// `METRICS_PORT`: a second listener, on `HOST` and this port, that serves only `/metrics`; the main port then answers `404` there. Empty (the default) keeps
    /// `/metrics` on the main port, where an ingress that forwards `/` publishes it. A port that does not parse is an error, not "unset" (see [`parse_port`]).
    pub metrics_port: Option<u16>,
    pub trust_proxy: bool,
    /// `TRUST_PROXY_HOPS`: how many reverse proxies stand between the client and this server, each appending the address it saw to
    /// `X-Forwarded-For`. The client's address is that many entries from the right; only read when `trust_proxy` is on.
    pub trust_proxy_hops: usize,
    pub shutdown_grace_secs: u64,
    pub log_json: bool,
    /// `SEED_DEMO_PROJECT`: overrides the setting `app.seedDemoProject` (useful before the first start, when no admin has set it).
    pub seed_demo_project: Option<bool>,
}

/// Where the configuration is read from: the process environment, or (in tests) a table.
type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

fn env(get: Lookup, key: &str) -> Option<String> {
    get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_bool(get: Lookup, key: &str, default: bool) -> bool {
    env(get, key).map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on")).unwrap_or(default)
}

fn env_num<T: std::str::FromStr>(get: Lookup, key: &str, default: T) -> T {
    env(get, key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// A TCP port as an operator writes it: 1 to 65535. A value that does not parse is an error and not a default, because the settings it guards fail open:
/// `METRICS_PORT=90a0` read as "unset" would leave `/metrics` on the public port with no word about it.
pub fn parse_port(name: &str, value: &str) -> anyhow::Result<u16> {
    match value.trim().parse::<u16>() {
        Ok(port) if port != 0 => Ok(port),
        _ => anyhow::bail!("{name} must be a port number from 1 to 65535, not {value:?}"),
    }
}

/// The most proxies in front of the server that `TRUST_PROXY_HOPS` accepts (a chain longer than this is a mistake, not a topology).
pub const MAX_PROXY_HOPS: usize = 16;

/// A secret that is a placeholder from a template (`.env.example`, `deploy/k8s/secret.example.yaml`), not a secret: case, `-` and `_` do not matter.
pub fn is_placeholder_secret(secret: &str) -> bool {
    let norm = secret.trim().to_lowercase().replace('_', "-");
    norm.starts_with("change-me") || norm.starts_with("changeme")
}

impl BootConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(&|k| std::env::var(k).ok())
    }

    /// The configuration as `get` answers for each variable (the process environment for `from_env`; a table in the tests, which need no
    /// process-wide variables for it).
    pub fn from_lookup(get: Lookup) -> anyhow::Result<Self> {
        let production = !matches!(env(get, "APP_ENV").as_deref(), Some("development") | Some("dev") | Some("test"));
        let bind: SocketAddr =
            format!("{}:{}", env(get, "HOST").unwrap_or_else(|| "0.0.0.0".into()), env(get, "PORT").unwrap_or_else(|| "8080".into())).parse().map_err(|e| anyhow::anyhow!("invalid HOST/PORT: {e}"))?;
        let database_url = env(get, "DATABASE_URL").ok_or_else(|| anyhow::anyhow!("DATABASE_URL is required (PostgreSQL connection string)"))?;
        let app_secret = match env(get, "APP_SECRET") {
            Some(s) if production && is_placeholder_secret(&s) => anyhow::bail!(
                "APP_SECRET is the placeholder from .env.example (or deploy/k8s/secret.example.yaml): a published string is not a secret, anyone could sign a session with it. \
                 Generate one with `openssl rand -hex 32` and put it in .env, or move the old .env aside and run `make .env` (deploy/init-env.sh), which writes a new one \
                 with generated secrets (it never overwrites a .env that exists). Note that changing APP_SECRET signs everybody out \
                 and makes the secrets already stored in the database (the OpenRouter key) unreadable: enter them again in Administration."
            ),
            Some(s) if s.len() >= 32 => s,
            Some(_) => anyhow::bail!("APP_SECRET must be at least 32 characters (generate one with: openssl rand -hex 32)"),
            None if production => anyhow::bail!("APP_SECRET is required in production (generate one with: openssl rand -hex 32). Set APP_ENV=development to use an ephemeral secret."),
            None => {
                tracing::warn!("APP_SECRET is not set: using an ephemeral development secret; sessions and encrypted settings will not survive a restart");
                use rand::RngCore;
                let mut b = [0u8; 48];
                rand::rngs::OsRng.fill_bytes(&mut b);
                base64_url(&b)
            }
        };
        let metrics_port = match env(get, "METRICS_PORT") {
            None => None,
            Some(v) => {
                let port = parse_port("METRICS_PORT", &v)?;
                if port == bind.port() {
                    anyhow::bail!("METRICS_PORT is the port of the application itself ({port}): /metrics would stay where it is; give the metrics listener a port of its own");
                }
                Some(port)
            }
        };
        let trust_proxy = env_bool(get, "TRUST_PROXY", false);
        let trust_proxy_hops = match env(get, "TRUST_PROXY_HOPS") {
            None => 1,
            Some(v) => match v.parse::<usize>() {
                Ok(n) if (1..=MAX_PROXY_HOPS).contains(&n) => n,
                _ => anyhow::bail!("TRUST_PROXY_HOPS must be a whole number from 1 to {MAX_PROXY_HOPS} (how many reverse proxies stand in front of the server), not {v:?}"),
            },
        };
        if trust_proxy {
            tracing::warn!(
                hops = trust_proxy_hops,
                "TRUST_PROXY is on: the client address is read from X-Forwarded-For, counted from the right with TRUST_PROXY_HOPS; it must equal the number of reverse proxies in front of the server"
            );
        }
        Ok(BootConfig {
            bind,
            database_url,
            db_max_connections: pool_size(env_num(get, "DB_MAX_CONNECTIONS", 10)),
            db_connect_timeout_secs: env_num(get, "DB_CONNECT_TIMEOUT_SECONDS", 120),
            production,
            app_secret,
            web_dir: PathBuf::from(env(get, "WEB_DIR").unwrap_or_else(|| "/app/web".into())),
            admin_email: env(get, "ADMIN_EMAIL"),
            admin_password: env(get, "ADMIN_PASSWORD"),
            openrouter_api_key: env(get, "OPENROUTER_API_KEY"),
            cookie_secure: env_bool(get, "COOKIE_SECURE", production),
            instance_id: env(get, "INSTANCE_ID").unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string()),
            metrics_token: env(get, "METRICS_TOKEN"),
            metrics_port,
            trust_proxy,
            trust_proxy_hops,
            shutdown_grace_secs: env_num(get, "SHUTDOWN_GRACE_SECONDS", 25),
            log_json: env_bool(get, "LOG_JSON", production),
            seed_demo_project: env(get, "SEED_DEMO_PROJECT").map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on")),
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

    /// The configuration as a process would read it from these variables (and from nothing else).
    fn boot(vars: &[(&str, &str)]) -> anyhow::Result<BootConfig> {
        let table: std::collections::HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        BootConfig::from_lookup(&|k| table.get(k).cloned())
    }

    const DB: (&str, &str) = ("DATABASE_URL", "postgres://infra@localhost/infra");

    #[test]
    fn placeholder_app_secret_is_rejected_in_production() {
        // the values that are published: .env.example, the Kubernetes example, and the same in other cases
        for placeholder in [
            "change-me-change-me-change-me-change-me",
            "CHANGE_ME_CHANGE_ME_CHANGE_ME_CHANGE_ME",
            "Change-Me-Change-Me-Change-Me-Change-Me",
            "changeme-changeme-changeme-changeme",
            "  change_me_please_change_me_please_00  ",
        ] {
            // APP_ENV unset is production
            let e = boot(&[DB, ("APP_SECRET", placeholder)]).expect_err(placeholder).to_string();
            assert!(e.contains(".env.example") && e.contains("signs everybody out") && e.contains("OpenRouter"), "the message says where the value is from and what changing it costs: {e}");
            // `make .env` leaves an existing file alone (deploy/init-env.sh), so the way out it names must start with moving the old one away
            assert!(e.contains("move the old .env aside"), "the message does not send the operator to a command that would do nothing: {e}");
            assert!(boot(&[DB, ("APP_ENV", "production"), ("APP_SECRET", placeholder)]).is_err(), "{placeholder}");
            // development and tests may keep it: there is nothing to protect
            for env in ["development", "dev", "test"] {
                assert_eq!(boot(&[DB, ("APP_ENV", env), ("APP_SECRET", placeholder)]).unwrap().app_secret, placeholder.trim(), "{env}");
            }
        }
    }

    #[test]
    fn a_real_app_secret_is_accepted_and_a_short_one_is_not() {
        let hex = "9f2c1a7e5b3d48c0a6e1f7b2d9c4a083e5f6b7a8c9d0e1f2a3b4c5d6e7f80912";
        assert_eq!(boot(&[DB, ("APP_SECRET", hex)]).unwrap().app_secret, hex);
        // a secret that merely contains the word later on is a secret
        assert!(boot(&[DB, ("APP_SECRET", "9f2c1a7e5b3d48c0-change-me-a6e1f7b2d9c4a083e5f6")]).is_ok());
        let e = boot(&[DB, ("APP_SECRET", "too-short")]).unwrap_err().to_string();
        assert!(e.contains("at least 32"), "{e}");
        assert!(boot(&[DB]).unwrap_err().to_string().contains("required in production"));
        assert!(boot(&[DB, ("APP_ENV", "test")]).is_ok(), "development gets an ephemeral secret");
    }

    #[test]
    fn env_example_ships_no_usable_secret() {
        // .env.example is what `cp .env.example .env` copies: it must not contain a value that starts a server
        let example = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../.env.example")).expect(".env.example");
        for key in ["APP_SECRET", "POSTGRES_PASSWORD"] {
            let value = example.lines().find_map(|l| l.strip_prefix(&format!("{key}="))).unwrap_or_else(|| panic!("{key} is not in .env.example"));
            assert!(value.trim().is_empty() || is_placeholder_secret(value), "{key}={value}: a copy of the example would run with a published value");
        }
        let secret = example.lines().find_map(|l| l.strip_prefix("APP_SECRET=")).unwrap();
        assert!(boot(&[DB, ("APP_SECRET", secret)]).is_err(), "and the server refuses it");
    }

    #[test]
    fn metrics_port_is_unset_by_default_and_a_bad_value_is_an_error_not_unset() {
        assert_eq!(boot(&[DB, ("APP_ENV", "test")]).unwrap().metrics_port, None);
        assert_eq!(boot(&[DB, ("APP_ENV", "test"), ("METRICS_PORT", "  ")]).unwrap().metrics_port, None, "blank is unset, like every other variable");
        assert_eq!(boot(&[DB, ("APP_ENV", "test"), ("METRICS_PORT", "9090")]).unwrap().metrics_port, Some(9090));
        assert_eq!(boot(&[DB, ("APP_ENV", "test"), ("METRICS_PORT", " 65535 ")]).unwrap().metrics_port, Some(65535));
        // a value that is not a port must not fall back to "unset": that would leave /metrics on the public port with no word about it
        for bad in ["90a0", "0", "-1", "65536", "99999999999", "9090.5", "http://x:9090", "9090,9091"] {
            let e = boot(&[DB, ("APP_ENV", "test"), ("METRICS_PORT", bad)]).expect_err(bad).to_string();
            assert!(e.contains("METRICS_PORT") && e.contains("1 to 65535"), "{bad}: {e}");
        }
    }

    #[test]
    fn metrics_port_cannot_be_the_applications_own_port() {
        // the default PORT is 8080
        let e = boot(&[DB, ("APP_ENV", "test"), ("METRICS_PORT", "8080")]).unwrap_err().to_string();
        assert!(e.contains("METRICS_PORT") && e.contains("8080"), "{e}");
        let e = boot(&[DB, ("APP_ENV", "test"), ("PORT", "3000"), ("METRICS_PORT", "3000")]).unwrap_err().to_string();
        assert!(e.contains("3000"), "{e}");
        // the same number is fine when the application is elsewhere
        assert_eq!(boot(&[DB, ("APP_ENV", "test"), ("PORT", "3000"), ("METRICS_PORT", "8080")]).unwrap().metrics_port, Some(8080));
    }

    #[test]
    fn trust_proxy_hops_defaults_to_one_and_must_be_a_small_number() {
        assert_eq!(boot(&[DB, ("APP_ENV", "test")]).unwrap().trust_proxy_hops, 1);
        assert_eq!(boot(&[DB, ("APP_ENV", "test"), ("TRUST_PROXY", "true"), ("TRUST_PROXY_HOPS", "2")]).unwrap().trust_proxy_hops, 2);
        for bad in ["0", "-1", "17", "two", "1.5"] {
            let e = boot(&[DB, ("APP_ENV", "test"), ("TRUST_PROXY_HOPS", bad)]).expect_err(bad).to_string();
            assert!(e.contains("TRUST_PROXY_HOPS"), "{bad}: {e}");
        }
        assert!(!boot(&[DB, ("APP_ENV", "test")]).unwrap().trust_proxy, "behind no proxy by default");
    }
}
