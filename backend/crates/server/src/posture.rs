//! The security posture of an installation: what an operator should know about how this server is set up, judged from its configuration and nothing else.
//!
//! A finding says a fact the server can back (the cookie has no `Secure` flag; `/metrics` is on the application's port with no token; the audit log keeps
//! 30 days) and what to do about it. It never says "secure": a check that does not fire says only that this one thing was not found. Nothing is
//! checked outside production (`APP_ENV`), where a development server is meant to be open.
//!
//! [`evaluate`] is a pure function of [`Inputs`]; [`gather`] reads them (boot configuration, settings, one query); the texts are data
//! (`defaults/posture.json`, `ru` and `en`, with `{name}` placeholders the check fills). The result is logged at start and returned as `posture` by
//! `GET /api/admin/system`. The thresholds are settings (`retention.auditDaysWarnBelow`).

use crate::config::{BootConfig, DEFAULT_ADMIN_EMAIL};
use crate::settings::Settings;
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

/// How much a finding asks of the operator: `Warn` should be fixed, `Info` is something to know about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Warn,
    Info,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Warn => "warn",
            Level::Info => "info",
        }
    }
}

/// One finding: which check (its id keys the text), how serious, and the values its text mentions.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub id: &'static str,
    pub level: Level,
    pub vars: Vec<(&'static str, String)>,
}

/// The ids of all checks; every one has a text in `defaults/posture.json` (a test holds the two together).
pub const IDS: [&str; 7] = ["cookie_insecure", "metrics_open", "default_admin_account", "trust_proxy", "llm_unbounded_spend", "audit_retention_short", "public_url_empty"];

/// What the checks look at, as plain values.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub production: bool,
    pub cookie_secure: bool,
    /// `observability.metricsEnabled`
    pub metrics_enabled: bool,
    pub metrics_token_set: bool,
    /// `METRICS_PORT` is set: the metrics are off the application's port
    pub metrics_on_own_port: bool,
    /// an enabled administrator with the published default address ([`DEFAULT_ADMIN_EMAIL`]) exists
    pub default_admin_account: bool,
    pub trust_proxy: bool,
    pub trust_proxy_hops: usize,
    /// `llm.enabled` and a key from the settings or the environment
    pub llm_active: bool,
    /// `llm.monthlyBudgetUsd` (0 = unlimited)
    pub llm_monthly_budget_usd: f64,
    /// `retention.auditDays`
    pub audit_days: i64,
    /// `retention.auditDaysWarnBelow` (0 = do not warn)
    pub audit_days_warn_below: i64,
    pub public_url_empty: bool,
}

/// The findings for these inputs, in a fixed order. Empty outside production.
pub fn evaluate(i: &Inputs) -> Vec<Check> {
    if !i.production {
        return vec![];
    }
    let mut out = vec![];
    if !i.cookie_secure {
        out.push(Check { id: "cookie_insecure", level: Level::Warn, vars: vec![] });
    }
    if i.metrics_enabled && !i.metrics_token_set && !i.metrics_on_own_port {
        out.push(Check { id: "metrics_open", level: Level::Warn, vars: vec![] });
    }
    if i.default_admin_account {
        out.push(Check { id: "default_admin_account", level: Level::Info, vars: vec![("email", DEFAULT_ADMIN_EMAIL.to_string())] });
    }
    if i.trust_proxy {
        out.push(Check { id: "trust_proxy", level: Level::Info, vars: vec![("hops", i.trust_proxy_hops.to_string())] });
    }
    if i.llm_active && i.llm_monthly_budget_usd <= 0.0 {
        out.push(Check { id: "llm_unbounded_spend", level: Level::Warn, vars: vec![] });
    }
    if i.audit_days_warn_below > 0 && i.audit_days < i.audit_days_warn_below {
        out.push(Check { id: "audit_retention_short", level: Level::Warn, vars: vec![("days", i.audit_days.to_string()), ("min", i.audit_days_warn_below.to_string())] });
    }
    if i.public_url_empty {
        out.push(Check { id: "public_url_empty", level: Level::Info, vars: vec![] });
    }
    out
}

/// The inputs of this installation: the boot configuration, the settings, and whether the default administrator is there.
pub fn inputs(boot: &BootConfig, settings: &Settings, llm_key_set: bool, default_admin_account: bool) -> Inputs {
    Inputs {
        production: boot.production,
        cookie_secure: boot.cookie_secure,
        metrics_enabled: settings.boolean("observability.metricsEnabled"),
        metrics_token_set: boot.metrics_token.is_some(),
        metrics_on_own_port: boot.metrics_port.is_some(),
        default_admin_account,
        trust_proxy: boot.trust_proxy,
        trust_proxy_hops: boot.trust_proxy_hops,
        llm_active: settings.boolean("llm.enabled") && llm_key_set,
        llm_monthly_budget_usd: settings.num("llm.monthlyBudgetUsd"),
        audit_days: settings.num("retention.auditDays") as i64,
        audit_days_warn_below: settings.num("retention.auditDaysWarnBelow") as i64,
        public_url_empty: settings.str("app.publicUrl").is_empty(),
    }
}

