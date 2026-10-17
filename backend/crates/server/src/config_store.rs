//! Catalog, rules, templates, prompts and engine settings: seeded from the defaults embedded in
//! the binary, edited by administrators, cached per instance and refreshed through a revision
//! counter in the database (so every replica converges within `CONFIG_CACHE_TTL`).

use crate::crypto::{sha256_hex, Keys};
use crate::settings::Settings;
use engine::catalog::{Catalog, Category, EdgeTypeDef, KindDef, PropDef, ZoneTypeDef};
use engine::config::{EngineConfig, EngineSettings, IacTemplate, Rule, Template};
use engine::lifecycle::VersionSet;
use engine::placement::{Pattern, PlacementConfig, PlacementSettings, Stack};
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
pub const K_VERSIONS: &str = "catalog.versions";
pub const K_RULE: &str = "rule";
pub const K_TEMPLATE: &str = "template";
pub const K_PROMPT: &str = "prompt";
pub const K_ENGINE: &str = "engine";
pub const K_IAC: &str = "iac";
pub const K_PLACEMENT_PATTERN: &str = "placement.pattern";
pub const K_PLACEMENT_STACK: &str = "placement.stack";
pub const K_PLACEMENT_SETTINGS: &str = "placement.settings";

pub const ALL_KINDS: &[&str] = &[K_KIND, K_EDGE, K_ZONE, K_CATEGORY, K_COMMON, K_VERSIONS, K_RULE, K_TEMPLATE, K_PROMPT, K_ENGINE, K_IAC, K_PLACEMENT_PATTERN, K_PLACEMENT_STACK, K_PLACEMENT_SETTINGS];

pub const PROMPTS_JSON: &str = include_str!("../defaults/prompts.json");

/// The id of the only document of a kind that has exactly one (`None` for every other kind): such a kind is edited, never extended.
pub fn single_document_id(kind: &str) -> Option<&'static str> {
    match kind {
        K_COMMON => Some("common"),
        K_ENGINE => Some("engine"),
        K_PLACEMENT_SETTINGS => Some("placement"),
        _ => None,
    }
}

/// What an administrator may call an item they add: 1-80 characters of letters, digits, `_`, `-` and `.`. Creating and importing share this.
pub fn is_valid_item_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

pub const MSG_INVALID_ITEM_ID: &str = "id must be 1–80 characters of letters, digits, '_', '-', '.'";
pub const MSG_SINGLE_DOCUMENT: &str = "this kind has a single document and cannot be extended";

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
    let versions: Value = serde_json::from_str(engine::defaults::VERSIONS_JSON).expect("versions.json");
    push_list(&mut out, K_VERSIONS, Some(&versions));
    let rules: Value = serde_json::from_str(engine::defaults::RULES_JSON).expect("rules.json");
    push_list(&mut out, K_RULE, Some(&rules));
    for (i, (id, raw)) in engine::defaults::TEMPLATES_JSON.iter().enumerate() {
        out.push(DefaultItem { kind: K_TEMPLATE, id: id.to_string(), data: serde_json::from_str(raw).expect("template"), sort: i as i32 });
    }
    for (i, (id, raw)) in engine::defaults::IAC_JSON.iter().enumerate() {
        out.push(DefaultItem { kind: K_IAC, id: id.to_string(), data: serde_json::from_str(raw).expect("iac template"), sort: i as i32 });
    }
    let placement: Value = serde_json::from_str(engine::defaults::PLACEMENT_JSON).expect("placement.json");
    push_list(&mut out, K_PLACEMENT_PATTERN, placement.get("patterns"));
    push_list(&mut out, K_PLACEMENT_STACK, placement.get("stacks"));
    out.push(DefaultItem { kind: K_PLACEMENT_SETTINGS, id: "placement".into(), data: placement.get("settings").cloned().unwrap_or(json!({})), sort: 0 });
    let prompts: Value = serde_json::from_str(PROMPTS_JSON).expect("prompts.json");
    push_list(&mut out, K_PROMPT, Some(&prompts));
    out.push(DefaultItem { kind: K_ENGINE, id: "engine".into(), data: serde_json::from_str(engine::defaults::ENGINE_JSON).expect("engine.json"), sort: 0 });
    out
}

