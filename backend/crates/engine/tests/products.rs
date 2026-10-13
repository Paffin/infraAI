//! The product catalog: variants change how the engine sees a node, quorums come from the members or the provider, and
//! lifecycle is judged against the shipped release tables.

use engine::catalog::Behavior;
use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::{evaluate, render_message, Report, Status};
use engine::lifecycle::{parse_date, Phase};
use engine::model::Architecture;
use engine::text::LText;
use serde_json::json;
use std::collections::BTreeSet;

fn october_2026() -> String {
    "2026-10-13".into()
}

fn june_2027() -> String {
    "2027-06-01".into()
}

fn cfg() -> EngineConfig {
    EngineConfig { today: october_2026, ..defaults::engine_config("ru") }
}

fn arch(v: serde_json::Value) -> Architecture {
    serde_json::from_value(v).unwrap()
}

fn both(t: &LText) -> bool {
    match t {
        LText::Map(m) => ["ru", "en"].iter().all(|l| m.get(*l).map(|s| !s.is_empty()).unwrap_or(false)),
        LText::Plain(s) => !s.is_empty(),
    }
}

fn message(c: &EngineConfig, r: &Report, rule: &str) -> String {
    let f = r.findings.iter().find(|f| f.rule_id == rule).unwrap_or_else(|| panic!("a finding of {rule}"));
    render_message(c.rules.iter().find(|x| x.id == rule).unwrap(), f, "ru")
}

fn status(r: &Report, id: &str) -> Status {
    r.rules.iter().find(|x| x.rule_id == id).unwrap_or_else(|| panic!("rule {id}")).status
}

#[test]
fn the_shipped_catalog_is_consistent() {
    let c = cfg();
    assert!(c.catalog.validate().is_empty(), "{:?}", c.catalog.validate());
    let tables: BTreeSet<&str> = c.versions.iter().map(|v| v.id.as_str()).collect();
    assert!(c.catalog.kinds.len() >= 100, "the library should cover the common products, has {}", c.catalog.kinds.len());
    for k in &c.catalog.kinds {
        assert!(both(&k.name), "{}: a name in both languages", k.id);
        assert!(k.description.as_ref().map(both).unwrap_or(false), "{}: a description in both languages", k.id);
        assert!(k.function.is_some(), "{}: every product has a function", k.id);
        for t in k.versions.iter().chain(k.variants.iter().filter_map(|v| v.versions.as_ref())).filter(|t| !t.is_empty()) {
            assert!(tables.contains(t.as_str()), "{}: release table {t} does not exist", k.id);
        }
        for v in &k.variants {
            assert!(both(&v.name), "{}/{}: a name in both languages", k.id, v.id);
            assert!(v.description.is_some(), "{}/{}: a description", k.id, v.id);
        }
        if !k.variants.is_empty() {
            assert_eq!(k.variants.iter().filter(|v| v.default).count(), 1, "{}: exactly one default variant", k.id);
        }
        // a property that names a table through another property: every option of that property is a table
        for p in &k.props {
            let Some(l) = &p.lifecycle else { continue };
            match l.strip_prefix('@') {
                Some(key) => {
                    let src = k.props.iter().find(|x| x.key == key).unwrap_or_else(|| panic!("{}: lifecycle of {} points at missing prop {key}", k.id, p.key));
                    assert!(!src.options.is_empty(), "{}: {key} must be an enum", k.id);
                    for o in &src.options {
                        assert!(tables.contains(o.value.as_str()), "{}: option {} of {key} is not a release table", k.id, o.value);
                    }
                }
                None => assert!(tables.contains(l.as_str()), "{}: lifecycle table {l} does not exist", k.id),
            }
        }
    }
    for cat in &c.catalog.categories {
        assert!(!cat.functions.is_empty(), "category {} declares its functions", cat.id);
    }
    for t in &c.versions {
        assert!(!t.name.is_empty(), "table {} has a display name", t.id);
        assert!(!t.cycles.is_empty(), "table {} has cycles", t.id);
        assert!(parse_date(&t.as_of).is_some(), "table {} says when it was taken", t.id);
    }
}

