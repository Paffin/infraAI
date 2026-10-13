//! The placement generator against the rule engine: every pattern, for one site up to ten, must produce a design the
//! engine has nothing critical to say about. The generator proposes, the engine judges.

use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::{evaluate, Report, Status};
use engine::graph::Index;
use engine::model::{validate, IssueLevel};
use engine::placement::{generate, PlacementRequest, PlacementResult};
use engine::sim::{simulate, standard_scenarios, RecoveryKind, Scenario, ServiceState};

fn cfg() -> EngineConfig {
    defaults::engine_config("en")
}

fn make(_cfg: &EngineConfig, pattern: &str, sites: u32) -> PlacementRequest {
    PlacementRequest { pattern: pattern.into(), sites: Some(sites), lang: Some("en".into()), ..Default::default() }
}

fn run(cfg: &EngineConfig, req: &PlacementRequest) -> (PlacementResult, Report) {
    let res = generate(cfg, &cfg.placement, req).unwrap_or_else(|e| panic!("{req:?}: {e}"));
    let issues = validate(&res.architecture, &cfg.catalog, &Default::default());
    assert!(issues.iter().all(|i| i.level != IssueLevel::Error), "{req:?}: invalid architecture: {issues:?}");
    let rep = evaluate(&res.architecture, cfg, &[]);
    (res, rep)
}

fn bad(rep: &Report) -> Vec<String> {
    rep.findings
        .iter()
        .filter(|f| f.accepted.is_none() && matches!(f.status, Status::Critical | Status::Risk))
        .map(|f| format!("{:?} {} {}", f.status, f.id, serde_json::to_string(&f.vars).unwrap_or_default()))
        .collect()
}

#[test]
fn every_pattern_gives_a_clean_design_for_one_to_ten_sites() {
    let c = cfg();
    let mut count = 0;
    for p in c.placement.patterns.iter().filter(|p| p.enabled) {
        let top = p.max_sites.unwrap_or(10).min(10);
        for n in p.min_sites..=top {
            let mut req = make(&c, &p.id, n);
            if p.regions.mode == engine::placement::RegionMode::Count {
                req.regions = Some(p.regions.default.min(n));
            }
            let (res, rep) = run(&c, &req);
            let problems = bad(&rep);
            assert!(problems.is_empty(), "pattern {} with {} sites: {:#?}\nnotes: {:?}", p.id, n, problems, res.notes);
            assert!(rep.score.value >= 80.0, "pattern {} with {} sites scores {}", p.id, n, rep.score.value);
            count += 1;
        }
    }
    assert!(count >= 20, "only {count} combinations were checked");
}

#[test]
fn generation_is_deterministic() {
    let c = cfg();
    let req = make(&c, "n-sites", 7);
    let a = serde_json::to_string(&generate(&c, &c.placement, &req).unwrap().architecture).unwrap();
    let b = serde_json::to_string(&generate(&c, &c.placement, &req).unwrap().architecture).unwrap();
    assert_eq!(a, b);
}

#[test]
fn site_counts_outside_the_pattern_are_refused() {
    let c = cfg();
    for (pattern, sites) in [("single-dc", 2), ("two-sites", 3), ("n-sites", 2), ("n-sites", 21), ("multi-region", 1)] {
        let e = generate(&c, &c.placement, &make(&c, pattern, sites)).unwrap_err();
        assert_eq!(e.code, "sites_out_of_range", "{pattern} {sites}");
    }
    assert_eq!(generate(&c, &c.placement, &make(&c, "nope", 3)).unwrap_err().code, "unknown_pattern");
    let mut req = make(&c, "n-sites", 3);
    req.kinds.insert("db".into(), "kafka".into());
    assert_eq!(generate(&c, &c.placement, &req).unwrap_err().code, "bad_kind");
}

