//! Placement wizard: the patterns and stacks it offers, a preview of a generated design with the verdict of the
//! rule engine, and a comparison of the same design for several numbers of sites. Nothing is stored here; a project
//! is created from a request through `POST /projects`.

use crate::api_support::{lang_of, limits};
use crate::auth::AuthUser;
use crate::config_store::Loaded;
use crate::error::{ApiError, ApiResult};
use crate::json::Json;
use crate::state::AppState;
use axum::extract::State;
use engine::evaluate::{evaluate, Report, Status};
use engine::placement::{generate, PlacementError, PlacementRequest};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Everything the wizard needs to draw its form.
pub async fn options(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    let p = &cfg.placement;
    let managed_traits = &cfg.engine.authority.provider_traits;
    let patterns: Vec<Value> = p.patterns.iter().filter(|x| x.enabled).map(|x| serde_json::to_value(x).unwrap_or(Value::Null)).collect();
    let stacks: Vec<Value> = p
        .stacks
        .iter()
        .map(|s| {
            json!({
                "id": s.id, "name": s.name, "description": s.description,
                "layers": s.layers.iter().map(|l| json!({
                    "id": l.id, "name": l.name, "kind": l.kind, "alternatives": l.alternatives, "function": l.function, "scope": l.scope,
                    "optional": l.optional, "default": l.default, "dcs": l.dcs,
                    // every product the layer can be built from, with the ways to deploy each (the wizard offers them)
                    "products": engine::placement::layer_products(&cfg.catalog, l).iter().map(|k| json!({
                        "kind": k.id,
                        "variants": k.variants.iter().map(|v| json!({"id": v.id, "default": v.default, "managed": v.add_traits.iter().any(|t| managed_traits.contains(t))})).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let zone_types: Vec<&str> = cfg.catalog.zone_types.iter().filter(|z| z.level == engine::catalog::ZoneLevel::Site).map(|z| z.id.as_str()).collect();
    // the pattern the wizard offers first: the administrator's choice when it is on offer, else the first one
    let default_pattern = p.patterns.iter().filter(|x| x.enabled).find(|x| x.id == p.settings.default_pattern).or_else(|| p.patterns.iter().find(|x| x.enabled)).map(|x| x.id.clone());
    Ok(Json(json!({
        "patterns": patterns,
        "defaultPattern": default_pattern,
        "stacks": stacks,
        "siteTypes": zone_types,
        "maxSiteFailures": cfg.engine.scenarios.max_site_failures,
        "maxNodes": limits(&cfg).max_nodes,
    })))
}

fn to_api(e: PlacementError) -> ApiError {
    ApiError::bad_request("placement_failed", e.message.clone()).with_details(json!({"code": e.code}))
}

/// The verdict on a generated design, in the shape the wizard shows.
fn verdict(r: &Report) -> Value {
    let mut top: Vec<&engine::evaluate::Finding> = r.findings.iter().filter(|f| f.accepted.is_none() && matches!(f.status, Status::Critical | Status::Risk)).collect();
    top.truncate(12);
    json!({
        "score": r.score.value,
        "coverage": r.score.coverage,
        "counts": r.counts,
        "objectives": r.objectives,
        "cost": {"total": r.cost.total, "currency": r.cost.currency, "symbol": r.cost.symbol, "incomplete": r.cost.incomplete},
        "scenarioStats": r.scenario_stats,
        "worstCapacity": r.capacity.iter().filter_map(|c| c.worst.as_ref().filter(|w| w.load_rps > 0.0).map(|w| w.capacity_rps / w.load_rps)).fold(None::<f64>, |acc, v| Some(acc.map_or(v, |a| a.min(v)))),
        "findings": top.iter().map(|f| json!({"id": f.id, "ruleId": f.rule_id, "status": f.status, "vars": f.vars})).collect::<Vec<_>>(),
    })
}

fn build(cfg: &Loaded, mut req: PlacementRequest, lang: &str) -> Result<(engine::placement::PlacementResult, Report), PlacementError> {
    let ec = cfg.engine_config(lang);
    if req.max_nodes.is_none() {
        req.max_nodes = Some(limits(cfg).max_nodes);
    }
    req.lang = Some(lang.to_string());
    let res = generate(&ec, &ec.placement, &req)?;
    let report = evaluate(&res.architecture, &ec, &[]);
    Ok((res, report))
}

/// Generate a design and judge it.
pub async fn preview(State(state): State<AppState>, _user: AuthUser, Json(req): Json<PlacementRequest>) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    let lang = lang_of(&cfg, req.lang.as_deref());
    state.metrics.evaluations.with_label_values(&["placement"]).inc();
    let (res, report) = tokio::task::spawn_blocking(move || build(&cfg, req, &lang)).await.map_err(ApiError::internal)?.map_err(to_api)?;
    Ok(Json(json!({
        "architecture": res.architecture,
        "sites": res.sites,
        "siteFailures": res.site_failures,
        "notes": res.notes,
        "issues": report.issues,
        "report": verdict(&report),
        "nodes": res.architecture.nodes.len(),
        "edges": res.architecture.edges.len(),
    })))
}

#[derive(Deserialize)]
pub struct CompareBody {
    /// Shared part of the requests.
    pub base: PlacementRequest,
    /// What differs: each entry overrides the fields it sets.
    pub variants: Vec<PlacementRequest>,
}

fn overlay(base: &PlacementRequest, v: &PlacementRequest) -> PlacementRequest {
    let mut r = base.clone();
    if !v.pattern.is_empty() {
        r.pattern = v.pattern.clone();
    }
    macro_rules! o {
        ($f:ident) => {
            if v.$f.is_some() {
                r.$f = v.$f.clone();
            }
        };
    }
    o!(sites);
    o!(regions);
    o!(racks);
    o!(site_failures);
    o!(stack);
    o!(layers);
    o!(site_type);
    o!(requirements);
    r
}

/// The same design for several numbers of sites (or patterns): what each costs and survives.
pub async fn compare(State(state): State<AppState>, _user: AuthUser, Json(b): Json<CompareBody>) -> ApiResult<Json<Value>> {
    let cfg = state.cfg().await?;
    let max = cfg.settings.num("limits.placementCompareMax") as usize;
    if b.variants.is_empty() || b.variants.len() > max.max(1) {
        return Err(ApiError::bad_request("invalid_variants", format!("1–{} variants are allowed", max.max(1))));
    }
    let lang = lang_of(&cfg, b.base.lang.as_deref());
    state.metrics.evaluations.with_label_values(&["placement"]).inc();
    let mut tasks = vec![];
    for v in &b.variants {
        let req = overlay(&b.base, v);
        let (cfg, lang) = (Arc::clone(&cfg), lang.clone());
        tasks.push(tokio::task::spawn_blocking(move || {
            let sites = req.sites;
            let pattern = req.pattern.clone();
            match build(&cfg, req, &lang) {
                Ok((res, report)) => json!({
                    "ok": true, "pattern": pattern, "sites": sites, "siteFailures": res.site_failures,
                    "nodes": res.architecture.nodes.len(), "zones": res.architecture.zones.len(),
                    "workingSites": res.sites.iter().filter(|s| s.role == "working" || s.role == "standby").count(),
                    "arbiterSites": res.sites.iter().filter(|s| s.role == "witness").count(),
                    "notes": res.notes, "report": verdict(&report),
                }),
                Err(e) => json!({"ok": false, "pattern": pattern, "sites": sites, "error": {"code": e.code, "message": e.message}}),
            }
        }));
    }
    let mut out = vec![];
    for t in tasks {
        out.push(t.await.map_err(ApiError::internal)?);
    }
    Ok(Json(json!({"variants": out})))
}
