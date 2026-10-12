//! Behavioural tests of the deterministic engine on reference architectures with known
//! properties and known defects.

use engine::config::{EngineConfig, Waiver};
use engine::defaults;
use engine::evaluate::{evaluate, Report, Status};
use engine::graph::Index;
use engine::model::{Architecture, Requirements};
use engine::ops::{apply_ops, Op};
use engine::sim::{simulate, NodeState, RecoveryKind, Scenario, ServiceState};
use serde_json::json;

fn cfg() -> EngineConfig {
    EngineConfig { catalog: defaults::catalog(), rules: defaults::rules(), settings: defaults::engine_settings(), lang: "ru".into() }
}

fn template(id: &str) -> Architecture {
    defaults::templates().into_iter().find(|t| t.id == id).unwrap().architecture
}

fn demo() -> Architecture {
    defaults::demo_project().architecture
}

fn sim(arch: &Architecture, sc: Scenario) -> engine::sim::SimResult {
    let c = cfg();
    let ix = Index::build(arch, &c.catalog, &c.settings);
    simulate(&ix, &sc)
}

fn rule_status(r: &Report, id: &str) -> Status {
    r.rules.iter().find(|x| x.rule_id == id).unwrap_or_else(|| panic!("rule {id}")).status
}

fn findings_of<'a>(r: &'a Report, id: &str) -> Vec<&'a engine::evaluate::Finding> {
    r.findings.iter().filter(|f| f.rule_id == id).collect()
}

#[test]
fn defaults_are_consistent() {
    let c = cfg();
    assert!(c.catalog.validate().is_empty(), "{:?}", c.catalog.validate());
    // every rule id unique, every template valid against the catalog
    let mut ids: Vec<_> = c.rules.iter().map(|r| r.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), c.rules.len());
    for t in defaults::templates() {
        let issues = engine::model::validate(&t.architecture, &c.catalog, &Default::default());
        assert!(issues.iter().all(|i| i.level != engine::model::IssueLevel::Error), "template {} invalid: {:?}", t.id, issues);
    }
    let d = demo();
    assert!(engine::model::validate(&d, &c.catalog, &Default::default()).iter().all(|i| i.level != engine::model::IssueLevel::Error));
}

#[test]
fn two_dc_baseline_is_up_with_headroom() {
    let a = template("two-dc-witness");
    let r = sim(&a, Scenario::None);
    assert_eq!(r.state, ServiceState::Up);
    assert!(r.capacity_ratio.unwrap() >= 1.0, "ratio {:?}", r.capacity_ratio);
}

#[test]
fn two_dc_survives_loss_of_dc1_with_35s_and_zero_rpo() {
    let a = template("two-dc-witness");
    let r = sim(&a, Scenario::Zone { zone_id: "dc1".into() });
    assert_ne!(r.state, ServiceState::Down, "events: {:?}", r.events);
    assert_eq!(r.recovery.kind, RecoveryKind::Auto);
    // GSLB: 5s health-check × 1 + 30s TTL = 35s; PG failover 30s + 5s etcd re-election = 35s
    assert_eq!(r.recovery.seconds, Some(35.0));
    let (rpo, _) = engine::sim::rpo_filtered(&r.rpo_items, &["mq".to_string(), "secrets".to_string(), "cache".to_string()]);
    assert_eq!(rpo, Some(0.0), "sync replica promoted: {:?}", r.rpo_items);
    assert_eq!(r.node_states["pg-replica"], NodeState::Promoted);
    assert_eq!(r.node_states["pg-primary"], NodeState::Down);
    assert!(r.split_brain.is_empty());
}

#[test]
fn two_dc_survives_loss_of_dc2_and_witness_site() {
    let a = template("two-dc-witness");
    for z in ["dc2", "site3"] {
        let r = sim(&a, Scenario::Zone { zone_id: z.into() });
        assert_ne!(r.state, ServiceState::Down, "{z}: {:?}", r.events);
    }
}

#[test]
fn link_cut_between_dcs_causes_no_split_brain() {
    let a = template("two-dc-witness");
    let r = sim(&a, Scenario::Link { a: "dc1".into(), b: "dc2".into() });
    assert!(r.split_brain.is_empty(), "{:?}", r.events);
    assert_ne!(r.state, ServiceState::Down);
    // the DC2 app cannot reach the primary and stalls; DC1 serves
    assert_eq!(r.node_states["app-dc2"], NodeState::Stalled);
    assert_eq!(r.node_states["app-dc1"], NodeState::Up);
}

