//! The component catalog: kinds, edge types, zone types. Entirely data-driven – the admin panel
//! edits it, defaults ship as JSON (`defaults/catalog.json`). Rules never mention concrete kinds
//! directly; they reference **traits** (`datastore`, `workload`, `balancer`, …) and
//! **behaviors** (how a component fails over), so a custom "Cassandra" kind with the right
//! traits is covered by all existing rules without touching code.

use crate::model::{Props, PropsExt};
use crate::text::{interpolate, LText};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub zone_types: Vec<ZoneTypeDef>,
    #[serde(default)]
    pub edge_types: Vec<EdgeTypeDef>,
    /// Properties every kind has (replicas, vcpu, ramGb, …).
    #[serde(default)]
    pub common_props: Vec<PropDef>,
    #[serde(default)]
    pub kinds: Vec<KindDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: LText,
    #[serde(default)]
    pub order: i32,
    /// Icon of the category (a name from the icon set of the UI, `network`, `database`…); the cards of a kind without an icon of its own wear it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// What the products of the category are for, in the order the library shows them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<FunctionDef>,
}

/// A job that several products can do (`rdbms`, `cache`, `l7`…): the library groups products by it, and a placement
/// layer offers every product of the function as an alternative.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDef {
    pub id: String,
    pub name: LText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LText>,
}

