//! Catalog, rules, templates, prompts and engine settings: seeded from the defaults embedded in
//! the binary, edited by administrators, cached per instance and refreshed through a revision
//! counter in the database (so every replica converges within `CONFIG_CACHE_TTL`).

use crate::crypto::{sha256_hex, Keys};
use crate::settings::Settings;
use engine::catalog::{Catalog, Category, EdgeTypeDef, KindDef, PropDef, ZoneTypeDef};
use engine::config::{EngineConfig, EngineSettings, Rule, Template};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub const K_KIND: &str = "catalog.kind";
pub const K_EDGE: &str = "catalog.edge_type";
pub const K_ZONE: &str = "catalog.zone_type";
pub const K_CATEGORY: &str = "catalog.category";
pub const K_COMMON: &str = "catalog.common_props";
pub const K_RULE: &str = "rule";
pub const K_TEMPLATE: &str = "template";
pub const K_PROMPT: &str = "prompt";
pub const K_ENGINE: &str = "engine";

pub const ALL_KINDS: &[&str] = &[K_KIND, K_EDGE, K_ZONE, K_CATEGORY, K_COMMON, K_RULE, K_TEMPLATE, K_PROMPT, K_ENGINE];

pub const PROMPTS_JSON: &str = include_str!("../defaults/prompts.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDef {
    pub id: String,
    pub title: Value,
    pub system: String,
    /// `planner` or `fast`: which configured model serves this prompt.
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct DefaultItem {
    pub kind: &'static str,
    pub id: String,
    pub data: Value,
    pub sort: i32,
}

/// All shipped defaults as `(kind, id, data)` items.
pub fn default_items() -> Vec<DefaultItem> {
    fn push_list(out: &mut Vec<DefaultItem>, kind: &'static str, arr: Option<&Value>) {
        if let Some(a) = arr.and_then(|v| v.as_array()) {
            for (i, item) in a.iter().enumerate() {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    out.push(DefaultItem { kind, id: id.to_string(), data: item.clone(), sort: i as i32 });
                }
            }
        }
    }
    let mut out = vec![];
    let cat: Value = serde_json::from_str(engine::defaults::CATALOG_JSON).expect("catalog.json");
    push_list(&mut out, K_CATEGORY, cat.get("categories"));
    push_list(&mut out, K_ZONE, cat.get("zoneTypes"));
    push_list(&mut out, K_EDGE, cat.get("edgeTypes"));
    push_list(&mut out, K_KIND, cat.get("kinds"));
    out.push(DefaultItem { kind: K_COMMON, id: "common".into(), data: cat.get("commonProps").cloned().unwrap_or(json!([])), sort: 0 });
    let rules: Value = serde_json::from_str(engine::defaults::RULES_JSON).expect("rules.json");
    push_list(&mut out, K_RULE, Some(&rules));
    for (i, (id, raw)) in engine::defaults::TEMPLATES_JSON.iter().enumerate() {
        out.push(DefaultItem { kind: K_TEMPLATE, id: id.to_string(), data: serde_json::from_str(raw).expect("template"), sort: i as i32 });
    }
    let prompts: Value = serde_json::from_str(PROMPTS_JSON).expect("prompts.json");
    push_list(&mut out, K_PROMPT, Some(&prompts));
    out.push(DefaultItem {
        kind: K_ENGINE,
        id: "engine".into(),
        data: serde_json::from_str(engine::defaults::ENGINE_JSON).expect("engine.json"),
        sort: 0,
    });
    out
}

fn hash_of(v: &Value) -> String {
    sha256_hex(serde_json::to_string(v).unwrap_or_default().as_bytes())
}

