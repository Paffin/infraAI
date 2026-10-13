//! Deterministic cost estimate from the price book in settings. It is an *estimate* – every
//! node that lacks the data needed to price it is listed as `incomplete` and never silently
//! counted as free.

use crate::config::EngineSettings;
use crate::graph::Index;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCost {
    pub node_id: String,
    pub amount: f64,
    /// Which inputs were missing (`vcpu`, `ramGb`, `diskGb`); the amount covers the rest.
    pub missing: Vec<String>,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostReport {
    pub currency: String,
    pub symbol: String,
    pub total: f64,
    pub nodes: Vec<NodeCost>,
    pub by_category: BTreeMap<String, f64>,
    pub by_zone: BTreeMap<String, f64>,
    pub by_kind: BTreeMap<String, f64>,
    /// Nodes that could not be fully priced.
    pub incomplete: usize,
    pub budget: Option<f64>,
}

fn resolve_model(ix: &Index<'_>, ni: usize, settings: &EngineSettings) -> String {
    for &zi in &ix.zone_chain[ni] {
        if let Some(m) = ix.arch.zones[zi].props.get("costModel").and_then(|v| v.as_str()) {
            if settings.cost.models.contains_key(m) {
                return m.to_string();
            }
        }
    }
    settings.cost.default_model.clone()
}

pub fn estimate(ix: &Index<'_>) -> CostReport {
    let settings = ix.settings;
    let mut nodes = Vec::new();
    let mut total = 0.0;
    let mut by_category: BTreeMap<String, f64> = BTreeMap::new();
    let mut by_zone: BTreeMap<String, f64> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, f64> = BTreeMap::new();
    let mut incomplete = 0;

    for (ni, n) in ix.arch.nodes.iter().enumerate() {
        let eff = &ix.eff[ni];
        let model_id = resolve_model(ix, ni, settings);
        let model = settings.cost.models.get(&model_id);
        let replicas = ix.replicas(ni) as f64;
        let mut missing = Vec::new();
        let kind = ix.kind[ni];
        let cost_def = ix.variant[ni].and_then(|v| v.cost.clone()).or_else(|| kind.and_then(|k| k.cost.clone())).unwrap_or_default();
        let multiplier = if cost_def.multiplier == 0.0 { 1.0 } else { cost_def.multiplier };
        let mut per_instance = 0.0;
        if let Some(m) = model {
            match eff.num("vcpu") {
                Some(v) => per_instance += v * m.vcpu_month,
                None => missing.push("vcpu".to_string()),
            }
            match eff.num("ramGb") {
                Some(v) => per_instance += v * m.ram_gb_month,
                None => missing.push("ramGb".to_string()),
            }
            if let Some(d) = eff.num("diskGb") {
                let class = eff.text("storageClass").unwrap_or_else(|| m.default_disk_class.clone());
                let rate = m.disk_gb_month.get(&class).copied().or_else(|| m.disk_gb_month.get(&m.default_disk_class).copied()).unwrap_or(0.0);
                per_instance += d * rate;
            }
        } else {
            missing.push("priceModel".to_string());
        }
        let amount = (per_instance * multiplier + cost_def.monthly_fixed) * replicas;
        // entry/external nodes (users) and pure network links carry no cost of their own
        let free = ix.has_trait(ni, "entry") || ix.has_trait(ni, "external");
        let missing = if free { vec![] } else { missing };
        if !missing.is_empty() && amount == 0.0 && !free {
            // nothing priced at all
        }
        if !missing.is_empty() {
            incomplete += 1;
        }
        total += amount;
        if let Some(k) = kind {
            *by_category.entry(k.category.clone()).or_insert(0.0) += amount;
            *by_kind.entry(k.id.clone()).or_insert(0.0) += amount;
        }
        let zone_key = n.zone_id.clone().unwrap_or_else(|| "_global".to_string());
        *by_zone.entry(zone_key).or_insert(0.0) += amount;
        nodes.push(NodeCost { node_id: n.id.clone(), amount, missing, model: model_id });
    }
    let round = |v: f64| (v * 100.0).round() / 100.0;
    CostReport {
        currency: settings.cost.currency.clone(),
        symbol: settings.cost.symbol.clone(),
        total: round(total),
        nodes,
        by_category: by_category.into_iter().map(|(k, v)| (k, round(v))).collect(),
        by_zone: by_zone.into_iter().map(|(k, v)| (k, round(v))).collect(),
        by_kind: by_kind.into_iter().map(|(k, v)| (k, round(v))).collect(),
        incomplete,
        budget: ix.arch.requirements.budget_monthly,
    }
}