#[test]
fn losing_both_dcs_restores_from_offsite_backup() {
    let a = template("two-dc-witness");
    let r = sim(&a, Scenario::Sites { zone_ids: vec!["dc1".into(), "dc2".into()] });
    assert_eq!(r.state, ServiceState::Down);
    assert_eq!(r.recovery.kind, RecoveryKind::Manual);
    assert_eq!(r.recovery.seconds, Some(7200.0));
    assert_eq!(r.rpo_seconds, Some(300.0));
    // only the (unbacked, mirrored) Kafka clusters lose their in-flight data; the system of record is restored
    assert!(r.data_lost.iter().all(|u| u.starts_with("kafka")), "{:?}", r.data_lost);
}

#[test]
fn clean_template_has_no_critical_findings_and_no_spof() {
    let a = template("two-dc-witness");
    let rep = evaluate(&a, &cfg(), &[]);
    let crit: Vec<_> = rep.findings.iter().filter(|f| f.status == Status::Critical).map(|f| (&f.rule_id, &f.vars)).collect();
    assert!(crit.is_empty(), "unexpected critical: {crit:#?}");
    assert_eq!(rule_status(&rep, "HA-SPOF-01"), Status::Ok);
    assert_eq!(rule_status(&rep, "DR-NET-01"), Status::Ok);
    assert_eq!(rule_status(&rep, "HA-QRM-02"), Status::Ok);
    assert!(rep.score.value >= 85.0, "score {}", rep.score.value);
}

#[test]
fn all_reference_templates_have_no_critical_findings() {
    for t in defaults::templates() {
        let rep = evaluate(&t.architecture, &cfg(), &[]);
        let crit: Vec<_> = rep.findings.iter().filter(|f| f.status == Status::Critical).map(|f| (f.rule_id.clone(), f.vars.clone())).collect();
        assert!(crit.is_empty(), "template {} has critical findings: {crit:#?}", t.id);
    }
}