#[test]
fn a_single_dc_is_protected_against_rack_loss_and_says_so() {
    let c = cfg();
    let (res, rep) = run(&c, &PlacementRequest { racks: Some(4), ..make(&c, "single-dc", 1) });
    let arch = &res.architecture;
    assert_eq!(arch.requirements.failure_tolerance.as_deref(), Some("rack"));
    assert_eq!(arch.zones.iter().filter(|z| z.zone_type == "rack").count(), 4);
    // every rack is a scenario; the data center itself is not required to survive
    let ix = Index::build(arch, &c.catalog, &c.settings);
    let racks = standard_scenarios(&ix).into_iter().filter(|s| matches!(s, Scenario::Zone { zone_id } if zone_id.contains("-r"))).count();
    assert_eq!(racks, 4);
    for r in 1..=4 {
        let sim = simulate(&ix, &Scenario::Zone { zone_id: format!("dc1-r{r}") });
        assert_ne!(sim.state, ServiceState::Down, "losing rack {r}: {:?}", sim.events);
        assert!(sim.recovery.seconds.unwrap_or(0.0) <= 60.0, "rack {r}: {:?}", sim.recovery);
    }
    // the site loss is a restore from the off-site copy – the engine reports it instead of pretending otherwise
    let dc = simulate(&ix, &Scenario::Zone { zone_id: "dc1".into() });
    assert_eq!(dc.state, ServiceState::Down);
    assert_eq!(dc.recovery.kind, RecoveryKind::Manual);
    assert!(rep.objectives.iter().any(|o| o.id == "site_loss"));
    assert_eq!(rep.scenario_stats.racks, 4);
}

#[test]
fn two_racks_cannot_hold_a_quorum_of_three_and_the_engine_says_it() {
    // honest generator: with two racks and no third place inside the DC the deciding vote goes off-site; the design
    // must still pass because the generator placed it there
    let c = cfg();
    let (res, rep) = run(&c, &PlacementRequest { racks: Some(2), ..make(&c, "single-dc", 1) });
    assert!(bad(&rep).is_empty(), "{:?}", bad(&rep));
    assert!(res.architecture.nodes.iter().any(|n| n.kind == "etcd" && n.role.as_deref() == Some("witness")));
}

#[test]
fn five_sites_survive_any_two_lost_at_once() {
    let c = cfg();
    let (res, rep) = run(&c, &make(&c, "n-sites", 5));
    assert_eq!(res.site_failures, 2);
    assert_eq!(res.architecture.requirements.site_failures, Some(2));
    let ix = Index::build(&res.architecture, &c.catalog, &c.settings);
    let ids: Vec<String> = res.sites.iter().map(|s| s.id.clone()).collect();
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let sim = simulate(&ix, &Scenario::Sites { zone_ids: vec![ids[i].clone(), ids[j].clone()] });
            assert_ne!(sim.state, ServiceState::Down, "{} + {}: {:?}", ids[i], ids[j], sim.events);
            assert!(sim.data_lost.is_empty(), "{} + {}: data lost {:?}", ids[i], ids[j], sim.data_lost);
        }
    }
    let r = rep.rules.iter().find(|r| r.rule_id == "HA-SITE-02").unwrap();
    assert_eq!(r.status, Status::Ok);
    assert!(rep.objectives.iter().any(|o| o.id == "sites_loss" && o.status == Status::Ok));
}

#[test]
fn a_design_that_cannot_lose_two_sites_is_caught() {
    // the same five sites, but the database lives on two of them only
    let c = cfg();
    let (res, _) = run(&c, &make(&c, "n-sites", 5));
    let mut arch = res.architecture.clone();
    let dbs: Vec<String> = arch.nodes.iter().filter(|n| n.kind == "postgresql" && n.role.as_deref() == Some("replica")).skip(1).map(|n| n.id.clone()).collect();
    assert!(!dbs.is_empty());
    arch.nodes.retain(|n| !dbs.contains(&n.id));
    arch.edges.retain(|e| !dbs.contains(&e.source) && !dbs.contains(&e.target));
    let rep = evaluate(&arch, &c, &[]);
    let r = rep.rules.iter().find(|r| r.rule_id == "HA-SITE-02").unwrap();
    assert_eq!(r.status, Status::Critical, "{:?}", bad(&rep));
    assert!(rep.objectives.iter().any(|o| o.id == "sites_loss" && o.status == Status::Critical));
    // and the spread rule asks for k+1 sites as well
    assert!(rep.findings.iter().any(|f| f.rule_id == "HA-ZONE-01"), "{:?}", bad(&rep));
}

