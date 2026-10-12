//! Default configuration shipped with the binary (embedded at compile time).
//!
//! These are *seeds*: the server copies them into the database on first start and the admin
//! panel edits the database copy. Upgrades never overwrite admin modifications (see the
//! server's seeding logic, which tracks a content hash per item).

use crate::catalog::Catalog;
use crate::config::{EngineSettings, Rule, Template};
use crate::model::Architecture;
use crate::text::LText;
use serde::{Deserialize, Serialize};

pub const CATALOG_JSON: &str = include_str!("../defaults/catalog.json");
pub const ENGINE_JSON: &str = include_str!("../defaults/engine.json");
pub const RULES_JSON: &str = include_str!("../defaults/rules.json");
pub const DEMO_PROJECT_JSON: &str = include_str!("../defaults/demo_project.json");

pub const TEMPLATES_JSON: &[(&str, &str)] = &[
    ("two-dc-witness", include_str!("../defaults/templates/two-dc-witness.json")),
    ("three-az-region", include_str!("../defaults/templates/three-az-region.json")),
    ("multi-region-pilot-light", include_str!("../defaults/templates/multi-region-pilot-light.json")),
    ("event-driven-kafka", include_str!("../defaults/templates/event-driven-kafka.json")),
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

pub fn demo_project() -> DemoProject {
    serde_json::from_str(DEMO_PROJECT_JSON).expect("embedded demo_project.json is valid")
}