#[test]
fn evaluation_is_deterministic() {
    let a = demo();
    let c = cfg();
    let r1 = serde_json::to_string(&evaluate(&a, &c, &[])).unwrap();
    let r2 = serde_json::to_string(&evaluate(&a, &c, &[])).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn demo_project_shows_the_known_defects() {
    let rep = evaluate(&demo(), &cfg(), &[]);
    // Redis without a replica across sites
    assert_eq!(rule_status(&rep, "HA-CACHE-01"), Status::Risk);
    // async Kafka mirror without mitigation
    assert_eq!(rule_status(&rep, "DR-KFK-01"), Status::Risk);
    // RTT stated without measurement
    assert_eq!(rule_status(&rep, "NET-LAT-01"), Status::NoData);
    // but the core HA properties hold
    assert_eq!(rule_status(&rep, "HA-SPOF-01"), Status::Ok);
    assert_eq!(rule_status(&rep, "HA-SITE-01"), Status::Ok);
    assert_eq!(rep.counts.critical, 0, "{:#?}", rep.findings.iter().filter(|f| f.status == Status::Critical).collect::<Vec<_>>());
}

#[test]
fn cache_finding_offers_a_fix_that_resolves_it() {
    let c = cfg();
    let a = demo();
    let rep = evaluate(&a, &c, &[]);
    let f = findings_of(&rep, "HA-CACHE-01").into_iter().next().expect("finding");
    let fix = f.fixes.first().expect("a mechanical fix is offered");
    let out = apply_ops(&a, &fix.ops, &c.catalog, &Default::default()).expect("fix applies");
    let rep2 = evaluate(&out.architecture, &c, &[]);
    // the specific cache finding that was fixed is gone
    assert!(rep2.findings.iter().all(|x| x.id != f.id), "finding still present");
}

// ------------------------------------------------------------------ defects

fn single_vm_app() -> Architecture {
    serde_json::from_value(json!({
        "requirements": {"rpsPeak": 100, "rtoSeconds": 900, "rpoSeconds": 0},
        "zones": [{"id":"dc1","name":"DC1","type":"dc","props":{"tier":"III","geo":"msk"}}],
        "nodes": [
            {"id":"users","kind":"users","name":"Users"},
            {"id":"app","kind":"vm_pool","name":"App","zoneId":"dc1","props":{"replicas":1,"vcpu":2,"ramGb":4}},
            {"id":"db","kind":"postgresql","name":"DB","zoneId":"dc1","role":"primary","props":{"vcpu":4,"ramGb":16,"diskGb":100}}
        ],
        "edges": [
            {"id":"e1","source":"users","target":"app","type":"traffic"},
            {"id":"e2","source":"app","target":"db","type":"traffic"},
            {"id":"e3","source":"users","target":"db","type":"traffic"}
        ]
    }))
    .unwrap()
}

#[test]
fn single_site_single_instance_app_is_flagged_everywhere() {
    let rep = evaluate(&single_vm_app(), &cfg(), &[]);
    assert_eq!(rule_status(&rep, "HA-SPOF-01"), Status::Critical);
    assert_eq!(rule_status(&rep, "HA-SITE-01"), Status::Critical);
    assert_eq!(rule_status(&rep, "DR-BKP-01"), Status::Critical);
    assert_eq!(rule_status(&rep, "NET-SEG-01"), Status::Critical, "users → db directly");
    assert_eq!(rule_status(&rep, "NET-WAF-01"), Status::Risk);
    assert_eq!(rule_status(&rep, "HA-OBS-01"), Status::Risk);
    assert!(rep.score.value < 50.0, "score {}", rep.score.value);
    let sp: Vec<_> = findings_of(&rep, "HA-SPOF-01").iter().map(|f| f.subjects.nodes[0].clone()).collect();
    assert!(sp.contains(&"app".to_string()) && sp.contains(&"db".to_string()), "{sp:?}");
}

#[test]
fn missing_requirements_yield_no_data_not_green() {
    let mut a = template("two-dc-witness");
    a.requirements = Requirements::default();
    let rep = evaluate(&a, &cfg(), &[]);
    assert_eq!(rule_status(&rep, "HA-RTO-01"), Status::NoData);
    assert_eq!(rule_status(&rep, "DR-RPO-01"), Status::NoData);
    assert_eq!(rule_status(&rep, "CAP-01"), Status::NoData);
    assert_eq!(rule_status(&rep, "COST-01"), Status::NoData);
    assert!(rep.score.coverage < 1.0);
}

fn two_node_db_no_arbiter() -> Architecture {
    // the classic trap: primary + replica in two DCs, automatic failover, no quorum authority
    serde_json::from_value(json!({
        "requirements": {"rpsPeak": 100, "rtoSeconds": 900, "rpoSeconds": 0},
        "zones": [
            {"id":"dc1","name":"DC1","type":"dc","props":{"geo":"a"}},
            {"id":"dc2","name":"DC2","type":"dc","props":{"geo":"b"}}
        ],
        "nodes": [
            {"id":"users","kind":"users","name":"Users"},
            {"id":"app1","kind":"vm_pool","name":"App","zoneId":"dc1","group":"app","props":{"replicas":2}},
            {"id":"app2","kind":"vm_pool","name":"App","zoneId":"dc2","group":"app","props":{"replicas":2}},
            {"id":"p","kind":"postgresql","name":"PG primary","zoneId":"dc1","group":"pg","role":"primary","props":{"failover":"auto"}},
            {"id":"r","kind":"postgresql","name":"PG replica","zoneId":"dc2","group":"pg","role":"replica","props":{"failover":"auto"}}
        ],
        "edges": [
            {"id":"e1","source":"users","target":"app1","type":"traffic"},
            {"id":"e2","source":"users","target":"app2","type":"traffic"},
            {"id":"e3","source":"app1","target":"p","type":"traffic"},
            {"id":"e4","source":"app2","target":"r","type":"traffic"},
            {"id":"e5","source":"p","target":"r","type":"sync","props":{"latencyMs":1.0,"latencyMeasured":true}}
        ]
    }))
    .unwrap()
}

#[test]
fn two_databases_without_arbiter_can_split_brain() {
    let a = two_node_db_no_arbiter();
    let r = sim(&a, Scenario::Link { a: "dc1".into(), b: "dc2".into() });
    assert_eq!(r.split_brain, vec!["pg".to_string()], "events {:?}", r.events);
    let rep = evaluate(&a, &cfg(), &[]);
    assert_eq!(rule_status(&rep, "DR-NET-01"), Status::Critical);
    assert_eq!(rule_status(&rep, "HA-DB-02"), Status::Critical);
    // the remedy is offered: add an arbiter in a third site is impossible here (no third zone),
    // but the finding must at least point at the unit
    let f = &findings_of(&rep, "HA-DB-02")[0];
    assert!(f.subjects.nodes.contains(&"p".to_string()));
}

#[test]
fn adding_an_arbiter_site_fixes_the_split_brain() {
    let c = cfg();
    let mut a = two_node_db_no_arbiter();
    a.zones.push(serde_json::from_value(json!({"id":"dc3","name":"DC3","type":"cloud","props":{"geo":"c"}})).unwrap());
    let rep = evaluate(&a, &c, &[]);
    let f = findings_of(&rep, "HA-DB-02")[0];
    let fix = f.fixes.iter().find(|x| x.kind == "add_witness").expect("add_witness offered when a third site exists");
    let out = apply_ops(&a, &fix.ops, &c.catalog, &Default::default()).unwrap();
    let rep2 = evaluate(&out.architecture, &c, &[]);
    assert_eq!(rule_status(&rep2, "HA-DB-02"), Status::Ok);
    assert_eq!(rule_status(&rep2, "DR-NET-01"), Status::Ok);
    let r = sim(&out.architecture, Scenario::Link { a: "dc1".into(), b: "dc2".into() });
    assert!(r.split_brain.is_empty());
}

#[test]
fn manual_failover_counts_manual_time_against_rto() {
    let mut a = template("two-dc-witness");
    for n in a.nodes.iter_mut().filter(|n| n.kind == "postgresql") {
        n.props.insert("failover".into(), json!("manual"));
    }
    let r = sim(&a, Scenario::Zone { zone_id: "dc1".into() });
    assert_eq!(r.recovery.kind, RecoveryKind::Manual);
    assert_eq!(r.recovery.seconds, Some(1800.0));
    let rep = evaluate(&a, &cfg(), &[]);
    // RTO is 900 s but a human needs 1800 s
    assert_eq!(rule_status(&rep, "HA-RTO-01"), Status::Critical);
}

#[test]
fn async_replication_violates_zero_rpo_and_declares_assumption() {
    let mut a = template("two-dc-witness");
    for e in a.edges.iter_mut().filter(|e| e.id == "e-pg-primary--pg-replica") {
        e.edge_type = "async".into();
    }
    let r = sim(&a, Scenario::Zone { zone_id: "dc1".into() });
    assert_eq!(r.rpo_seconds, Some(5.0));
    assert!(r.rpo_assumed, "lag was not declared, so the 5 s is an assumption");
    let rep = evaluate(&a, &cfg(), &[]);
    assert_eq!(rule_status(&rep, "DR-RPO-01"), Status::Critical);
    assert_eq!(rule_status(&rep, "HA-DB-03"), Status::Critical);
}

#[test]
fn waiver_moves_a_finding_out_of_the_score_but_keeps_it_visible() {
    let a = demo();
    let c = cfg();
    let rep = evaluate(&a, &c, &[]);
    let f = findings_of(&rep, "DR-KFK-01").into_iter().next().unwrap().clone();
    let w = Waiver { fingerprint: f.id.clone(), reason: "orders are protected by the outbox".into(), by: Some("qa".into()), at: None };
    let rep2 = evaluate(&a, &c, &[w]);
    assert_eq!(rule_status(&rep2, "DR-KFK-01"), Status::Ok);
    assert_eq!(rep2.counts.accepted, 1);
    assert!(rep2.score.value >= rep.score.value);
    assert!(rep2.findings.iter().any(|x| x.id == f.id && x.accepted.is_some()));
}

#[test]
fn no_data_findings_cannot_be_waived() {
    let a = demo();
    let c = cfg();
    let rep = evaluate(&a, &c, &[]);
    let f = findings_of(&rep, "NET-LAT-01").into_iter().next().unwrap().clone();
    let w = Waiver { fingerprint: f.id.clone(), reason: "x".into(), by: None, at: None };
    let rep2 = evaluate(&a, &c, &[w]);
    assert_eq!(rule_status(&rep2, "NET-LAT-01"), Status::NoData);
}

#[test]
fn pilot_light_scales_out_on_disaster_and_adds_scale_time() {
    let a = template("multi-region-pilot-light");
    let r = sim(&a, Scenario::Zone { zone_id: "r1-a".into() });
    assert_ne!(r.state, ServiceState::Down, "{:?}", r.events);
    let all_r1 = sim(&a, Scenario::Geo { geo: "region-1".into() });
    assert_ne!(all_r1.state, ServiceState::Down, "region 1 loss must be survivable: {:?}", all_r1.events);
    assert_eq!(all_r1.recovery.kind, RecoveryKind::Manual, "DB promotion in the DR region is manual");
    assert!(all_r1.recovery.seconds.unwrap() >= 1800.0);
    assert!(all_r1.events.iter().any(|e| e.code == "scale_out"));
}

#[test]
fn capacity_shortfall_is_reported_and_fixable() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    for n in a.nodes.iter_mut().filter(|n| n.kind == "kubernetes") {
        n.props.insert("replicas".into(), json!(4));
    }
    let rep = evaluate(&a, &c, &[]);
    assert_eq!(rule_status(&rep, "CAP-01"), Status::Critical);
    assert_eq!(rule_status(&rep, "CAP-02"), Status::Risk);
    let f = findings_of(&rep, "CAP-01").into_iter().find(|f| !f.fixes.is_empty()).expect("scale fix");
    let out = apply_ops(&a, &f.fixes[0].ops, &c.catalog, &Default::default()).unwrap();
    let rep2 = evaluate(&out.architecture, &c, &[]);
    assert_ne!(rule_status(&rep2, "CAP-01"), Status::Critical);
}