#[test]
fn a_variant_changes_behavior_traits_and_defaults_of_a_node() {
    let c = cfg();
    let cat = &c.catalog;
    let patroni = cat.variant("postgresql", None).unwrap();
    assert_eq!(patroni.id, "patroni", "the default variant is assumed when a node names none");
    assert_eq!(cat.variant("postgresql", Some("rds")).unwrap().id, "rds");
    assert!(cat.traits_of("postgresql", cat.variant("postgresql", Some("rds"))).contains("managed"));
    assert!(!cat.traits_of("postgresql", Some(patroni)).contains("managed"));
    let raft = cat.variant("vault", Some("raft"));
    assert_eq!(cat.behavior_of("vault", raft), Some(Behavior::Consensus));
    assert_eq!(cat.behavior_of("vault", None), Some(Behavior::PrimaryReplica));
    let eff = cat.default_props_for("redis", cat.variant("redis", Some("cluster")));
    assert_eq!(eff.get("mode"), Some(&json!("cluster")));
    assert!(cat.traits_of("redis", cat.variant("redis", Some("cluster"))).contains("self-quorum"));
}

fn two_db_sites(kind: &str, variant: Option<&str>, third_site: bool) -> Architecture {
    let mut props = json!({"failover": "auto"});
    if let Some(v) = variant {
        props["variant"] = json!(v);
    }
    let mut zones = vec![json!({"id":"dc1","name":"DC1","type":"dc","props":{"geo":"a"}}), json!({"id":"dc2","name":"DC2","type":"dc","props":{"geo":"b"}})];
    if third_site {
        zones.push(json!({"id":"dc3","name":"DC3","type":"dc","props":{"geo":"c"}}));
    }
    arch(json!({
        "requirements": {"rpsPeak": 100, "rtoSeconds": 900, "rpoSeconds": 0},
        "zones": zones,
        "nodes": [
            {"id":"users","kind":"users","name":"Users"},
            {"id":"app1","kind":"vm_pool","name":"App","zoneId":"dc1","group":"app","props":{"replicas":2}},
            {"id":"app2","kind":"vm_pool","name":"App","zoneId":"dc2","group":"app","props":{"replicas":2}},
            {"id":"p","kind":kind,"name":"DB primary","zoneId":"dc1","group":"db","role":"primary","props":props.clone()},
            {"id":"r","kind":kind,"name":"DB replica","zoneId":"dc2","group":"db","role":"replica","props":props}
        ],
        "edges": [
            {"id":"e1","source":"users","target":"app1","type":"traffic"},
            {"id":"e2","source":"users","target":"app2","type":"traffic"},
            {"id":"e3","source":"app1","target":"p","type":"traffic"},
            {"id":"e4","source":"app2","target":"r","type":"traffic"},
            {"id":"e5","source":"p","target":"r","type":"sync","props":{"latencyMs":1.0,"latencyMeasured":true}}
        ]
    }))
}

#[test]
fn automatic_failover_needs_a_quorum_unless_the_provider_or_the_members_hold_it() {
    let c = cfg();
    // Patroni with no DCS: the classic trap
    let rep = evaluate(&two_db_sites("postgresql", None, false), &c, &[]);
    assert_eq!(status(&rep, "HA-DB-02"), Status::Critical);
    assert_eq!(status(&rep, "DR-NET-01"), Status::Critical);
    // the same pair on a managed service: the provider holds the quorum
    let rep = evaluate(&two_db_sites("postgresql", Some("rds"), false), &c, &[]);
    assert_eq!(status(&rep, "HA-DB-02"), Status::Ok, "a managed service needs no DCS");
    assert_eq!(status(&rep, "DR-NET-01"), Status::Ok, "and does not split its brain");
    // MongoDB elects by the votes of its members: two members cannot form a majority across two sites
    let rep = evaluate(&two_db_sites("mongodb", None, false), &c, &[]);
    assert_eq!(status(&rep, "HA-DB-02"), Status::Ok);
    assert_eq!(status(&rep, "HA-QRM-01"), Status::Critical, "two voters are not a quorum design: {:?}", rep.findings.iter().filter(|f| f.rule_id == "HA-QRM-01").collect::<Vec<_>>());
}

