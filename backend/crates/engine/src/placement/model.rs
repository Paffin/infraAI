//! Data model of the placement generator. Patterns and stacks are configuration (admin panel, seeds in
//! `defaults/placement.json`); the generator only interprets them.

use crate::model::{Props, Requirements};
use crate::text::LText;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}

/// How the sites of a pattern are grouped into disaster domains (`geo`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RegionMode {
    /// Every site shares one domain: a metro area or one cloud region with several availability zones.
    #[default]
    Single,
    /// Sites are split into `count` domains as evenly as possible.
    Count,
    /// Every site is its own domain: geographically distributed.
    PerSite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RegionPolicy {
    pub mode: RegionMode,
    /// Default number of domains for `count`.
    pub default: u32,
    pub min: u32,
    pub max: u32,
}

impl Default for RegionPolicy {
    fn default() -> Self {
        Self { mode: RegionMode::Single, default: 1, min: 1, max: 1 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RackPolicy {
    pub default: u32,
    pub min: u32,
    pub max: u32,
}

impl Default for RackPolicy {
    fn default() -> Self {
        Self { default: 1, min: 1, max: 1 }
    }
}

/// How many sites the design is meant to lose at once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FailurePolicy {
    /// `auto` = as many as the number of sites allows (a majority must remain), within `max`; `fixed` = `value`.
    pub mode: String,
    pub value: u32,
    pub max: u32,
}

impl Default for FailurePolicy {
    fn default() -> Self {
        Self { mode: "auto".into(), value: 1, max: 2 }
    }
}

/// A named way of placing a stack on sites: how many, how grouped, what is tolerated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pattern {
    pub id: String,
    pub name: LText,
    #[serde(default)]
    pub description: Option<LText>,
    /// When to choose it and what it costs you; short lines for the pattern card.
    #[serde(default)]
    pub pros: Vec<LText>,
    #[serde(default)]
    pub cons: Vec<LText>,
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub order: i32,
    #[serde(default = "one")]
    pub min_sites: u32,
    #[serde(default)]
    pub max_sites: Option<u32>,
    #[serde(default = "one")]
    pub default_sites: u32,
    /// Zone type of the working sites (`dc`, `az`, `cloud`).
    #[serde(default)]
    pub site_type: String,
    #[serde(default)]
    pub regions: RegionPolicy,
    #[serde(default)]
    pub racks: RackPolicy,
    /// `failureTolerance` the generated architecture asks for (`rack`, `site`, `geo`).
    #[serde(default)]
    pub tolerance: String,
    #[serde(default)]
    pub site_failures: FailurePolicy,
    /// Add arbiter sites when the working sites alone cannot keep a majority after the planned losses.
    #[serde(default = "yes")]
    pub witness: bool,
    /// The last `standby_sites` working sites run a pilot-light copy that scales out on a disaster.
    #[serde(default)]
    pub standby_sites: u32,
    /// Replicate synchronously to the nearest copy when it lives in the same disaster domain.
    #[serde(default = "yes")]
    pub sync_within_geo: bool,
    /// Needs an off-site location for the backup (single-site designs add one).
    #[serde(default)]
    pub offsite_backup_site: bool,
    /// Requirement defaults of the pattern (RTO, RPO, SLO…); the request can override them.
    #[serde(default)]
    pub requirements: Requirements,
    /// Stack placed by default.
    #[serde(default)]
    pub stack: String,
}

/// Where a layer lives.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayerScope {
    /// No zone: users, anycast DNS, CDN.
    Global,
    /// One node per placement slot (site, or rack when racks are modelled), sized from the load.
    #[default]
    Site,
    /// One logical service spread over the slots by the behavior of its kind (replicas, voters, brokers).
    Cluster,
    /// One node in one place (`place`).
    Single,
}