/// Insert missing defaults; upgrade unmodified ones; flag modified ones whose default changed.
/// Returns `true` if anything changed.
pub async fn seed_defaults(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let mut changed = false;
    for it in default_items() {
        let h = hash_of(&it.data);
        let row: Option<(Value, bool, Option<String>)> =
            sqlx::query_as("select data, modified, default_hash from config_items where kind = $1 and id = $2").bind(it.kind).bind(&it.id).fetch_optional(pool).await?;
        match row {
            None => {
                sqlx::query("insert into config_items (kind, id, data, origin, modified, default_hash, sort) values ($1, $2, $3, 'default', false, $4, $5)")
                    .bind(it.kind)
                    .bind(&it.id)
                    .bind(&it.data)
                    .bind(&h)
                    .bind(it.sort)
                    .execute(pool)
                    .await?;
                changed = true;
            }
            Some((_, modified, old_hash)) => {
                if old_hash.as_deref() == Some(h.as_str()) {
                    continue;
                }
                if !modified {
                    sqlx::query("update config_items set data = $3, default_hash = $4, update_available = false, sort = $5, updated_at = now() where kind = $1 and id = $2")
                        .bind(it.kind)
                        .bind(&it.id)
                        .bind(&it.data)
                        .bind(&h)
                        .bind(it.sort)
                        .execute(pool)
                        .await?;
                } else {
                    sqlx::query("update config_items set update_available = true where kind = $1 and id = $2").bind(it.kind).bind(&it.id).execute(pool).await?;
                }
                changed = true;
            }
        }
    }
    if changed {
        bump_rev(pool).await?;
    }
    Ok(changed)
}

pub async fn bump_rev(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let v: (String,) = sqlx::query_as("update meta set value = (value::bigint + 1)::text where key = 'config_rev' returning value").fetch_one(pool).await?;
    Ok(v.0.parse().unwrap_or(0))
}

pub async fn current_rev(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let v: (String,) = sqlx::query_as("select value from meta where key = 'config_rev'").fetch_one(pool).await?;
    Ok(v.0.parse().unwrap_or(0))
}

/// Immutable snapshot of the whole runtime configuration.
pub struct Loaded {
    pub rev: i64,
    pub settings: Settings,
    pub catalog: Catalog,
    pub rules: Vec<Rule>,
    pub engine: EngineSettings,
    pub templates: Vec<Template>,
    pub prompts: HashMap<String, PromptDef>,
    /// Items that failed to parse (kind, id, error) – surfaced in the admin panel.
    pub errors: Vec<(String, String, String)>,
    /// Ready-to-use engine configuration per UI language (generated names follow the language).
    engine_cfgs: HashMap<String, Arc<EngineConfig>>,
}

impl Loaded {
    /// Engine configuration for `lang` (falls back to the default language).
    pub fn engine_config(&self, lang: &str) -> Arc<EngineConfig> {
        self.engine_cfgs
            .get(lang)
            .or_else(|| self.engine_cfgs.get(&self.settings.str("app.defaultLanguage")))
            .or_else(|| self.engine_cfgs.values().next())
            .cloned()
            .expect("at least one engine configuration")
    }
}

pub async fn load(pool: &PgPool, keys: &Keys) -> Result<Loaded, sqlx::Error> {
    let rev = current_rev(pool).await?;
    let settings = Settings::load(pool, keys).await?;
    let rows: Vec<(String, String, Value)> = sqlx::query_as("select kind, id, data from config_items order by kind, sort, id").fetch_all(pool).await?;
    let mut errors = vec![];
    let mut catalog = Catalog::default();
    let mut rules = vec![];
    let mut templates = vec![];
    let mut prompts = HashMap::new();
    let mut engine_settings = EngineSettings::default();
    for (kind, id, data) in rows {
        let mut fail = |e: String| errors.push((kind.clone(), id.clone(), e));
        match kind.as_str() {
            K_CATEGORY => match serde_json::from_value::<Category>(data) {
                Ok(v) => catalog.categories.push(v),
                Err(e) => fail(e.to_string()),
            },
            K_ZONE => match serde_json::from_value::<ZoneTypeDef>(data) {
                Ok(v) => catalog.zone_types.push(v),
                Err(e) => fail(e.to_string()),
            },
            K_EDGE => match serde_json::from_value::<EdgeTypeDef>(data) {
                Ok(v) => catalog.edge_types.push(v),
                Err(e) => fail(e.to_string()),
            },
            K_KIND => match serde_json::from_value::<KindDef>(data) {
                Ok(v) => catalog.kinds.push(v),
                Err(e) => fail(e.to_string()),
            },
            K_COMMON => match serde_json::from_value::<Vec<PropDef>>(data) {
                Ok(v) => catalog.common_props = v,
                Err(e) => fail(e.to_string()),
            },
            K_RULE => match serde_json::from_value::<Rule>(data) {
                Ok(v) => rules.push(v),
                Err(e) => fail(e.to_string()),
            },
            K_TEMPLATE => match serde_json::from_value::<Template>(data) {
                Ok(v) => templates.push(v),
                Err(e) => fail(e.to_string()),
            },
            K_PROMPT => match serde_json::from_value::<PromptDef>(data) {
                Ok(v) => {
                    prompts.insert(v.id.clone(), v);
                }
                Err(e) => fail(e.to_string()),
            },
            K_ENGINE => match serde_json::from_value::<EngineSettings>(data) {
                Ok(v) => engine_settings = v,
                Err(e) => fail(e.to_string()),
            },
            _ => {}
        }
    }
    templates.sort_by_key(|t| t.order);
    let mut langs = settings.list("app.languages");
    if langs.is_empty() {
        langs.push(settings.str("app.defaultLanguage"));
    }
    let engine_cfgs = langs
        .into_iter()
        .map(|l| {
            let c = EngineConfig { catalog: catalog.clone(), rules: rules.clone(), settings: engine_settings.clone(), lang: l.clone() };
            (l, Arc::new(c))
        })
        .collect();
    Ok(Loaded { rev, settings, catalog, rules, engine: engine_settings, templates, prompts, errors, engine_cfgs })
}