#[test]
fn a_replica_set_survives_a_site_only_when_its_votes_do() {
    let c = cfg();
    let nodes = |zones: [&str; 3]| {
        arch(json!({
            "requirements": {"rpsPeak": 100, "rtoSeconds": 900, "rpoSeconds": 0},
            "zones": [
                {"id":"dc1","name":"DC1","type":"dc","props":{"geo":"a"}},
                {"id":"dc2","name":"DC2","type":"dc","props":{"geo":"b"}},
                {"id":"dc3","name":"DC3","type":"dc","props":{"geo":"c"}}
            ],
            "nodes": [
                {"id":"users","kind":"users","name":"Users"},
                {"id":"app1","kind":"vm_pool","name":"App","zoneId":"dc1","group":"app","props":{"replicas":2}},
                {"id":"app2","kind":"vm_pool","name":"App","zoneId":"dc2","group":"app","props":{"replicas":2}},
                {"id":"m1","kind":"mongodb","name":"mongo-1","zoneId":zones[0],"group":"mongo","role":"primary","props":{"failover":"auto"}},
                {"id":"m2","kind":"mongodb","name":"mongo-2","zoneId":zones[1],"group":"mongo","role":"replica","props":{"failover":"auto"}},
                {"id":"m3","kind":"mongodb","name":"mongo-3","zoneId":zones[2],"group":"mongo","role":"replica","props":{"failover":"auto"}}
            ],
            "edges": [
                {"id":"e1","source":"users","target":"app1","type":"traffic"},
                {"id":"e2","source":"users","target":"app2","type":"traffic"},
                {"id":"e3","source":"app1","target":"m1","type":"traffic"},
                {"id":"e4","source":"app2","target":"m2","type":"traffic"},
                {"id":"e5","source":"m1","target":"m2","type":"sync","props":{"latencyMs":1.0,"latencyMeasured":true}},
                {"id":"e6","source":"m1","target":"m3","type":"sync","props":{"latencyMs":1.0,"latencyMeasured":true}}
            ]
        }))
    };
    // two members on one site: losing it loses the majority
    let rep = evaluate(&nodes(["dc1", "dc1", "dc2"]), &c, &[]);
    assert_eq!(status(&rep, "HA-QRM-02"), Status::Critical, "{:?}", rep.findings.iter().filter(|f| f.rule_id == "HA-QRM-02").collect::<Vec<_>>());
    // one member per site: any single site can be lost
    let rep = evaluate(&nodes(["dc1", "dc2", "dc3"]), &c, &[]);
    assert_eq!(status(&rep, "HA-QRM-02"), Status::Ok);
    assert_eq!(status(&rep, "HA-QRM-01"), Status::Ok);
    assert_eq!(status(&rep, "HA-DB-02"), Status::Ok);
}

fn single_node(kind: &str, props: serde_json::Value) -> Architecture {
    arch(json!({
        "requirements": {"rpsPeak": 100},
        "zones": [{"id":"dc1","name":"DC1","type":"dc","props":{"geo":"a"}}],
        "nodes": [{"id":"n","kind":kind,"name":"Node","zoneId":"dc1","props":props}],
        "edges": []
    }))
}