/// How a component fails over / what "serving" means. Drives the simulator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum Behavior {
    /// Any live member serves (web tiers, balancers, object storage, monitoring…).
    #[default]
    Stateless,
    /// One writer + replicas, promotion on failure (PostgreSQL, Redis, Vault DR…).
    PrimaryReplica,
    /// Majority-of-votes consensus (etcd, ZooKeeper, Sentinel-like arbiters…).
    Consensus,
    /// Needs `minInsync`/majority of instances alive in one network partition (Kafka, OpenSearch…).
    QuorumLog,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoleDef {
    pub id: String,
    pub name: LText,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CapacityDef {
    /// Requests per second one instance sustains (unless the node overrides `rpsPerReplica`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rps_per_replica: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rps_per_vcpu: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CostDef {
    /// Fixed monthly price per instance on top of resource pricing (licences, managed premium).
    #[serde(default)]
    pub monthly_fixed: f64,
    /// Multiplier on resource pricing (managed service premium, HA overhead…).
    #[serde(default = "one_f")]
    pub multiplier: f64,
}

fn one_f() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunbookDef {
    /// Step lists (supports `{node}`, `{group}` placeholders) shown in the generated runbook.
    #[serde(default)]
    pub failover: Vec<LText>,
    #[serde(default)]
    pub restore: Vec<LText>,
    #[serde(default)]
    pub verify: Vec<LText>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindDef {
    pub id: String,
    pub category: String,
    pub name: LText,
    #[serde(default)]
    pub description: Option<LText>,
    /// 2–4 letter badge: shown where there is no room for an icon (a diagram zoomed far out, the exported SVG, chips).
    pub abbr: String,
    pub color: String,
    /// Icon on the node card (a name from the icon set of the UI: `database`, `shield-check`…); the category's icon when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub behavior: Behavior,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub roles: Vec<RoleDef>,
    #[serde(default)]
    pub props: Vec<PropDef>,
    /// Template for the monospace sub-line of the node card: `"{replicas}× VRRP"`.
    #[serde(default)]
    pub subtitle: Option<LText>,
    #[serde(default)]
    pub capacity: Option<CapacityDef>,
    #[serde(default)]
    pub cost: Option<CostDef>,
    #[serde(default)]
    pub runbook: Option<RunbookDef>,
    /// How often its instances fail (the engine settings say it for every kind that does not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reliability: Option<ReliabilityDef>,
    /// Default size of the card on the canvas is fixed; this lets a kind be hidden from the
    /// library without deleting it from existing projects.
    #[serde(default)]
    pub hidden: bool,
    /// What the product is for (`rdbms`, `cache`, `l7`, …): products of one function replace each other, the library
    /// groups by it and placement stacks offer every product of the function for a layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// The basic library: the products an architect reaches for first. The library lists them above the rest of their function, and the AI
    /// always sees them in full; the rest are found by search and by the function they serve (the AI is told about them in an index).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub core: bool,
    /// Ways to deploy the product (HA topologies, managed offerings). A node picks one with its `variant` prop; the
    /// variant changes how the engine sees the node (behavior, traits, defaults, capacity, cost).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<VariantDef>,
    /// Id of the release table (`catalog.versions`) of this product; the kind id when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versions: Option<String>,
}

/// How often the instances of a product fail and what the provider promises (all of it optional: what is not said comes
/// from the engine settings `reliability`). A variant's values replace the kind's.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReliabilityDef {
    /// Failures per instance per year (hardware, VM, pod, process).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failures_per_year: Option<f64>,
    /// Seconds until a failed instance is repaired or replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_seconds: Option<f64>,
    /// Failures per year of the whole service at once (a bad release, an expired certificate, a bug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_mode_per_year: Option<f64>,
    /// Availability (0..1) the provider guarantees to a service it runs; the service as a whole is down that much.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sla: Option<f64>,
}

impl ReliabilityDef {
    /// `self` with the values of `over` where it has any.
    pub fn over(&self, over: &ReliabilityDef) -> ReliabilityDef {
        ReliabilityDef {
            failures_per_year: over.failures_per_year.or(self.failures_per_year),
            repair_seconds: over.repair_seconds.or(self.repair_seconds),
            common_mode_per_year: over.common_mode_per_year.or(self.common_mode_per_year),
            sla: over.sla.or(self.sla),
        }
    }
}

/// A way to deploy a product: Patroni or plain streaming replication for PostgreSQL, Sentinel or Cluster for Redis,
/// a managed cloud offering… Everything it does not say is inherited from the kind.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VariantDef {
    pub id: String,
    pub name: LText,
    #[serde(default)]
    pub description: Option<LText>,
    /// Assumed when a node declares no variant.
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub behavior: Option<Behavior>,
    /// Roles of a node in this variant when they differ from the kind's (a Raft cluster has voters, not primaries).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleDef>,
    #[serde(default)]
    pub add_traits: Vec<String>,
    #[serde(default)]
    pub remove_traits: Vec<String>,
    /// Prop defaults of this variant (they replace the kind's: automatic failover in 30 s, three voters…).
    #[serde(default)]
    pub defaults: Props,
    #[serde(default)]
    pub capacity: Option<CapacityDef>,
    #[serde(default)]
    pub cost: Option<CostDef>,
    /// Failure rates and the provider's SLA of this way to deploy the product.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reliability: Option<ReliabilityDef>,
    /// Release table of this variant when it differs from the kind's: a managed offering has its own lifecycle
    /// (Amazon RDS supports a PostgreSQL line for a different time than the community does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versions: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PropType {
    #[default]
    Number,
    String,
    Boolean,
    Enum,
    /// Reference to another unit (group) in the architecture – e.g. the DCS of a database.
    UnitRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PropOption {
    pub value: String,
    pub label: LText,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PropDef {
    pub key: String,
    #[serde(rename = "type", default)]
    pub prop_type: PropType,
    pub label: LText,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub options: Vec<PropOption>,
    /// Default value assumed by the catalog. A value that comes from here (and not from the
    /// node itself) is treated as *assumed*, not *declared*.
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub help: Option<LText>,
    /// For `unitRef`: only units with this behavior are offered.
    #[serde(default)]
    pub ref_behavior: Option<Behavior>,
    #[serde(default)]
    pub advanced: bool,
    /// Only for these variants of the kind (empty = every variant).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
    /// The prop holds a version of a product that has a release table (`catalog.versions`): the id of the table, or
    /// `@key` to take it from another prop of the node (`@os`: the value of `os` names the table, so Ubuntu 22.04
    /// and RHEL 9 are judged by their own dates). The node's own `version` needs no such mark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Semantics {
    /// Request path: the source needs the target to serve (unless `hard=false`).
    Dependency,
    /// Data replication primary→replica. `mode` = `sync` | `async`.
    Replication,
    /// Consensus membership / quorum link (visual + latency checks; connectivity).
    Membership,
    /// Backup stream source→backup target.
    Backup,
    /// Management plane (GitOps, monitoring scrapes, secrets): not on the availability path.
    Control,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeStyle {
    pub color: String,
    #[serde(default)]
    pub dash: Option<String>,
    #[serde(default)]
    pub width: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeTypeDef {
    pub id: String,
    pub name: LText,
    /// Short caption for the canvas legend (`sync`, `кворум`); falls back to `name`.
    #[serde(default)]
    pub short: Option<LText>,
    /// What a link of this type means, in a sentence: the legend of the canvas says it next to the way the link is drawn. Without it the
    /// legend says what the semantics mean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LText>,
    pub semantics: Semantics,
    pub style: EdgeStyle,
    #[serde(default = "yes")]
    pub directed: bool,
    /// Default props merged into every edge of this type (e.g. `mode: sync`).
    #[serde(default)]
    pub default_props: Props,
    #[serde(default)]
    pub props: Vec<PropDef>,
    /// `network` | `data` | `dr` – used by the layer filter on the canvas.
    #[serde(default)]
    pub layer: Option<String>,
}

fn yes() -> bool {
    true
}

/// Failure-domain level of a zone type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ZoneLevel {
    /// Visual grouping only; not a failure domain.
    Group,
    Rack,
    /// Unit of power/network failure and of network partition (DC, AZ, cloud region site).
    Site,
    /// Disaster domain: a metro/region where all contained sites may fail together.
    Geo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneTypeDef {
    pub id: String,
    pub name: LText,
    pub level: ZoneLevel,
    pub color: String,
    /// Icon in the header of the zone (a name from the icon set of the UI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub props: Vec<PropDef>,
}

impl Catalog {
    pub fn kind(&self, id: &str) -> Option<&KindDef> {
        self.kinds.iter().find(|k| k.id == id)
    }
    /// The products that do one job (`rdbms`), hidden ones left out.
    pub fn kinds_of_function(&self, function: &str) -> Vec<&KindDef> {
        self.kinds.iter().filter(|k| !k.hidden && k.function.as_deref() == Some(function)).collect()
    }
    pub fn edge_type(&self, id: &str) -> Option<&EdgeTypeDef> {
        self.edge_types.iter().find(|t| t.id == id)
    }
    pub fn zone_type(&self, id: &str) -> Option<&ZoneTypeDef> {
        self.zone_types.iter().find(|t| t.id == id)
    }

    /// The variant a node of `kind` runs: the one it names, else the kind's default variant; `None` for a kind
    /// without variants (or without a default one).
    pub fn variant(&self, kind: &str, id: Option<&str>) -> Option<&VariantDef> {
        let k = self.kind(kind)?;
        id.filter(|i| !i.is_empty()).and_then(|i| k.variants.iter().find(|v| v.id == i)).or_else(|| k.variants.iter().find(|v| v.default))
    }

    /// Catalog-level default props for a kind: common props ← kind props (← the variant's defaults).
    pub fn default_props(&self, kind: &str) -> Props {
        self.default_props_for(kind, None)
    }

    pub fn default_props_for(&self, kind: &str, variant: Option<&VariantDef>) -> Props {
        let mut out = Props::new();
        for p in &self.common_props {
            if let Some(d) = &p.default {
                out.insert(p.key.clone(), d.clone());
            }
        }
        if let Some(k) = self.kind(kind) {
            for p in &k.props {
                if let Some(d) = &p.default {
                    out.insert(p.key.clone(), d.clone());
                }
            }
        }
        if let Some(v) = variant {
            for (key, val) in &v.defaults {
                out.insert(key.clone(), val.clone());
            }
        }
        out
    }

    /// Behavior of a node of `kind` in `variant`.
    pub fn behavior_of(&self, kind: &str, variant: Option<&VariantDef>) -> Option<Behavior> {
        self.kind(kind).map(|k| variant.and_then(|v| v.behavior).unwrap_or(k.behavior))
    }

    /// What the catalog says of the failures of a node of `kind` in `variant`: the kind's values, replaced by the variant's.
    pub fn reliability_of(&self, kind: &str, variant: Option<&VariantDef>) -> ReliabilityDef {
        let base = self.kind(kind).and_then(|k| k.reliability.clone()).unwrap_or_default();
        match variant.and_then(|v| v.reliability.as_ref()) {
            Some(v) => base.over(v),
            None => base,
        }
    }

    /// Traits of a node of `kind` in `variant`: the kind's, plus what the variant adds, minus what it removes.
    pub fn traits_of(&self, kind: &str, variant: Option<&VariantDef>) -> BTreeSet<String> {
        let mut t: BTreeSet<String> = self.kind(kind).map(|k| k.traits.iter().cloned().collect()).unwrap_or_default();
        if let Some(v) = variant {
            t.extend(v.add_traits.iter().cloned());
            for r in &v.remove_traits {
                t.remove(r);
            }
        }
        t
    }

    pub fn prop_def(&self, kind: &str, key: &str) -> Option<&PropDef> {
        self.kind(kind).and_then(|k| k.props.iter().find(|p| p.key == key)).or_else(|| self.common_props.iter().find(|p| p.key == key))
    }

    /// The range a numeric prop of `kind` may take, field by field: the kind's own minimum and maximum where it declares them, the
    /// common prop's of the same key where it does not. A product that describes its own `replicas` (most do: the default, the help) and
    /// says nothing of a maximum still has the maximum every other product has; without this a bound written once for all kinds would
    /// hold for the 294 kinds that inherit the whole definition and not for the 662 that restate it.
    pub fn prop_bounds(&self, kind: &str, key: &str) -> (Option<f64>, Option<f64>) {
        let own = self.kind(kind).and_then(|k| k.props.iter().find(|p| p.key == key));
        let common = self.common_props.iter().find(|p| p.key == key);
        (own.and_then(|p| p.min).or_else(|| common.and_then(|p| p.min)), own.and_then(|p| p.max).or_else(|| common.and_then(|p| p.max)))
    }

    pub fn kind_has_trait(&self, kind: &str, tr: &str) -> bool {
        self.kind(kind).map(|k| k.traits.iter().any(|t| t == tr)).unwrap_or(false)
    }

    /// Roles a node of `kind` can take in `variant`: the variant's own, else the kind's.
    pub fn roles_of<'a>(&'a self, kind: &'a KindDef, variant: Option<&'a VariantDef>) -> &'a [RoleDef] {
        match variant {
            Some(v) if !v.roles.is_empty() => &v.roles,
            _ => &kind.roles,
        }
    }

    /// Basic catalog self-consistency: unique ids and valid cross references.
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let mut seen = BTreeSet::new();
        for k in &self.kinds {
            if !seen.insert(("kind", k.id.as_str())) {
                errs.push(format!("duplicate kind id '{}'", k.id));
            }
            if !self.categories.iter().any(|c| c.id == k.category) {
                errs.push(format!("kind '{}' references unknown category '{}'", k.id, k.category));
            }
            if let Some(f) = k.function.as_deref() {
                let declared = self.categories.iter().find(|c| c.id == k.category).map(|c| c.functions.iter().any(|x| x.id == f)).unwrap_or(false);
                if !declared {
                    errs.push(format!("kind '{}' names function '{}' that category '{}' does not declare", k.id, f, k.category));
                }
            }
            let mut pk = BTreeSet::new();
            for p in &k.props {
                if !pk.insert(p.key.as_str()) {
                    errs.push(format!("kind '{}' has duplicate prop '{}'", k.id, p.key));
                }
                for v in &p.variants {
                    if !k.variants.iter().any(|x| &x.id == v) {
                        errs.push(format!("kind '{}' prop '{}' names unknown variant '{}'", k.id, p.key, v));
                    }
                }
            }
            let mut vk = BTreeSet::new();
            for v in &k.variants {
                if !vk.insert(v.id.as_str()) {
                    errs.push(format!("kind '{}' has duplicate variant '{}'", k.id, v.id));
                }
            }
            if k.variants.iter().filter(|v| v.default).count() > 1 {
                errs.push(format!("kind '{}' has more than one default variant", k.id));
            }
        }
        for t in &self.edge_types {
            if !seen.insert(("edge", t.id.as_str())) {
                errs.push(format!("duplicate edge type id '{}'", t.id));
            }
        }
        for t in &self.zone_types {
            if !seen.insert(("zone", t.id.as_str())) {
                errs.push(format!("duplicate zone type id '{}'", t.id));
            }
        }
        errs.extend(self.validate_ranges());
        errs
    }

    /// What is wrong with the ranges of the catalog: a minimum above the maximum allows nothing, a default outside the range is a value the catalog
    /// itself would refuse. Every message names the item, so the one an edit has just caused can be told from the ones that were already there
    /// (`validate` says these too).
    pub fn validate_ranges(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let mut defs: Vec<(String, &PropDef, (Option<f64>, Option<f64>))> = Vec::new();
        for p in &self.common_props {
            defs.push((format!("common prop '{}'", p.key), p, (p.min, p.max)));
        }
        for k in &self.kinds {
            for p in &k.props {
                defs.push((format!("kind '{}' prop '{}'", k.id, p.key), p, self.prop_bounds(&k.id, &p.key)));
            }
        }
        for t in &self.edge_types {
            for p in &t.props {
                defs.push((format!("edge type '{}' prop '{}'", t.id, p.key), p, (p.min, p.max)));
            }
        }
        for t in &self.zone_types {
            for p in &t.props {
                defs.push((format!("zone type '{}' prop '{}'", t.id, p.key), p, (p.min, p.max)));
            }
        }
        for (what, p, (min, max)) in &defs {
            if let (Some(lo), Some(hi)) = (min, max) {
                if lo > hi {
                    errs.push(format!("{what} has min {lo} above max {hi}"));
                }
            }
            if p.prop_type == PropType::Number {
                if let Some(d) = p.default.as_ref().and_then(crate::model::as_f64) {
                    if out_of(d, *min, *max) {
                        errs.push(format!("{what} has default {d} outside its range"));
                    }
                }
            }
        }
        for k in &self.kinds {
            for v in &k.variants {
                for (key, val) in &v.defaults {
                    let Some(def) = self.prop_def(&k.id, key) else { continue };
                    if def.prop_type != PropType::Number {
                        continue;
                    }
                    let (min, max) = self.prop_bounds(&k.id, key);
                    if let Some(d) = crate::model::as_f64(val) {
                        if out_of(d, min, max) {
                            errs.push(format!("kind '{}' variant '{}' default {} = {} is outside its range", k.id, v.id, key, d));
                        }
                    }
                }
            }
        }
        errs
    }
}

fn out_of(v: f64, min: Option<f64>, max: Option<f64>) -> bool {
    min.is_some_and(|m| v < m) || max.is_some_and(|m| v > m)
}

/// Effective props of a node/edge: catalog defaults overlaid with declared values, remembering
/// which keys were declared by the user (as opposed to assumed from the catalog).
#[derive(Debug, Clone, Default)]
pub struct Eff {
    pub map: Props,
    pub declared: BTreeSet<String>,
}

impl Eff {
    pub fn build(defaults: Props, declared: &Props) -> Self {
        let mut map = defaults;
        let mut d = BTreeSet::new();
        for (k, v) in declared {
            if !matches!(v, Value::Null) {
                map.insert(k.clone(), v.clone());
                d.insert(k.clone());
            }
        }
        Eff { map, declared: d }
    }
    pub fn num(&self, k: &str) -> Option<f64> {
        self.map.num(k)
    }
    pub fn flag(&self, k: &str) -> Option<bool> {
        self.map.flag(k)
    }
    pub fn text(&self, k: &str) -> Option<String> {
        self.map.text(k)
    }
    pub fn is_declared(&self, k: &str) -> bool {
        self.declared.contains(k)
    }
}

/// Render the card sub-line from a kind's template and effective props. Segments (split by
/// ` · `) that reference a property which is not set are dropped, so a partly configured node
/// shows `3 pods` instead of `3 pods · HPA {hpaMin}–{hpaMax}`.
pub fn render_subtitle(tpl: &LText, lang: &str, eff: &Props) -> String {
    let vars: BTreeMap<String, Value> = eff.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    tpl.get(lang).split(" · ").map(|seg| interpolate(seg, &vars)).filter(|seg| !seg.contains('{')).collect::<Vec<_>>().join(" · ")
}