/// How the nodes of two layers are connected.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Wire {
    /// Every source node to the nearest target node.
    #[default]
    Same,
    /// Every source node to every target node.
    All,
    /// Every target node from the nearest source node.
    EachTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerLink {
    /// Layer the edge starts at.
    pub layer: String,
    #[serde(default, rename = "type")]
    pub edge_type: Option<String>,
    #[serde(default)]
    pub wire: Wire,
    #[serde(default)]
    pub props: Props,
    #[serde(default)]
    pub label: Option<LText>,
    /// The link is only drawn when this layer is *not* placed (a fallback path: with the WAF switched off
    /// the balancers are fed straight from the DNS layer).
    #[serde(default)]
    pub unless: Option<String>,
}

/// How a layer's nodes are sized.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Sizing {
    /// `fixed` (the replicas of `props`) or `load` (enough instances for the declared peak load).
    pub mode: String,
    /// Share of the peak requests this layer sees (1 = all of them).
    pub share: f64,
    pub min_replicas: u32,
    /// Autoscaler bounds as multiples of the computed replicas (0 = leave them alone).
    pub hpa_min_ratio: f64,
    pub hpa_max_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CopyPolicy {
    /// Fewest data copies (primary + replicas) regardless of the planned losses.
    pub min: u32,
    /// Most data copies.
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    pub name: LText,
    /// Catalog kind placed by default.
    pub kind: String,
    /// Other kinds the user may pick for this layer (same role, different product).
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// Catalog function of the layer (`rdbms`, `cache`…): every product of it may be picked, not only the listed alternatives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    #[serde(default)]
    pub scope: LayerScope,
    /// Offered in the wizard as a switch; mandatory layers are always placed.
    #[serde(default)]
    pub optional: bool,
    /// Placed unless the user switches it off (only for optional layers).
    #[serde(default = "yes")]
    pub default: bool,
    /// Layers that must be placed for this one to make sense (switched on with it).
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub props: Props,
    #[serde(default)]
    pub sizing: Sizing,
    /// Data copies for primary/replica layers.
    #[serde(default)]
    pub copies: CopyPolicy,
    /// `first`, `last`, `witness` or `offsite` for `single` layers.
    #[serde(default)]
    pub place: Option<String>,
    /// Edges that end at this layer.
    #[serde(default)]
    pub from: Vec<LayerLink>,
    /// Layer that provides the quorum authority of this one (becomes the `dcs` prop).
    #[serde(default)]
    pub dcs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stack {
    pub id: String,
    pub name: LText,
    #[serde(default)]
    pub description: Option<LText>,
    #[serde(default)]
    pub order: i32,
    pub layers: Vec<Layer>,
}

/// Latency/bandwidth assumptions of the links between places; they end up as edge props the architect can correct.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkProfile {
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub paths: u32,
    /// Replication lag assumed for asynchronous links.
    pub lag_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlacementSettings {
    /// The pattern the wizard offers first (the id of an enabled pattern); empty or unknown: the first one.
    pub default_pattern: String,
    /// Between racks of one site, between sites of one disaster domain, between domains.
    pub intra_site: LinkProfile,
    pub intra_geo: LinkProfile,
    pub inter_geo: LinkProfile,
    /// Names of generated zones (`{n}` is replaced by the number): site type → name.
    pub site_names: BTreeMap<String, LText>,
    pub region_name: LText,
    pub witness_name: LText,
    pub rack_name: LText,
    /// Rack label when there is only one working site (the site name would repeat in every node name).
    pub rack_short_name: LText,
    pub backup_site_name: LText,
    /// Geo label prefix of generated disaster domains.
    pub geo_prefix: String,
    pub witness_site_type: String,
    pub region_zone_type: String,
    pub rack_zone_type: String,
    /// Fewest voters of a consensus cluster and the most the generator ever places.
    pub min_voters: u32,
    pub max_voters: u32,
    /// Instances of a witness node and its resources.
    pub witness_props: Props,
    /// Pilot-light settings of standby sites.
    pub standby_scale_up_seconds: f64,
    pub standby_min_replicas: u32,
    /// Most nodes the generator may produce (a guard; the project limit applies as well).
    pub max_nodes: usize,
    /// Edges between sites are encrypted (`tls: true`) unless the stack says otherwise.
    pub tls_between_sites: bool,
}

impl Default for PlacementSettings {
    fn default() -> Self {
        Self {
            default_pattern: String::new(),
            intra_site: LinkProfile { latency_ms: 0.3, bandwidth_mbps: 25000.0, paths: 2, lag_seconds: 1.0 },
            intra_geo: LinkProfile { latency_ms: 1.2, bandwidth_mbps: 25000.0, paths: 2, lag_seconds: 1.0 },
            inter_geo: LinkProfile { latency_ms: 28.0, bandwidth_mbps: 2000.0, paths: 2, lag_seconds: 5.0 },
            site_names: BTreeMap::new(),
            region_name: LText::default(),
            witness_name: LText::default(),
            rack_name: LText::default(),
            rack_short_name: LText::default(),
            backup_site_name: LText::default(),
            geo_prefix: "geo".into(),
            witness_site_type: "cloud".into(),
            region_zone_type: "region".into(),
            rack_zone_type: "rack".into(),
            min_voters: 3,
            max_voters: 7,
            witness_props: Props::new(),
            standby_scale_up_seconds: 600.0,
            standby_min_replicas: 2,
            max_nodes: 500,
            tls_between_sites: true,
        }
    }
}

/// Everything the generator needs besides the engine configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementConfig {
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    #[serde(default)]
    pub stacks: Vec<Stack>,
    #[serde(default)]
    pub settings: PlacementSettings,
}

