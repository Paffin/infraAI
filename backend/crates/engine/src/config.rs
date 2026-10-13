//! Engine configuration: rules, assumptions, scoring, pricing, reference templates.
//! Everything here is data – loaded from the database (seeded from `defaults/*.json`) and
//! editable in the admin panel. The engine never contains a magic threshold of its own.

use crate::catalog::Catalog;
use crate::model::Architecture;
use crate::text::LText;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

// -------------------------------------------------------------------------------------------
// Rules
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Risk,
    Critical,
}

/// The implementation a rule is bound to. A rule is *configuration* (id, texts, severity,
/// params) on top of one of these generic, well-tested check implementations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CheckType {
    Spof,
    SurviveSite,
    SurviveSites,
    VersionLifecycle,
    VersionSkew,
    Rto,
    Rpo,
    GeoSurvive,
    GeoSpread,
    PartitionSafety,
    MinInstances,
    SpreadSites,
    QuorumVoters,
    WitnessIndependent,
    ReplicationPolicy,
    FailoverAuthority,
    BackupExists,
    BackupOffsite,
    Prop,
    Capacity,
    CapacityUnderFailure,
    CostBudget,
    /// The expected availability (`reliability`) against the availability target of the requirements.
    AvailabilitySlo,
    PathContains,
    ForbiddenEdge,
    NoCycles,
    RequiredKind,
    CacheResilience,
    GroupHygiene,
    RoleRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub category: String,
    pub check: CheckType,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Status reported when the check is violated.
    #[serde(default = "default_severity")]
    pub severity: Severity,
    /// Override of the scoring weight for this rule (otherwise by status).
    #[serde(default)]
    pub weight: Option<f64>,
    #[serde(default)]
    pub params: Value,
    pub title: LText,
    /// Message per violation; placeholders come from the check (`{node}`, `{zone}`, …).
    pub message: LText,
    /// Message for `no_data` findings (premises declared by the architect cannot be verified).
    #[serde(default)]
    pub no_data_message: Option<LText>,
    /// Message for `no_data` findings caused by a missing *requirement* (finding key `req`).
    #[serde(default)]
    pub requirement_message: Option<LText>,
    /// Message for findings whose data is lost for good (finding key ends with `:lost`).
    #[serde(default)]
    pub data_lost_message: Option<LText>,
    /// Messages for findings of one kind of problem: the check sets the finding var `variant` and the matching text
    /// replaces `message` (a version past its end of life reads differently from one that is about to be).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variant_messages: BTreeMap<String, LText>,
    /// Shown when the rule passes.
    #[serde(default)]
    pub ok: LText,
    /// Why this matters (best-practice rationale).
    #[serde(default)]
    pub rationale: Option<LText>,
    #[serde(default)]
    pub remediation: Option<LText>,
    /// fix kind -> label template, e.g. `add_replica` -> "Добавить реплику в {zone}".
    #[serde(default)]
    pub fix_labels: BTreeMap<String, LText>,
    #[serde(default)]
    pub references: Vec<String>,
}

fn yes() -> bool {
    true
}
fn default_severity() -> Severity {
    Severity::Risk
}

// -------------------------------------------------------------------------------------------
// Engine settings (assumptions, scoring, scenario budget, price book)
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Assumptions {
    /// Replication lag assumed for async links without a declared `lagSeconds`.
    pub async_lag_seconds: f64,
    /// Time for a human to perform a manual failover.
    pub manual_failover_seconds: f64,
    /// Time to restore a stateful service from backup after a destructive failure.
    pub restore_seconds: f64,
    /// Interval of backup edges without a declared `intervalSeconds`.
    pub backup_interval_seconds: f64,
    pub default_failover_seconds: f64,
    pub default_election_seconds: f64,
    pub default_health_check_seconds: f64,
    pub default_fail_threshold: f64,
    /// Fraction of peak capacity that must remain during a failure (0.8 = 80%).
    pub min_capacity_during_failure: f64,
    /// Capacity headroom above peak (+growth) required in normal operation, percent.
    pub headroom_percent: f64,
    /// Maximum acceptable RTT for synchronous replication, ms.
    pub sync_max_rtt_ms: f64,
    /// Failure tolerance used when requirements do not specify one.
    pub default_failure_tolerance: String,
    /// Sites that host members of one unit are assumed to be connected by the unit's own
    /// network (full mesh) even if no edge is drawn between them. A `link` scenario still
    /// cuts that mesh. Disable to require explicit edges.
    pub implicit_mesh: bool,
    /// If true, sites can reach each other through a third site (transit routing), so a single
    /// cut link never partitions anything while a path via another site exists. Default false:
    /// the conservative, pairwise model.
    pub transit_routing: bool,
    /// Scale-out time for nodes with `failoverReplicas` when `scaleUpSeconds` is not declared.
    pub default_scale_up_seconds: f64,
}