#[test]
fn budget_exceeded_is_critical() {
    let mut a = template("two-dc-witness");
    a.requirements.budget_monthly = Some(1000.0);
    let rep = evaluate(&a, &cfg(), &[]);
    assert_eq!(rule_status(&rep, "COST-01"), Status::Critical);
    assert!(rep.cost.total > 1000.0);
}

#[test]
fn disabled_rules_are_skipped_and_scenarios_are_bounded() {
    let mut c = cfg();
    c.rules.iter_mut().for_each(|r| r.enabled = r.id == "HA-SPOF-01");
    let rep = evaluate(&template("event-driven-kafka"), &c, &[]);
    assert_eq!(rep.rules.len(), 1);
    c.settings.scenarios.max_count = 3;
    let a = template("event-driven-kafka");
    let ix = Index::build(&a, &c.catalog, &c.settings);
    assert!(engine::sim::standard_scenarios(&ix).len() <= 3);
}

#[test]
fn group_hygiene_detects_ungrouped_parallel_nodes() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    for n in a.nodes.iter_mut().filter(|n| n.id.starts_with("ingress-")) {
        n.group = None;
    }
    let rep = evaluate(&a, &c, &[]);
    assert_eq!(rule_status(&rep, "HA-GRP-01"), Status::Risk);
    // and the engine consequently sees AND semantics: losing a whole DC now stops the service
    assert_eq!(rule_status(&rep, "HA-SITE-01"), Status::Critical);
    let f = &findings_of(&rep, "HA-GRP-01")[0];
    let out = apply_ops(&a, &f.fixes[0].ops, &c.catalog, &Default::default()).unwrap();
    let rep2 = evaluate(&out.architecture, &c, &[]);
    assert_eq!(rule_status(&rep2, "HA-GRP-01"), Status::Ok);
    assert_eq!(rule_status(&rep2, "HA-SITE-01"), Status::Ok);
}