impl PlacementConfig {
    pub fn pattern(&self, id: &str) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }
    pub fn stack(&self, id: &str) -> Option<&Stack> {
        self.stacks.iter().find(|s| s.id == id)
    }
}

/// What the wizard sends.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PlacementRequest {
    pub pattern: String,
    /// Working sites (data centers, availability zones, cloud regions).
    pub sites: Option<u32>,
    /// Disaster domains the sites are split into (patterns with `regions.mode = count`).
    pub regions: Option<u32>,
    /// Racks per site; more than one makes the racks failure domains of their own.
    pub racks: Option<u32>,
    /// Sites that may fail at the same time.
    pub site_failures: Option<u32>,
    /// Stack to place (the pattern's by default).
    pub stack: Option<String>,
    /// Optional layers to place; `None` = the stack's defaults.
    pub layers: Option<Vec<String>>,
    /// Layer id → catalog kind (one of its alternatives or of its function).
    pub kinds: BTreeMap<String, String>,
    /// Layer id → variant of the layer's product (an HA topology, a managed offering); the product's default when absent.
    #[serde(default)]
    pub variants: BTreeMap<String, String>,
    /// Requirement values that replace the pattern's.
    pub requirements: Option<Requirements>,
    /// Zone type of the working sites (`dc`, `az`, `cloud`) instead of the pattern's.
    pub site_type: Option<String>,
    /// Language of generated names.
    pub lang: Option<String>,
    /// Name of the architecture.
    pub name: Option<String>,
    /// Project limit on nodes; the generator refuses to exceed it.
    pub max_nodes: Option<usize>,
}

/// A fact about the generated design the UI explains in the user's language (`code` is translated there).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub code: String,
    /// `info`, `warn` or `error`.
    pub level: String,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub vars: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedSite {
    pub id: String,
    pub name: String,
    /// `working`, `standby` or `witness`.
    pub role: String,
    pub geo: String,
    pub racks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementResult {
    pub architecture: crate::model::Architecture,
    pub sites: Vec<PlacedSite>,
    /// Sites that may fail at once, as designed for.
    pub site_failures: u32,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for PlacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for PlacementError {}

impl PlacementError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}