#[test]
fn lifecycle_is_judged_against_the_release_tables_and_the_clock() {
    let c = cfg();
    let today = parse_date("2026-10-13").unwrap();
    // 12 reached its end of life in November 2024
    let rep = evaluate(&single_node("postgresql", json!({"version": "12.19"})), &c, &[]);
    assert_eq!(status(&rep, "VER-01"), Status::Critical);
    let f = rep.findings.iter().find(|f| f.rule_id == "VER-01").unwrap();
    let text = message(&c, &rep, "VER-01");
    assert!(text.contains("PostgreSQL 12.19") && text.contains("Рекомендуемая линия: 18"), "{text}");
    assert!(f.fixes.iter().any(|x| x.kind == "set_version"), "a move to the recommended line is offered");
    // a supported line is fine; an undeclared version is not judged
    assert_eq!(status(&evaluate(&single_node("postgresql", json!({"version": "17.2"})), &c, &[]), "VER-01"), Status::Ok);
    assert_eq!(status(&evaluate(&single_node("postgresql", json!({})), &c, &[]), "VER-01"), Status::Ok);
    // a line that is not in the table cannot be checked
    assert_eq!(status(&evaluate(&single_node("postgresql", json!({"version": "99"})), &c, &[]), "VER-01"), Status::NoData);
    // without a clock the rule is not applicable instead of guessing
    let rep = evaluate(&single_node("postgresql", json!({"version": "12.19"})), &defaults::engine_config("ru"), &[]);
    assert_eq!(status(&rep, "VER-01"), Status::Na);
    // the table behind a variant is its own: a managed offering lives by the provider's dates
    let set = c.versions.iter().find(|v| v.id == "amazon-rds-postgresql").unwrap();
    assert!(set.lifecycle("12", today).map(|l| l.phase == Phase::Eol).unwrap_or(true));
}

#[test]
fn the_os_and_the_runtime_of_a_node_are_judged_by_the_table_they_name() {
    let c = cfg();
    // Ubuntu 20.04 and Python 3.8 are long past their end of life; Debian 13 is fine
    let rep = evaluate(&single_node("vm_pool", json!({"os": "ubuntu", "osVersion": "20.04"})), &c, &[]);
    assert_eq!(status(&rep, "VER-01"), Status::Critical);
    let text = message(&c, &rep, "VER-01");
    assert!(text.contains("Ubuntu 20.04"), "{text}");
    println!("{text}");
    assert_eq!(status(&evaluate(&single_node("vm_pool", json!({"os": "debian", "osVersion": "13"})), &c, &[]), "VER-01"), Status::Ok);
    let rep = evaluate(&single_node("kubernetes", json!({"runtime": "python", "runtimeVersion": "3.8"})), &c, &[]);
    assert_eq!(status(&rep, "VER-01"), Status::Critical);
    // the same version number means different things in different tables
    let rep = evaluate(&single_node("vm_pool", json!({"os": "windows-server", "osVersion": "2022"})), &c, &[]);
    assert_eq!(status(&rep, "VER-01"), Status::Ok);
}

#[test]
fn members_of_one_cluster_on_different_lines_are_reported() {
    let c = cfg();
    let a = arch(json!({
        "requirements": {"rpsPeak": 100},
        "zones": [{"id":"dc1","name":"DC1","type":"dc","props":{"geo":"a"}}],
        "nodes": [
            {"id":"p","kind":"postgresql","name":"p","zoneId":"dc1","group":"pg","role":"primary","props":{"version":"16.3"}},
            {"id":"r","kind":"postgresql","name":"r","zoneId":"dc1","group":"pg","role":"replica","props":{"version":"17.1"}}
        ],
        "edges": []
    }));
    assert_eq!(status(&evaluate(&a, &c, &[]), "VER-02"), Status::Risk);
}

#[test]
fn recommended_lines_are_maintained_and_not_brand_new() {
    let c = cfg();
    let today = parse_date("2026-10-13").unwrap();
    for set in &c.versions {
        let Some(rec) = set.recommended(today, true, 90) else { continue };
        let l = set.lifecycle(&rec.cycle, today).unwrap();
        assert_ne!(l.phase, Phase::Eol, "{}: the recommended line {} is already over", set.id, rec.cycle);
    }
}

#[test]
fn a_release_table_that_is_too_old_is_reported_instead_of_trusted() {
    // the shipped tables are from October 2026: eight months later their dates are no basis for a verdict
    let c = EngineConfig { today: june_2027, ..defaults::engine_config("ru") };
    let rep = evaluate(&single_node("postgresql", json!({"version": "17.2"})), &c, &[]);
    assert_eq!(status(&rep, "VER-01"), Status::NoData);
    let text = message(&c, &rep, "VER-01");
    assert!(text.contains("PostgreSQL") && text.contains("устарела"), "{text}");
    // a node that declares no version has nothing to doubt
    assert_eq!(status(&evaluate(&single_node("postgresql", json!({})), &c, &[]), "VER-01"), Status::Ok);
}