/// The texts of the checks: `{id: {message: {ru, en}, fix: {ru, en}}}`.
fn texts() -> &'static HashMap<String, Value> {
    static T: OnceLock<HashMap<String, Value>> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("../defaults/posture.json")).expect("embedded posture.json is valid"))
}

fn fill(text: &Value, vars: &[(&'static str, String)]) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(langs) = text.as_object() {
        for (lang, t) in langs {
            let mut s = t.as_str().unwrap_or_default().to_string();
            for (name, value) in vars {
                s = s.replace(&format!("{{{name}}}"), value);
            }
            out.insert(lang.clone(), Value::String(s));
        }
    }
    Value::Object(out)
}

/// A finding as the API says it: `{id, level, message: {ru, en}, fix: {ru, en}}`.
pub fn describe(check: &Check) -> Value {
    let text = texts().get(check.id).unwrap_or(&Value::Null);
    json!({"id": check.id, "level": check.level.as_str(), "message": fill(&text["message"], &check.vars), "fix": fill(&text["fix"], &check.vars)})
}

/// Reads what the checks need from the running server and judges it. The one fact that is not in the configuration, the default administrator, is
/// a query; if it cannot be asked the error is the caller's (a statement the server cannot back is not made).
pub async fn gather(state: &AppState) -> Result<Vec<Check>, crate::error::ApiError> {
    let cfg = state.cfg().await?;
    let default_admin: bool =
        sqlx::query_scalar("select exists(select 1 from users where lower(email) = $1 and role = 'admin' and not disabled)").bind(DEFAULT_ADMIN_EMAIL).fetch_one(&state.pool).await?;
    Ok(evaluate(&inputs(&state.boot, &cfg.settings, state.llm_key(&cfg).is_some(), default_admin)))
}

/// One line per finding at start-up (`warn` or `info`, in English: the log is for the operator's tools), and one `info` line when there is none.
pub async fn log(state: &AppState) {
    match gather(state).await {
        Ok(checks) if checks.is_empty() => tracing::info!(production = state.boot.production, "posture: no finding"),
        Ok(checks) => {
            for check in checks {
                let d = describe(&check);
                let (message, fix) = (d["message"]["en"].as_str().unwrap_or_default(), d["fix"]["en"].as_str().unwrap_or_default());
                match check.level {
                    Level::Warn => tracing::warn!(check = check.id, %message, %fix, "posture"),
                    Level::Info => tracing::info!(check = check.id, %message, %fix, "posture"),
                }
            }
        }
        Err(e) => tracing::warn!(error = %e.message, "posture could not be judged"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A production installation with nothing to say.
    fn safe() -> Inputs {
        Inputs {
            production: true,
            cookie_secure: true,
            metrics_enabled: true,
            metrics_token_set: false,
            metrics_on_own_port: true,
            default_admin_account: false,
            trust_proxy: false,
            trust_proxy_hops: 1,
            llm_active: false,
            llm_monthly_budget_usd: 0.0,
            audit_days: 365,
            audit_days_warn_below: 90,
            public_url_empty: false,
        }
    }

    fn ids(i: &Inputs) -> Vec<&'static str> {
        evaluate(i).iter().map(|c| c.id).collect()
    }

    #[test]
    fn posture_flags_each_unsafe_default() {
        assert_eq!(ids(&safe()), Vec::<&str>::new(), "a safe production installation has no finding");
        // (what is made unsafe, the check it fires, its level): each on its own, from the safe installation
        type Spoil = fn(&mut Inputs);
        let table: [(Spoil, &str, Level); 9] = [
            (|i| i.cookie_secure = false, "cookie_insecure", Level::Warn),
            (|i| (i.metrics_on_own_port, i.metrics_token_set) = (false, false), "metrics_open", Level::Warn),
            (|i| i.default_admin_account = true, "default_admin_account", Level::Info),
            (|i| i.trust_proxy = true, "trust_proxy", Level::Info),
            (|i| (i.llm_active, i.llm_monthly_budget_usd) = (true, 0.0), "llm_unbounded_spend", Level::Warn),
            (|i| i.audit_days = 30, "audit_retention_short", Level::Warn),
            (|i| i.public_url_empty = true, "public_url_empty", Level::Info),
            // one day below the threshold is a finding (the threshold itself is not: see what_is_not_unsafe_does_not_fire)
            (|i| i.audit_days = 89, "audit_retention_short", Level::Warn),
            // a budget below zero cannot be set, but would not be a budget
            (|i| (i.llm_active, i.llm_monthly_budget_usd) = (true, -1.0), "llm_unbounded_spend", Level::Warn),
        ];
        for (spoil, id, level) in table {
            let mut i = safe();
            spoil(&mut i);
            let found = evaluate(&i);
            assert_eq!(found.iter().map(|c| c.id).collect::<Vec<_>>(), vec![id], "{id}");
            assert_eq!(found[0].level, level, "{id}");
        }
        // all at once: every check, in the fixed order of IDS
        let mut all = safe();
        all.cookie_secure = false;
        all.metrics_on_own_port = false;
        all.default_admin_account = true;
        all.trust_proxy = true;
        all.llm_active = true;
        all.audit_days = 1;
        all.public_url_empty = true;
        assert_eq!(ids(&all), IDS.to_vec());
    }

    #[test]
    fn what_is_not_unsafe_does_not_fire() {
        // metrics open on the application's port are fine when switched off, when a token guards them, and when they are on a port of their own
        let spares: [fn(&mut Inputs); 2] = [|i| (i.metrics_on_own_port, i.metrics_enabled) = (false, false), |i| (i.metrics_on_own_port, i.metrics_token_set) = (false, true)];
        for spare in spares {
            let mut i = safe();
            spare(&mut i);
            assert!(ids(&i).is_empty());
        }
        // a budget silences the spend finding, an unused key (AI off, or no key) never fires it
        let mut i = safe();
        i.llm_active = true;
        i.llm_monthly_budget_usd = 25.0;
        assert!(ids(&i).is_empty());
        // 0 switches the retention check off; the threshold itself is enough
        i.audit_days = 5;
        i.audit_days_warn_below = 0;
        assert!(ids(&i).is_empty());
        i.audit_days = 90;
        i.audit_days_warn_below = 90;
        assert!(ids(&i).is_empty());
    }

    #[test]
    fn nothing_is_said_outside_production() {
        let mut i = safe();
        (i.cookie_secure, i.metrics_on_own_port, i.default_admin_account, i.trust_proxy, i.llm_active, i.audit_days, i.public_url_empty) = (false, false, true, true, true, 1, true);
        assert_eq!(ids(&i).len(), 7);
        i.production = false;
        assert!(evaluate(&i).is_empty(), "a development server is meant to be open");
    }

    #[test]
    fn every_check_has_a_text_in_both_languages_with_no_placeholder_left() {
        let texts = texts();
        let mut sorted: Vec<&String> = texts.keys().collect();
        sorted.sort();
        let mut want: Vec<&str> = IDS.to_vec();
        want.sort();
        assert_eq!(sorted, want, "posture.json and IDS name the same checks");
        // every check, filled with the values its own finding carries
        let mut all = safe();
        (all.cookie_secure, all.metrics_on_own_port, all.default_admin_account, all.trust_proxy, all.llm_active, all.audit_days, all.public_url_empty) = (false, false, true, true, true, 1, true);
        for check in evaluate(&all) {
            let d = describe(&check);
            assert_eq!(d["id"], check.id);
            for part in ["message", "fix"] {
                for lang in ["ru", "en"] {
                    let text = d[part][lang].as_str().unwrap_or_default();
                    assert!(text.len() > 20, "{} {part} {lang} has no text", check.id);
                    assert!(!text.contains('{') && !text.contains('}'), "{} {part} {lang} has a placeholder nobody filled: {text}", check.id);
                }
            }
        }
        // and the placeholders a text uses are the variables its check carries (a text may not ask for what the check does not give)
        for check in evaluate(&all) {
            let raw = serde_json::to_string(&texts[check.id]).unwrap();
            for name in check.vars.iter().map(|(n, _)| *n) {
                assert!(raw.contains(&format!("{{{name}}}")), "{} carries {name} and no text uses it", check.id);
            }
        }
    }

    #[test]
    fn the_threshold_is_a_setting_with_both_languages() {
        let def = crate::settings::def("retention.auditDaysWarnBelow").expect("the threshold is in the registry");
        assert_eq!(def.default, json!(90));
        assert!(def.label["ru"].as_str().is_some_and(|s| !s.is_empty()) && def.label["en"].as_str().is_some_and(|s| !s.is_empty()));
        let help = def.help.as_ref().expect("a help text");
        assert!(help["ru"].as_str().is_some_and(|s| !s.is_empty()) && help["en"].as_str().is_some_and(|s| !s.is_empty()));
    }
}