#[test]
fn ops_roundtrip_through_json() {
    let ops: Vec<Op> = serde_json::from_value(json!([
        {"op":"update_node","id":"app-dc1","set":{"props.replicas":40}},
        {"op":"set_requirements","set":{"rpsPeak":9000}}
    ]))
    .unwrap();
    let c = cfg();
    let a = template("two-dc-witness");
    let out = apply_ops(&a, &ops, &c.catalog, &Default::default()).unwrap();
    assert_eq!(out.architecture.requirements.rps_peak, Some(9000.0));
    assert_eq!(out.architecture.node("app-dc1").unwrap().props["replicas"], json!(40));
}

// ------------------------------------------------------------------ exports

#[test]
fn exports_are_generated_from_the_model() {
    let c = cfg();
    let a = template("two-dc-witness");
    let rep = evaluate(&a, &c, &[]);
    for f in engine::export::FORMATS {
        let files = engine::export::generate(f, &a, &c, &rep, &[], "ru", "2026-10-12").unwrap_or_else(|| panic!("format {f}"));
        assert!(!files.is_empty(), "{f}");
        assert!(files.iter().all(|x| !x.content.trim().is_empty()), "{f} has an empty file");
    }
    let tf = engine::export::terraform(&a, &c);
    let main = tf.iter().find(|f| f.path == "main.tf").unwrap();
    assert!(main.content.contains("module \"pg_primary\""));
    assert!(main.content.contains("depends_on"), "dependency ordering");
    assert!(!main.content.lines().any(|l| l.trim_start().starts_with("version =")), "reserved module argument must not be used as an input");
    assert!(main.content.contains("pg_version"));
    assert!(tf.iter().any(|f| f.path == "modules/postgresql/main.tf"));
    let hf = engine::export::helm(&a, &c);
    assert!(hf.iter().any(|f| f.path == "helmfile.yaml" && f.content.contains("releases:")));
    let adr = engine::export::adr(&a, &c, &rep, &[], "ru", "2026-10-12");
    assert!(adr.content.contains("99/100") || adr.content.contains("100/100") || adr.content.contains("/100"));
    let rb = engine::export::runbook(&a, &c, "en");
    assert!(rb.content.contains("Scenario:") && rb.content.contains("patronictl list"), "{}", rb.content);
    let mm = engine::export::mermaid(&a, &c);
    assert!(mm.starts_with("flowchart TB") && mm.contains("subgraph"));
}