/// `asOf` of a release table (empty when it does not say).
fn as_of(table: &Value) -> String {
    table.get("asOf").and_then(|v| v.as_str()).unwrap_or("").to_string()
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
            Some((stored, modified, old_hash)) => {
                if old_hash.as_deref() == Some(h.as_str()) {
                    continue;
                }
                // a release table refreshed from its source is fresher than the one that ships with the program
                if it.kind == K_VERSIONS && as_of(&stored) >= as_of(&it.data) {
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

/// Move the configuration revision, which tells every replica to reload. Takes any executor so that a change made in a transaction
/// moves the revision in the same transaction: the change and the signal that it happened are committed together or not at all.
pub async fn bump_rev<'e>(db: impl sqlx::PgExecutor<'e>) -> Result<i64, sqlx::Error> {
    let v: (String,) = sqlx::query_as("update meta set value = (value::bigint + 1)::text where key = 'config_rev' returning value").fetch_one(db).await?;
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
    pub iac: Vec<IacTemplate>,
    pub placement: PlacementConfig,
    pub versions: Vec<VersionSet>,
    pub prompts: HashMap<String, PromptDef>,
    /// Items that failed to parse (kind, id, error) – surfaced in the admin panel.
    pub errors: Vec<(String, String, String)>,
    /// Ready-to-use engine configuration per UI language (generated names follow the language).
    engine_cfgs: HashMap<String, Arc<EngineConfig>>,
    /// The catalog as JSON, written once when the configuration is loaded: it is megabytes, and every `/bootstrap` carries it.
    pub catalog_json: Box<serde_json::value::RawValue>,
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
    Ok(Loaded::assemble(rev, settings, rows))
}

impl Loaded {
    /// The configuration exactly as shipped (settings defaults + embedded catalog, rules, templates, prompts,
    /// engine settings, IaC templates) – what a fresh installation starts with. No database needed.
    pub fn embedded_defaults() -> Loaded {
        let mut rows: Vec<(String, String, Value)> = default_items().into_iter().map(|i| (i.kind.to_string(), i.id, i.data)).collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Loaded::assemble(0, Settings::defaults(), rows)
    }

    /// The shipped configuration with an instance's configuration backup (`GET /admin/config-export`) laid over it: what that instance
    /// runs. Every item is validated like an administrator's edit, so a broken backup is refused with the reason instead of being half
    /// applied. No database needed.
    pub fn from_export(export: &Value) -> Result<Loaded, String> {
        match export.get("format").and_then(|v| v.as_str()) {
            Some("infra-architect-config/1") | None => {}
            Some(other) => return Err(format!("unknown configuration format '{other}' (expected 'infra-architect-config/1')")),
        }
        // the defaults keep their order (rules and templates are listed in it); what the backup adds goes after them
        let mut rows: Vec<(String, String, Value)> = default_items().into_iter().map(|i| (i.kind.to_string(), i.id, i.data)).collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let mut problems = vec![];
        for it in export.get("items").and_then(|v| v.as_array()).map(|a| a.as_slice()).unwrap_or(&[]) {
            let (Some(kind), Some(id), Some(data)) = (it.get("kind").and_then(|v| v.as_str()), it.get("id").and_then(|v| v.as_str()), it.get("data")) else {
                problems.push("an item lacks kind, id or data".to_string());
                continue;
            };
            if let Err(e) = validate_item(kind, id, data) {
                problems.push(format!("{kind}:{id}: {e}"));
                continue;
            }
            match rows.iter_mut().find(|r| r.0 == kind && r.1 == id) {
                Some(r) => r.2 = data.clone(),
                None => rows.push((kind.to_string(), id.to_string(), data.clone())),
            }
        }
        let mut settings = Settings::defaults();
        if let Some(changed) = export.get("settings").and_then(|v| v.as_object()) {
            for (k, v) in changed {
                match settings.clone().with(k, v.clone()) {
                    Ok(s) => settings = s,
                    Err(e) => problems.push(e),
                }
            }
        }
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        let loaded = Loaded::assemble(0, settings, rows);
        if let Some((kind, id, e)) = loaded.errors.first() {
            return Err(format!("{kind}:{id}: {e}"));
        }
        Ok(loaded)
    }

    fn assemble(rev: i64, settings: Settings, rows: Vec<(String, String, Value)>) -> Loaded {
        let mut errors = vec![];
        let mut catalog = Catalog::default();
        let mut rules = vec![];
        let mut templates = vec![];
        let mut iac = vec![];
        let mut placement = PlacementConfig::default();
        let mut versions = vec![];
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
                K_VERSIONS => match serde_json::from_value::<VersionSet>(data) {
                    Ok(v) => versions.push(v),
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
                K_IAC => match serde_json::from_value::<IacTemplate>(data) {
                    Ok(v) => iac.push(v),
                    Err(e) => fail(e.to_string()),
                },
                K_PLACEMENT_PATTERN => match serde_json::from_value::<Pattern>(data) {
                    Ok(v) => placement.patterns.push(v),
                    Err(e) => fail(e.to_string()),
                },
                K_PLACEMENT_STACK => match serde_json::from_value::<Stack>(data) {
                    Ok(v) => placement.stacks.push(v),
                    Err(e) => fail(e.to_string()),
                },
                K_PLACEMENT_SETTINGS => match serde_json::from_value::<PlacementSettings>(data) {
                    Ok(v) => placement.settings = v,
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
        placement.patterns.sort_by_key(|p| p.order);
        placement.stacks.sort_by_key(|p| p.order);
        let wanted = &placement.settings.default_pattern;
        if !wanted.is_empty() && !placement.patterns.iter().any(|p| p.enabled && &p.id == wanted) {
            errors.push((K_PLACEMENT_SETTINGS.to_string(), "placement".to_string(), format!("defaultPattern '{wanted}' is not an enabled placement pattern; the wizard offers the first one")));
        }
        let mut langs = settings.list("app.languages");
        if langs.is_empty() {
            langs.push(settings.str("app.defaultLanguage"));
        }
        let engine_cfgs = langs
            .into_iter()
            .map(|l| {
                let c = EngineConfig {
                    catalog: catalog.clone(),
                    rules: rules.clone(),
                    settings: engine_settings.clone(),
                    iac: iac.clone(),
                    placement: placement.clone(),
                    versions: versions.clone(),
                    today: crate::versions::today_utc,
                    lang: l.clone(),
                };
                (l, Arc::new(c))
            })
            .collect();
        let catalog_json = serde_json::value::to_raw_value(&catalog).expect("the catalog is JSON");
        Loaded { rev, settings, catalog, rules, engine: engine_settings, templates, iac, placement, versions, prompts, errors, engine_cfgs, catalog_json }
    }
}

/// Validate an item before it is written (so the admin cannot break the running system).
pub fn validate_item(kind: &str, id: &str, data: &Value) -> Result<(), String> {
    let id_in_data = data.get("id").and_then(|v| v.as_str());
    let needs_id = !matches!(kind, K_COMMON | K_ENGINE | K_PLACEMENT_SETTINGS);
    if needs_id && id_in_data != Some(id) {
        return Err(format!("data.id must equal '{id}'"));
    }
    let r: Result<(), serde_json::Error> = match kind {
        K_CATEGORY => serde_json::from_value::<Category>(data.clone()).map(|_| ()),
        K_ZONE => serde_json::from_value::<ZoneTypeDef>(data.clone()).map(|_| ()),
        K_EDGE => serde_json::from_value::<EdgeTypeDef>(data.clone()).map(|_| ()),
        K_KIND => serde_json::from_value::<KindDef>(data.clone()).map(|_| ()),
        K_COMMON => serde_json::from_value::<Vec<PropDef>>(data.clone()).map(|_| ()),
        K_VERSIONS => return validate_versions(data),
        K_RULE => serde_json::from_value::<Rule>(data.clone()).map(|_| ()),
        K_TEMPLATE => serde_json::from_value::<Template>(data.clone()).map(|_| ()),
        K_PROMPT => serde_json::from_value::<PromptDef>(data.clone()).map(|_| ()),
        K_ENGINE => serde_json::from_value::<EngineSettings>(data.clone()).map(|_| ()),
        K_PLACEMENT_PATTERN => serde_json::from_value::<Pattern>(data.clone()).map(|_| ()),
        K_PLACEMENT_STACK => serde_json::from_value::<Stack>(data.clone()).map(|_| ()),
        K_PLACEMENT_SETTINGS => serde_json::from_value::<PlacementSettings>(data.clone()).map(|_| ()),
        K_IAC => {
            // the template must parse and every expression in it must compile
            let t: IacTemplate = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
            let errs = engine::iac::validate_template(&t);
            if !errs.is_empty() {
                return Err(errs.join("; "));
            }
            Ok(())
        }
        _ => return Err(format!("unknown config kind '{kind}'")),
    };
    r.map_err(|e| e.to_string())
}

/// A release table must be readable by the engine and make sense: dates parse, lines are unique, newest-first order is not
/// required (the engine sorts), an empty table says nothing and is refused.
fn validate_versions(data: &Value) -> Result<(), String> {
    let set: VersionSet = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
    if set.cycles.is_empty() {
        return Err("a release table needs at least one release line".into());
    }
    if !set.as_of.is_empty() && engine::lifecycle::parse_date(&set.as_of).is_none() {
        return Err(format!("asOf '{}' is not a date (YYYY-MM-DD)", set.as_of));
    }
    let mut seen = std::collections::HashSet::new();
    for c in &set.cycles {
        if c.cycle.trim().is_empty() || !seen.insert(c.cycle.as_str()) {
            return Err(format!("release line '{}' is empty or repeated", c.cycle));
        }
        for d in [c.release_date.as_deref(), c.latest_release_date.as_deref(), c.support.as_ref().and_then(|x| x.date()), c.eol.as_ref().and_then(|x| x.date())].into_iter().flatten() {
            if engine::lifecycle::parse_date(d).is_none() {
                return Err(format!("release line '{}': '{d}' is not a date (YYYY-MM-DD)", c.cycle));
            }
        }
    }
    Ok(())
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

    /// A backup of a changed built-in item must be importable: importing holds items to the id rule of creating them, and a kind with one document to its id.
    #[test]
    fn every_shipped_item_passes_the_rules_that_an_import_enforces() {
        for it in default_items() {
            assert!(is_valid_item_id(&it.id), "{}:{} would be refused by an import", it.kind, it.id);
            if let Some(only) = single_document_id(it.kind) {
                assert_eq!(it.id, only, "the single document of {}", it.kind);
            }
        }
        for kind in [K_COMMON, K_ENGINE, K_PLACEMENT_SETTINGS] {
            assert_eq!(default_items().iter().filter(|i| i.kind == kind).count(), 1, "{kind} has exactly one shipped document");
            assert!(single_document_id(kind).is_some());
        }
        assert!(single_document_id(K_RULE).is_none() && !is_valid_item_id("") && !is_valid_item_id(&"x".repeat(81)) && !is_valid_item_id("a b") && is_valid_item_id("HA-SPOF-01"));
    }
}