/// Validate an item before it is written (so the admin cannot break the running system).
pub fn validate_item(kind: &str, id: &str, data: &Value) -> Result<(), String> {
    let id_in_data = data.get("id").and_then(|v| v.as_str());
    let needs_id = !matches!(kind, K_COMMON | K_ENGINE);
    if needs_id && id_in_data != Some(id) {
        return Err(format!("data.id must equal '{id}'"));
    }
    let r: Result<(), serde_json::Error> = match kind {
        K_CATEGORY => serde_json::from_value::<Category>(data.clone()).map(|_| ()),
        K_ZONE => serde_json::from_value::<ZoneTypeDef>(data.clone()).map(|_| ()),
        K_EDGE => serde_json::from_value::<EdgeTypeDef>(data.clone()).map(|_| ()),
        K_KIND => serde_json::from_value::<KindDef>(data.clone()).map(|_| ()),
        K_COMMON => serde_json::from_value::<Vec<PropDef>>(data.clone()).map(|_| ()),
        K_RULE => serde_json::from_value::<Rule>(data.clone()).map(|_| ()),
        K_TEMPLATE => serde_json::from_value::<Template>(data.clone()).map(|_| ()),
        K_PROMPT => serde_json::from_value::<PromptDef>(data.clone()).map(|_| ()),
        K_ENGINE => serde_json::from_value::<EngineSettings>(data.clone()).map(|_| ()),
        _ => return Err(format!("unknown config kind '{kind}'")),
    };
    r.map_err(|e| e.to_string())
}

/// Per-instance cache with revision-based refresh.
pub struct ConfigCache {
    inner: RwLock<Option<(Arc<Loaded>, Instant)>>,
    ttl: Duration,
}

impl ConfigCache {
    pub fn new(ttl: Duration) -> Self {
        ConfigCache { inner: RwLock::new(None), ttl }
    }

    pub async fn invalidate(&self) {
        *self.inner.write().await = None;
    }

    pub async fn get(&self, pool: &PgPool, keys: &Keys) -> Result<Arc<Loaded>, sqlx::Error> {
        {
            let g = self.inner.read().await;
            if let Some((l, at)) = g.as_ref() {
                if at.elapsed() < self.ttl {
                    return Ok(l.clone());
                }
            }
        }
        let mut g = self.inner.write().await;
        if let Some((l, at)) = g.as_mut() {
            if at.elapsed() < self.ttl {
                return Ok(l.clone());
            }
            if current_rev(pool).await? == l.rev {
                *at = Instant::now();
                return Ok(l.clone());
            }
        }
        let loaded = Arc::new(load(pool, keys).await?);
        *g = Some((loaded.clone(), Instant::now()));
        Ok(loaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_items_cover_everything_and_validate() {
        let items = default_items();
        assert!(items.iter().any(|i| i.kind == K_KIND) && items.iter().any(|i| i.kind == K_RULE) && items.iter().any(|i| i.kind == K_TEMPLATE));
        for it in &items {
            validate_item(it.kind, &it.id, &it.data).unwrap_or_else(|e| panic!("{}:{} invalid: {e}", it.kind, it.id));
        }
        let mut seen = std::collections::HashSet::new();
        for it in &items {
            assert!(seen.insert((it.kind, it.id.clone())), "duplicate default {}:{}", it.kind, it.id);
        }
    }
}