#[test]
fn hcl_strings_are_escaped() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    a.nodes[1].name = "evil \"name\" ${jndi:x} %{ if }".into();
    let tf = engine::export::terraform(&a, &c);
    let main = &tf.iter().find(|f| f.path == "main.tf").unwrap().content;
    assert!(main.contains("$${jndi"), "template interpolation must be escaped: {main}");
    assert!(!main.contains("\"${") && !main.contains(" ${jndi"));
    assert!(main.contains("%%{ if }"));
}

#[test]
fn data_lost_for_good_breaks_the_rpo_objective_and_a_backup_removes_the_loss() {
    // two sites, the database lives in one of them with neither a replica nor a backup
    let mut a = template("two-dc-witness");
    a.nodes.retain(|n| n.id != "pg-replica");
    a.edges.retain(|e| e.source != "pg-replica" && e.target != "pg-replica");
    let before = evaluate(&a, &cfg(), &[]);
    let rpo = before.objectives.iter().find(|o| o.id == "rpo").unwrap();
    assert_eq!(rpo.status, Status::Critical, "RPO tile must not be green while the data is gone");
    let f = findings_of(&before, "DR-RPO-01");
    assert!(f.iter().any(|f| f.id.ends_with(":lost")), "{f:?}");
    let rule = cfg().rules.iter().find(|r| r.id == "DR-RPO-01").unwrap().clone();
    assert!(engine::evaluate::render_message(&rule, f[0], "ru").contains("безвозвратно"));

    // attaching the existing backup storage turns "lost for good" into "restore from backup": the
    // data-loss finding goes away and the number of critical findings does not grow (the restore
    // time may now violate the RTO, which the engine reports honestly)
    let fix = before.findings.iter().flat_map(|f| f.fixes.iter()).find(|x| x.kind == "add_backup").expect("backup fix");
    let fixed = engine::ops::apply_ops(&a, &fix.ops, &cfg().catalog, &Default::default()).unwrap().architecture;
    let after = evaluate(&fixed, &cfg(), &[]);
    assert!(!findings_of(&after, "DR-RPO-01").iter().any(|f| f.id.ends_with(":lost")));
    assert!(after.counts.critical <= before.counts.critical, "critical {} -> {}", before.counts.critical, after.counts.critical);
}