impl Default for Assumptions {
    fn default() -> Self {
        Self {
            async_lag_seconds: 5.0,
            manual_failover_seconds: 1800.0,
            restore_seconds: 14400.0,
            backup_interval_seconds: 86400.0,
            default_failover_seconds: 30.0,
            default_election_seconds: 10.0,
            default_health_check_seconds: 10.0,
            default_fail_threshold: 3.0,
            min_capacity_during_failure: 0.8,
            headroom_percent: 30.0,
            sync_max_rtt_ms: 5.0,
            default_failure_tolerance: "site".into(),
            implicit_mesh: true,
            transit_routing: false,
            default_scale_up_seconds: 300.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Scoring {
    pub critical: f64,
    pub risk: f64,
    pub no_data: f64,
    /// Upper bound of the total penalty coming from `no_data` rules.
    pub no_data_cap: f64,
}

impl Default for Scoring {
    fn default() -> Self {
        Self { critical: 15.0, risk: 4.0, no_data: 1.5, no_data_cap: 12.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScenarioSettings {
    /// Hard cap on simulated scenarios per evaluation (combinatorial explosion guard).
    pub max_count: usize,
    /// Simultaneous loss of two sites (kept for the standard scenario list; the `survive_sites` check
    /// uses `maxSiteFailures`).
    pub include_pairs: bool,
    pub include_instances: bool,
    pub include_nodes: bool,
    /// Racks (zones below a site) as failure scenarios of their own.
    pub include_racks: bool,
    /// Sites that lose the network but keep running ("island"): the failure that makes the minority
    /// side of a quorum stop, and the one a single cut link never shows with three or more sites.
    pub include_isolation: bool,
    /// Largest group of sites isolated together (1 = every site alone, 2 = also every pair, …).
    pub isolation_max_sites: usize,
    /// Upper bound of the `siteFailures` requirement the engine accepts (bigger values are clamped).
    pub max_site_failures: usize,
    /// Largest number of site combinations simulated for one `siteFailures` value. Past it the
    /// combinations are sampled evenly and the check says so.
    pub max_combinations: usize,
}

impl Default for ScenarioSettings {
    fn default() -> Self {
        Self {
            max_count: 400,
            include_pairs: true,
            include_instances: true,
            include_nodes: true,
            include_racks: true,
            include_isolation: true,
            isolation_max_sites: 2,
            max_site_failures: 4,
            max_combinations: 120,
        }
    }
}

/// One level of the failure the architecture must survive (`requirements.failureTolerance`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ToleranceLevel {
    pub id: String,
    /// Tolerating a level means tolerating every level with a lower rank.
    pub rank: u32,
    /// Failure-domain level whose loss the level covers. `None` = lost instances only.
    pub domain: Option<crate::catalog::ZoneLevel>,
    pub name: LText,
    pub help: LText,
}

/// The ladder of failures a requirement can ask for. Data, not code: a company that works with
/// "rooms" or "halls" adds a level without touching the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToleranceSettings {
    pub levels: Vec<ToleranceLevel>,
}

impl Default for ToleranceSettings {
    fn default() -> Self {
        use crate::catalog::ZoneLevel;
        let lvl = |id: &str, rank: u32, domain: Option<ZoneLevel>| ToleranceLevel { id: id.into(), rank, domain, ..Default::default() };
        Self { levels: vec![lvl("instance", 0, None), lvl("rack", 1, Some(ZoneLevel::Rack)), lvl("site", 2, Some(ZoneLevel::Site)), lvl("geo", 3, Some(ZoneLevel::Geo))] }
    }
}

impl ToleranceSettings {
    pub fn level(&self, id: &str) -> Option<&ToleranceLevel> {
        self.levels.iter().find(|l| l.id == id)
    }
    /// Rank of `id`; an unknown id is treated like the lowest level that covers sites.
    pub fn rank(&self, id: &str) -> u32 {
        match self.level(id) {
            Some(l) => l.rank,
            None => self.levels.iter().find(|l| l.domain == Some(crate::catalog::ZoneLevel::Site)).map(|l| l.rank).unwrap_or(0),
        }
    }
    /// Does tolerating `id` include surviving the loss of a failure domain of `level`?
    pub fn covers(&self, id: &str, level: crate::catalog::ZoneLevel) -> bool {
        let rank = self.rank(id);
        self.levels.iter().any(|l| l.domain == Some(level) && l.rank <= rank)
    }
    /// Rank of the level that covers `level`; `None` if no level does.
    pub fn rank_of_domain(&self, level: crate::catalog::ZoneLevel) -> Option<u32> {
        self.levels.iter().filter(|l| l.domain == Some(level)).map(|l| l.rank).min()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PriceModel {
    pub vcpu_month: f64,
    pub ram_gb_month: f64,
    /// storage class -> price per GB per month
    pub disk_gb_month: BTreeMap<String, f64>,
    pub default_disk_class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CostSettings {
    pub currency: String,
    pub symbol: String,
    pub default_model: String,
    pub models: BTreeMap<String, PriceModel>,
    /// Utilisation of the budget (percent) from which the rule reports `risk`.
    pub warn_percent: f64,
}

impl Default for CostSettings {
    fn default() -> Self {
        Self { currency: "RUB".into(), symbol: "₽".into(), default_model: "onprem".into(), models: BTreeMap::new(), warn_percent: 90.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportSettings {
    pub terraform_required_version: String,
    /// Body of the `terraform { … }` backend block (remote state belongs here, not in the code).
    /// `{arch}`, `{cluster}` and `{stage}` are replaced so every state gets its own key.
    pub terraform_backend: String,
    /// Terraform provider requirements: name -> `{source, version}`.
    pub terraform_providers: BTreeMap<String, ProviderReq>,
    pub adr_status: String,
    /// Guard rails of the template language (a runaway template must not stall an export).
    pub template_limits: TemplateLimits,
    /// Kubernetes-native generators (manifests, Helm, Terraform on top of both).
    pub k8s: K8sSettings,
    /// lang -> key -> text for generated documents (ADR, runbook, IaC report).
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
}

/// Limits of one template evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TemplateLimits {
    /// Instruction budget of one expression.
    pub fuel: u64,
    /// Largest list one `$each` may walk.
    pub max_items: usize,
}

impl Default for TemplateLimits {
    fn default() -> Self {
        Self { fuel: 50_000, max_items: 5_000 }
    }
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            terraform_required_version: ">= 1.6.0".into(),
            terraform_backend: "backend \"local\" {\n    path = \"terraform.tfstate\"\n  }".into(),
            terraform_providers: BTreeMap::new(),
            adr_status: "Proposed".into(),
            template_limits: TemplateLimits::default(),
            k8s: K8sSettings::default(),
            texts: BTreeMap::new(),
        }
    }
}

impl ExportSettings {
    /// Text for `key` in `lang` (falls back to `en`, then to the key itself).
    pub fn text(&self, lang: &str, key: &str) -> String {
        self.texts.get(lang).and_then(|m| m.get(key)).or_else(|| self.texts.get("en").and_then(|m| m.get(key))).cloned().unwrap_or_else(|| key.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderReq {
    pub source: String,
    pub version: String,
}

/// Where and how workloads are placed on Kubernetes. All of it is data: change it in the admin
/// panel (Engine settings) to match your platform conventions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct K8sSettings {
    /// Catalog trait that marks a kind as a Kubernetes cluster (workloads are placed on it).
    pub cluster_trait: String,
    pub default_namespace: String,
    /// Namespace that holds the Helm release records of the helper releases (namespaces, workloads).
    pub release_namespace: String,
    /// Catalog category -> namespace (e.g. `data` -> `data`).
    pub namespaces: BTreeMap<String, String>,
    pub create_namespaces: bool,
    /// Node label that carries the failure domain (used for spread/anti-affinity).
    pub zone_label: String,
    /// Node label that identifies a single host (spread inside a one-zone site).
    pub host_label: String,
    /// DNS suffix of cross-cluster services (Multi-Cluster Services API: `clusterset.local`).
    pub cluster_set_domain: String,
    /// Labels added to every generated object (values may use `{{ }}` placeholders).
    pub labels: BTreeMap<String, String>,
    /// Value of the `storageClass` property -> Kubernetes StorageClass name (empty = cluster default).
    pub storage_classes: BTreeMap<String, String>,
    /// Prerequisites (operators, controllers) that templates can `require`.
    pub operators: BTreeMap<String, Operator>,
}

impl Default for K8sSettings {
    fn default() -> Self {
        Self {
            cluster_trait: "k8s-cluster".into(),
            default_namespace: "apps".into(),
            release_namespace: "default".into(),
            namespaces: BTreeMap::new(),
            create_namespaces: true,
            zone_label: "topology.kubernetes.io/zone".into(),
            host_label: "kubernetes.io/hostname".into(),
            cluster_set_domain: "clusterset.local".into(),
            labels: BTreeMap::new(),
            storage_classes: BTreeMap::new(),
            operators: BTreeMap::new(),
        }
    }
}

/// A Helm chart that must be installed before the objects of a template can be applied.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Operator {
    pub title: LText,
    pub release: String,
    pub namespace: String,
    pub chart: String,
    pub repo: String,
    /// Exact chart version – pinned on purpose, upgrades are a conscious act.
    pub version: String,
    pub values: Value,
    /// Operators that must be installed first.
    pub after: Vec<String>,
}

/// Who holds the quorum that lets a replica take over, told by the traits of the members of a unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AuthoritySettings {
    /// The members vote among themselves: a majority is needed to elect or keep a primary (MongoDB replica set,
    /// Redis Cluster, Group Replication, Galera…). No separate DCS or witness is required, but the votes must survive
    /// the failure domains like any other quorum.
    pub self_quorum_traits: Vec<String>,
    /// The provider or the platform holds the quorum and fences the old primary (a managed service, an operator on
    /// the Kubernetes API): the model sees no split brain, and the node does not need a DCS.
    pub provider_traits: Vec<String>,
}

impl Default for AuthoritySettings {
    fn default() -> Self {
        Self { self_quorum_traits: vec!["self-quorum".into()], provider_traits: vec!["managed".into(), "platform-quorum".into()] }
    }
}

/// How the lifecycle tables are read (`catalog.versions`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LifecycleSettings {
    /// A version whose security fixes end within this many days is a risk already (a rule may override it).
    pub warn_days: i64,
    /// A release line must be out this long to be recommended: brand-new `x.0` releases are not.
    pub mature_days: i64,
    /// Recommend long-term-support lines first.
    pub prefer_lts: bool,
    /// A release table older than this is reported: dates that may have moved since are no basis for a verdict.
    pub stale_days: i64,
}

impl Default for LifecycleSettings {
    fn default() -> Self {
        Self { warn_days: 180, mature_days: 90, prefer_lts: true, stale_days: 120 }
    }
}

/// How often things fail, by what: events per year. A rate is per instance for `instance`, per service for `unit`
/// (all of its instances at once), per failure domain for the zones, per pair of sites for `link`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FailureRates {
    pub instance: f64,
    /// A whole service at once: a bad release, an expired certificate, a configuration slip. Redundancy does not help.
    pub unit: f64,
    /// Failure domains by zone type (`dc`, `cloud`, `az`, `region`, `rack`…), then by level (`rack`, `site`, `geo`).
    pub zone: BTreeMap<String, f64>,
    /// All paths between two sites down.
    pub link: f64,
    /// A site (or the group of sites that shares a carrier) loses the network but keeps running.
    pub isolation: f64,
    /// A disaster that takes every site of one disaster domain.
    pub geo: f64,
}

impl Default for FailureRates {
    fn default() -> Self {
        Self { instance: 2.0, unit: 0.25, zone: BTreeMap::new(), link: 0.5, isolation: 0.5, geo: 0.01 }
    }
}

/// How long a failed thing stays failed when nothing recovers it automatically, seconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepairTimes {
    pub instance: f64,
    /// A bad release is rolled back, a certificate renewed.
    pub unit: f64,
    pub zone: BTreeMap<String, f64>,
    pub link: f64,
    pub isolation: f64,
    pub geo: f64,
}

impl Default for RepairTimes {
    fn default() -> Self {
        Self { instance: 14_400.0, unit: 3_600.0, zone: BTreeMap::new(), link: 14_400.0, isolation: 7_200.0, geo: 259_200.0 }
    }
}

/// What a provider's SLA is worth when the catalog does not name it for a product.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderSla {
    /// Traits that mark a service as run by a provider.
    pub traits: Vec<String>,
    /// Availability of a provider-run service by product function (`rdbms`, `cache`, `l7`…).
    pub by_function: BTreeMap<String, f64>,
    /// Availability when the function is not listed.
    pub default: f64,
    /// An SLA is a share of the year; the engine turns it into incidents of this length (seconds), so that what
    /// the architecture does about an outage (a second region, a standby) can act on it.
    pub incident_seconds: f64,
}

impl Default for ProviderSla {
    fn default() -> Self {
        Self { traits: vec!["managed".into()], by_function: BTreeMap::new(), default: 0.999, incident_seconds: 1_800.0 }
    }
}

/// The estimate of availability: what fails, how often and for how long (`docs/AVAILABILITY.md`). All of it is an
/// assumption the architect can correct; the estimate says so.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default = "ReliabilitySettings::bare")]
pub struct ReliabilitySettings {
    pub enabled: bool,
    /// Days in the year the downtime is counted over.
    pub days_per_year: f64,
    pub failures: FailureRates,
    pub repair: RepairTimes,
    /// Multiplier of the failure rate of a data center by the value of its `tier` property (`I`…`IV`): a Tier I site is
    /// lost far more often than a Tier IV one.
    pub tier_factor: BTreeMap<String, f64>,
    /// Sites isolated together (a shared carrier) fail together this much more rarely than one site alone.
    pub isolation_group_factor: f64,
    pub provider: ProviderSla,
    /// Count the capacity a failure takes away as unavailability of the requests that no longer fit.
    pub partial_capacity: bool,
    /// Share of the downtime budget (percent) from which meeting the target is only a risk.
    pub warn_percent: f64,
    /// How many of the costliest failures the report lists.
    pub top: usize,
    /// Availability targets (percent) the editor offers to pick from: the usual classes of nines.
    pub presets: Vec<f64>,
}

impl ReliabilitySettings {
    /// The plain numbers without the tables (zone types, tiers, products): what a key that is missing from a stored
    /// configuration falls back to.
    pub fn bare() -> Self {
        Self {
            enabled: true,
            days_per_year: 365.25,
            failures: FailureRates::default(),
            repair: RepairTimes::default(),
            tier_factor: BTreeMap::new(),
            isolation_group_factor: 0.1,
            provider: ProviderSla::default(),
            partial_capacity: true,
            warn_percent: 80.0,
            top: 8,
            presets: vec![99.0, 99.5, 99.9, 99.95, 99.99],
        }
    }
}

impl Default for ReliabilitySettings {
    /// What ships with the engine (`defaults/engine.json`): a configuration stored before the estimate existed has no
    /// `reliability` block, and it must get the rates and tables that ship, not an empty set that would quietly leave
    /// every site, rack and provider out of the estimate.
    fn default() -> Self {
        serde_json::from_str::<serde_json::Value>(crate::defaults::ENGINE_JSON).ok().and_then(|v| v.get("reliability").cloned()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_else(Self::bare)
    }
}

/// "What to improve first": the engine's fixes tried one by one and ranked by the effect they measurably have (`docs/RECOMMENDATIONS.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecommendSettings {
    pub enabled: bool,
    /// How many fixes are tried; each costs one more evaluation.
    pub max_candidates: usize,
    /// How many recommendations the editor lists before "show more".
    pub show: usize,
    /// Kinds of fixes that may be recommended (`add_member`, `set_replicas`, `add_backup`…); empty = all of them.
    pub kinds: Vec<String>,
}

impl Default for RecommendSettings {
    fn default() -> Self {
        Self { enabled: true, max_candidates: 16, show: 4, kinds: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EngineSettings {
    pub assumptions: Assumptions,
    pub scoring: Scoring,
    pub scenarios: ScenarioSettings,
    pub tolerance: ToleranceSettings,
    pub authority: AuthoritySettings,
    pub lifecycle: LifecycleSettings,
    pub reliability: ReliabilitySettings,
    pub recommendations: RecommendSettings,
    pub cost: CostSettings,
    pub export: ExportSettings,
}

/// Everything the engine needs for one evaluation.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub catalog: Catalog,
    pub rules: Vec<Rule>,
    pub settings: EngineSettings,
    /// IaC generators per catalog kind.
    pub iac: Vec<IacTemplate>,
    /// Placement patterns and stacks (how a platform is laid out on one site, two, ten…).
    pub placement: crate::placement::PlacementConfig,
    /// Release tables of the products in the catalog (`catalog.versions`).
    pub versions: Vec<crate::lifecycle::VersionSet>,
    /// Today's date (`YYYY-MM-DD`). The engine reads no clock: whoever runs it says what day it is; a function that
    /// returns nothing switches the lifecycle checks off.
    pub today: fn() -> String,
    /// Language used for names of nodes generated by deterministic fixes (`ru`, `en`, …).
    pub lang: String,
}

// -------------------------------------------------------------------------------------------
// Reference architectures
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: String,
    pub name: LText,
    #[serde(default)]
    pub description: Option<LText>,
    /// Short badge shown next to the name (`DR`, `HA`).
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub order: i32,
    pub architecture: Architecture,
}

/// Accepted risk. Matching findings no longer count against the score but remain visible.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Waiver {
    pub fingerprint: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub at: Option<String>,
}

// -------------------------------------------------------------------------------------------
// IaC templates
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum IacTarget {
    /// Deployed onto a Kubernetes cluster of the architecture.
    #[default]
    Kubernetes,
    /// No generator: the component is provisioned some other way (reason is reported).
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum IacGranularity {
    /// One workload per logical service (group) and cluster – databases, queues.
    #[default]
    Unit,
    /// One workload per node.
    Node,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IacRequire {
    Id(String),
    When { id: String, when: Option<String> },
}

impl IacRequire {
    pub fn id(&self) -> &str {
        match self {
            IacRequire::Id(i) => i,
            IacRequire::When { id, .. } => id,
        }
    }
    pub fn when(&self) -> Option<&str> {
        match self {
            IacRequire::Id(_) => None,
            IacRequire::When { when, .. } => when.as_deref(),
        }
    }
}

/// A condition under which the template refuses to generate, with the (localised) reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IacRefusal {
    #[serde(default)]
    pub when: Option<String>,
    pub text: LText,
}

/// An input without which nothing sensible can be generated (e.g. the application image).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IacNeed {
    pub prop: String,
    #[serde(default)]
    pub hint: Option<LText>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IacVar {
    pub name: String,
    pub expr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IacObject {
    /// Short id used in file names (`cluster`, `scheduledbackup`).
    pub id: String,
    #[serde(default)]
    pub when: Option<String>,
    /// Kubernetes object as a template (see the DSL in `iac::dsl`).
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IacRelease {
    pub id: String,
    #[serde(default)]
    pub when: Option<String>,
    pub name: String,
    pub namespace: String,
    pub chart: String,
    pub repo: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub values: Value,
}

/// A Secret the operator of the cluster must create – secrets are never generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IacSecret {
    #[serde(default)]
    pub when: Option<String>,
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub hint: Option<LText>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IacNote {
    #[serde(default)]
    pub when: Option<String>,
    /// `info` or `warn`.
    #[serde(default)]
    pub level: String,
    pub text: LText,
}

/// How one catalog kind is turned into infrastructure code. Stored as a config item (kind `iac`,
/// id = catalog kind id) and editable in the admin panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IacTemplate {
    pub id: String,
    #[serde(default)]
    pub title: Option<LText>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub target: IacTarget,
    /// Why there is no generator (shown in the export report).
    #[serde(default)]
    pub reason: Option<LText>,
    #[serde(default)]
    pub granularity: IacGranularity,
    /// Ids of `export.k8s.operators` that must be installed first (optionally only `when` an
    /// expression holds, e.g. the backup plugin only if backups are wired).
    #[serde(default)]
    pub requires: Vec<IacRequire>,
    /// Situations this template cannot express faithfully; the workload is reported, not faked.
    #[serde(default)]
    pub unsupported: Vec<IacRefusal>,
    #[serde(default)]
    pub needs: Vec<IacNeed>,
    /// Named expressions evaluated in order and added to the context.
    #[serde(default)]
    pub vars: Vec<IacVar>,
    #[serde(default)]
    pub objects: Vec<IacObject>,
    #[serde(default)]
    pub releases: Vec<IacRelease>,
    #[serde(default)]
    pub secrets: Vec<IacSecret>,
    #[serde(default)]
    pub notes: Vec<IacNote>,
}
