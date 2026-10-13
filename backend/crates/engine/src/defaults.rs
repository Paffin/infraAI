//! Default configuration shipped with the binary (embedded at compile time).
//!
//! These are *seeds*: the server copies them into the database on first start and the admin
//! panel edits the database copy. Upgrades never overwrite admin modifications (see the
//! server's seeding logic, which tracks a content hash per item).

use crate::catalog::Catalog;
use crate::config::{EngineSettings, IacTemplate, Rule, Template};
use crate::model::Architecture;
use crate::text::LText;
use serde::{Deserialize, Serialize};

pub const CATALOG_JSON: &str = include_str!("../defaults/catalog.json");
pub const ENGINE_JSON: &str = include_str!("../defaults/engine.json");
pub const RULES_JSON: &str = include_str!("../defaults/rules.json");
pub const DEMO_PROJECT_JSON: &str = include_str!("../defaults/demo_project.json");
pub const PLACEMENT_JSON: &str = include_str!("../defaults/placement.json");

pub const TEMPLATES_JSON: &[(&str, &str)] = &[
    ("two-dc-witness", include_str!("../defaults/templates/two-dc-witness.json")),
    ("three-az-region", include_str!("../defaults/templates/three-az-region.json")),
    ("multi-region-pilot-light", include_str!("../defaults/templates/multi-region-pilot-light.json")),
    ("event-driven-kafka", include_str!("../defaults/templates/event-driven-kafka.json")),
    ("single-dc-racks", include_str!("../defaults/templates/single-dc-racks.json")),
    ("three-sites-quorum", include_str!("../defaults/templates/three-sites-quorum.json")),
    ("five-sites-two-failures", include_str!("../defaults/templates/five-sites-two-failures.json")),
    ("ten-sites-regions", include_str!("../defaults/templates/ten-sites-regions.json")),
];

/// Infrastructure-as-code generators, one per catalog kind (see `iac`).
pub const IAC_JSON: &[(&str, &str)] = &[
    ("users", include_str!("../defaults/iac/users.json")),
    ("gslb", include_str!("../defaults/iac/gslb.json")),
    ("cdn", include_str!("../defaults/iac/cdn.json")),
    ("waf", include_str!("../defaults/iac/waf.json")),
    ("l4", include_str!("../defaults/iac/l4.json")),
    ("l7", include_str!("../defaults/iac/l7.json")),
    ("vpn_dci", include_str!("../defaults/iac/vpn_dci.json")),
    ("kubernetes", include_str!("../defaults/iac/kubernetes.json")),
    ("vm_pool", include_str!("../defaults/iac/vm_pool.json")),
    ("functions", include_str!("../defaults/iac/functions.json")),
    ("api_gateway", include_str!("../defaults/iac/api_gateway.json")),
    ("postgresql", include_str!("../defaults/iac/postgresql.json")),
    ("redis", include_str!("../defaults/iac/redis.json")),
    ("kafka", include_str!("../defaults/iac/kafka.json")),
    ("clickhouse", include_str!("../defaults/iac/clickhouse.json")),
    ("object_storage", include_str!("../defaults/iac/object_storage.json")),
    ("opensearch", include_str!("../defaults/iac/opensearch.json")),
    ("backup_storage", include_str!("../defaults/iac/backup_storage.json")),
    ("etcd", include_str!("../defaults/iac/etcd.json")),
    ("vault", include_str!("../defaults/iac/vault.json")),
    ("monitoring", include_str!("../defaults/iac/monitoring.json")),
    ("gitops", include_str!("../defaults/iac/gitops.json")),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoProject {
    pub name: String,
    pub description: LText,
    pub architecture: Architecture,
}

pub fn catalog() -> Catalog {
    serde_json::from_str(CATALOG_JSON).expect("embedded catalog.json is valid")
}

pub fn engine_settings() -> EngineSettings {
    serde_json::from_str(ENGINE_JSON).expect("embedded engine.json is valid")
}

pub fn rules() -> Vec<Rule> {
    serde_json::from_str(RULES_JSON).expect("embedded rules.json is valid")
}

pub fn templates() -> Vec<Template> {
    TEMPLATES_JSON.iter().map(|(id, s)| serde_json::from_str(s).unwrap_or_else(|e| panic!("embedded template {id} is valid: {e}"))).collect()
}

pub fn iac_templates() -> Vec<IacTemplate> {
    IAC_JSON.iter().map(|(id, s)| serde_json::from_str(s).unwrap_or_else(|e| panic!("embedded iac template {id} is valid: {e}"))).collect()
}

pub fn placement() -> crate::placement::PlacementConfig {
    serde_json::from_str(PLACEMENT_JSON).expect("embedded placement.json is valid")
}

/// The complete engine configuration as shipped (what a fresh installation starts with).
pub fn engine_config(lang: &str) -> crate::config::EngineConfig {
    crate::config::EngineConfig { catalog: catalog(), rules: rules(), settings: engine_settings(), iac: iac_templates(), placement: placement(), lang: lang.into() }
}

pub fn demo_project() -> DemoProject {
    serde_json::from_str(DEMO_PROJECT_JSON).expect("embedded demo_project.json is valid")
}