#[test]
fn an_isolated_site_is_a_scenario_with_three_or_more_sites() {
    let c = cfg();
    let (res, rep) = run(&c, &make(&c, "n-sites", 5));
    let ix = Index::build(&res.architecture, &c.catalog, &c.settings);
    let set = standard_scenarios(&ix);
    let single = set.iter().filter(|s| matches!(s, Scenario::Isolate { zone_ids } if zone_ids.len() == 1)).count();
    let pairs = set.iter().filter(|s| matches!(s, Scenario::Isolate { zone_ids } if zone_ids.len() == 2)).count();
    assert_eq!(single, 5);
    assert_eq!(pairs, 10);
    // an isolated site keeps running but is on the minority side: the quorum moves on without it
    let sim = simulate(&ix, &Scenario::Isolate { zone_ids: vec!["dc1".into()] });
    assert_ne!(sim.state, ServiceState::Down, "{:?}", sim.events);
    assert!(sim.split_brain.is_empty(), "{:?}", sim.events);
    assert!(rep.scenarios.iter().any(|s| matches!(s.scenario, Scenario::Isolate { .. })));
    // with two sites the only cut is the link between them
    let (res2, _) = run(&c, &make(&c, "single-dc", 1));
    let ix2 = Index::build(&res2.architecture, &c.catalog, &c.settings);
    assert!(!standard_scenarios(&ix2).iter().any(|s| matches!(s, Scenario::Isolate { .. })));
}

#[test]
fn ten_sites_stay_within_the_scenario_budget_and_say_when_they_do_not() {
    let c = cfg();
    let (res, rep) = run(&c, &make(&c, "n-sites", 10));
    let ix = Index::build(&res.architecture, &c.catalog, &c.settings);
    let set = engine::sim::scenario_set(&ix);
    assert!(set.scenarios.len() <= c.settings.scenarios.max_count);
    assert!(!rep.scenario_stats.truncated, "{:?}", rep.scenario_stats);
    // a tighter combination budget samples the pairs and reports it
    let mut tight = cfg();
    tight.settings.scenarios.max_combinations = 10;
    let rep = evaluate(&res.architecture, &tight, &[]);
    assert!(rep.scenario_stats.truncated, "{:?}", rep.scenario_stats);
    assert!(rep.scenarios.len() < 100);
}

#[test]
fn optional_layers_other_kinds_and_racks_combine() {
    let c = cfg();
    let mut req = make(&c, "n-sites", 4);
    req.layers = Some(["waf", "dcs", "db", "cache", "mq", "objects", "search", "analytics", "secrets", "monitoring", "gitops", "backup"].iter().map(|s| s.to_string()).collect());
    req.kinds.insert("app".into(), "vm_pool".into());
    req.racks = Some(2);
    let (res, rep) = run(&c, &req);
    let problems = bad(&rep);
    assert!(problems.is_empty(), "{problems:#?}\n{:?}", res.notes);
    for kind in ["kafka", "object_storage", "opensearch", "clickhouse", "vm_pool"] {
        assert!(res.architecture.nodes.iter().any(|n| n.kind == kind), "{kind} missing");
    }
    // racks are zones of their own below every working site
    assert_eq!(res.architecture.zones.iter().filter(|z| z.zone_type == "rack").count(), 8);
}

#[test]
fn switching_layers_off_removes_them_and_their_links() {
    let c = cfg();
    let mut req = make(&c, "two-sites", 2);
    req.layers = Some(vec!["db".into()]);
    let (res, rep) = run(&c, &req);
    let kinds: Vec<&str> = res.architecture.nodes.iter().map(|n| n.kind.as_str()).collect();
    for gone in ["waf", "redis", "vault", "gitops", "monitoring", "backup_storage"] {
        assert!(!kinds.contains(&gone), "{gone} should be off");
    }
    // without the WAF the balancers are fed straight from the DNS layer
    assert!(res.architecture.edges.iter().any(|e| e.source == "gslb" && e.target.starts_with("ingress")));
    // a database without a backup is something the engine objects to
    assert!(rep.findings.iter().any(|f| f.rule_id == "DR-BKP-01"), "{:?}", bad(&rep));
}

#[test]
fn regions_split_the_sites_into_disaster_domains() {
    let c = cfg();
    let mut req = make(&c, "multi-region", 6);
    req.regions = Some(3);
    let (res, rep) = run(&c, &req);
    let geos: std::collections::BTreeSet<&String> = res.architecture.zones.iter().filter(|z| z.zone_type == "dc").filter_map(|z| z.props.get("geo").and_then(|g| g.as_str().map(|_| &z.id))).collect();
    assert_eq!(geos.len(), 6);
    let labels: std::collections::BTreeSet<String> = res.sites.iter().filter(|s| s.role == "working").map(|s| s.geo.clone()).collect();
    assert_eq!(labels.len(), 3);
    assert!(bad(&rep).is_empty(), "{:?}", bad(&rep));
    assert_eq!(res.architecture.requirements.failure_tolerance.as_deref(), Some("geo"));
    // losing a whole region is survived: the geo rules are applicable and green
    let r = rep.rules.iter().find(|r| r.rule_id == "DR-GEO-02").unwrap();
    assert_eq!(r.status, Status::Ok);
}

#[test]
fn the_standby_site_is_a_pilot_light_that_scales_out() {
    let c = cfg();
    let mut req = make(&c, "active-standby", 3);
    req.regions = Some(2);
    let (res, rep) = run(&c, &req);
    assert!(bad(&rep).is_empty(), "{:?}", bad(&rep));
    let app = res.architecture.nodes.iter().find(|n| n.id == "app-dc3").expect("standby app");
    assert!(app.props.contains_key("failoverReplicas"));
    assert!(app.props.get("replicas").and_then(|v| v.as_f64()).unwrap() < app.props.get("failoverReplicas").and_then(|v| v.as_f64()).unwrap());
    let db = res.architecture.nodes.iter().find(|n| n.id == "db-dc3").expect("standby database");
    assert_eq!(db.props.get("failover").and_then(|v| v.as_str()), Some("manual"));
}

#[test]
fn the_node_limit_is_respected() {
    let c = cfg();
    let mut req = make(&c, "n-sites", 10);
    req.racks = Some(3);
    req.max_nodes = Some(100);
    let e = generate(&c, &c.placement, &req).unwrap_err();
    assert_eq!(e.code, "too_many_nodes");
}

#[test]
fn rack_tolerance_alone_does_not_demand_site_rules() {
    let c = cfg();
    let (_, rep) = run(&c, &make(&c, "single-dc", 1));
    for id in ["HA-SITE-01", "HA-SITE-02", "HA-ZONE-02", "HA-QRM-02", "DR-NET-03"] {
        let r = rep.rules.iter().find(|r| r.rule_id == id).unwrap();
        assert_eq!(r.status, Status::Na, "{id}");
    }
    for id in ["HA-RACK-01", "HA-RACK-02", "HA-QRM-03"] {
        let r = rep.rules.iter().find(|r| r.rule_id == id).unwrap();
        assert_eq!(r.status, Status::Ok, "{id}");
    }
}
