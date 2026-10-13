//! The estimate of availability against the real simulator and the real catalog: what it counts, what moves it and what
//! it refuses to claim.

use engine::catalog::ReliabilityDef;
use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::{evaluate, render_message, Report, Status};
use engine::model::Architecture;
use engine::reliability::{Availability, Cause, Standing};
use serde_json::json;

fn cfg() -> EngineConfig {
    defaults::engine_config("en")
}

fn template(id: &str) -> Architecture {
    defaults::templates().into_iter().find(|t| t.id == id).unwrap().architecture
}

fn rule_status(r: &Report, id: &str) -> Status {
    r.rules.iter().find(|x| x.rule_id == id).unwrap_or_else(|| panic!("rule {id}")).status
}

fn avail(a: &Architecture, c: &EngineConfig) -> Availability {
    evaluate(a, c, &[]).availability.expect("an estimate")
}

/// One data center, users in front of one application cluster of `replicas` instances: the smallest design with a
/// failure that matters.
fn small(replicas: u32, variant: Option<&str>) -> Architecture {
    let mut props = json!({ "replicas": replicas, "vcpu": 2, "ramGb": 4 });
    if let Some(v) = variant {
        props["variant"] = json!(v);
    }
    serde_json::from_value(json!({
        "requirements": { "availabilitySlo": 99.9 },
        "zones": [{ "id": "dc1", "name": "DC-1", "type": "dc", "role": "active", "props": { "geo": "g1", "tier": "III" } }],
        "nodes": [
            { "id": "users", "kind": "users", "name": "Users" },
            { "id": "app", "kind": "kubernetes", "name": "App", "zoneId": "dc1", "group": "app", "props": props }
        ],
        "edges": [{ "id": "e1", "source": "users", "target": "app", "type": "traffic" }]
    }))
    .unwrap()
}

#[test]
fn every_design_that_ships_meets_the_target_it_carries() {
    let c = cfg();
    let mut all: Vec<(String, Architecture)> = defaults::templates().into_iter().map(|t| (t.id, t.architecture)).collect();
    all.push(("demo".into(), defaults::demo_project().architecture));
    for (id, a) in all {
        let rep = evaluate(&a, &c, &[]);
        let av = rep.availability.as_ref().unwrap_or_else(|| panic!("{id}: no estimate"));
        assert!(av.target.is_some(), "{id} carries no availability target");
        assert_eq!(av.standing(), Standing::Within, "{id}: {:.5} against {:?}, {:?} of the budget", av.estimate, av.target, av.budget_used());
        assert_eq!(rule_status(&rep, "AV-SLO-01"), Status::Ok, "{id}");
    }
}

#[test]
fn the_estimate_is_deterministic_and_costs_a_whole_year_of_failures() {
    let c = cfg();
    let a = template("two-dc-witness");
    let one = serde_json::to_string(&avail(&a, &c)).unwrap();
    let two = serde_json::to_string(&avail(&a, &c)).unwrap();
    assert_eq!(one, two);
    let av = avail(&a, &c);
    let year = c.settings.reliability.days_per_year * 86_400.0;
    assert!((av.estimate - (1.0 - av.downtime_seconds_per_year / year)).abs() < 1e-12);
    // the shares of the causes add up to the whole
    let shares: f64 = av.causes.iter().map(|x| x.share).sum();
    assert!((shares - 1.0).abs() < 1e-9, "{shares}");
    let seconds: f64 = av.causes.iter().map(|x| x.seconds_per_year).sum();
    assert!((seconds - av.downtime_seconds_per_year).abs() < 1e-6);
    // the costliest failures come first and each one is rate × downtime
    assert!(av.top.windows(2).all(|w| w[0].seconds_per_year >= w[1].seconds_per_year));
    for t in &av.top {
        assert!((t.per_year * t.downtime_seconds - t.seconds_per_year).abs() < 1e-6, "{t:?}");
    }
}

#[test]
fn a_single_instance_is_the_failure_that_costs_most_and_a_second_one_removes_it() {
    let c = cfg();
    let one = avail(&small(1, None), &c);
    let three = avail(&small(3, None), &c);
    assert!(one.estimate < three.estimate, "{} vs {}", one.estimate, three.estimate);
    // with one instance its loss is down until the repair: instance failures × the repair time
    let top = &one.top[0];
    assert_eq!(top.cause, Cause::Instance);
    assert_eq!(top.basis, "repair");
    let want = c.settings.reliability.failures.instance * c.settings.reliability.repair.instance;
    assert!((top.seconds_per_year - want).abs() < 1e-6, "{} vs {}", top.seconds_per_year, want);
    // with three it is the loss of the whole service (a bad release) and the site that remain
    assert!(three.top.iter().all(|t| t.cause != Cause::Instance || t.seconds_per_year < 60.0), "{:?}", three.top);
    assert!(three.top.iter().any(|t| t.cause == Cause::Unit));
    // and the single instance is named as the service that costs most
    assert_eq!(one.units[0].name, "App");
}

#[test]
fn a_slower_failover_costs_more() {
    let c = cfg();
    let auto = template("two-dc-witness");
    let mut manual = auto.clone();
    for n in manual.nodes.iter_mut().filter(|n| n.kind == "postgresql") {
        n.props.insert("failover".into(), json!("manual"));
    }
    let (a, m) = (avail(&auto, &c), avail(&manual, &c));
    assert!(m.downtime_seconds_per_year > a.downtime_seconds_per_year, "{} vs {}", m.downtime_seconds_per_year, a.downtime_seconds_per_year);
    assert!(m.units.iter().any(|u| u.name == "PostgreSQL"));
}

#[test]
fn what_a_provider_promises_becomes_incidents_of_the_service() {
    let mut c = cfg();
    // a managed cluster: the provider's instances are not ours, its SLA is
    let managed = small(3, Some("eks"));
    let year = c.settings.reliability.days_per_year * 86_400.0;
    let sla = *c.settings.reliability.provider.by_function.get("k8s").expect("an SLA for the function");
    let av = avail(&managed, &c);
    let p: Vec<_> = av.top.iter().filter(|t| t.cause == Cause::Provider).collect();
    assert_eq!(p.len(), 1, "{:?}", av.top);
    assert!((p[0].seconds_per_year - (1.0 - sla) * year).abs() < 1e-3, "{} vs {}", p[0].seconds_per_year, (1.0 - sla) * year);
    // …and none of its instances or its release is counted
    assert!(av.top.iter().all(|t| t.cause != Cause::Instance && t.cause != Cause::Unit), "{:?}", av.top);

    // the catalog can say what the SLA of one product is; it replaces the table of the settings
    let kind = c.catalog.kinds.iter_mut().find(|k| k.id == "kubernetes").unwrap();
    let eks = kind.variants.iter_mut().find(|v| v.id == "eks").unwrap();
    eks.reliability = Some(ReliabilityDef { sla: Some(0.99), ..Default::default() });
    let worse = avail(&managed, &c);
    let p = worse.top.iter().find(|t| t.cause == Cause::Provider).unwrap();
    assert!((p.seconds_per_year - 0.01 * year).abs() < 1e-3, "{}", p.seconds_per_year);
    assert!(worse.estimate < av.estimate);
}

#[test]
fn a_better_data_center_tier_makes_losing_it_rarer() {
    let c = cfg();
    let at = |tier: &str| {
        let mut a = small(3, None);
        a.zones[0].props.insert("tier".into(), json!(tier));
        avail(&a, &c)
    };
    let (i, iii, iv) = (at("I"), at("III"), at("IV"));
    assert!(i.estimate < iii.estimate && iii.estimate < iv.estimate, "{} {} {}", i.estimate, iii.estimate, iv.estimate);
    let site = |av: &Availability| av.causes.iter().find(|x| x.cause == Cause::Site).map(|x| x.seconds_per_year).unwrap_or(0.0);
    let f = &c.settings.reliability.tier_factor;
    assert!((site(&i) / site(&iii) - f["I"]).abs() < 1e-9);
    assert!((site(&iv) / site(&iii) - f["IV"]).abs() < 1e-9);
}

#[test]
fn the_target_is_a_percentage_or_a_fraction_and_decides_the_budget() {
    let c = cfg();
    let year = c.settings.reliability.days_per_year * 86_400.0;
    let mut a = small(3, None);
    a.requirements.availability_slo = Some(99.95);
    let pct = avail(&a, &c);
    a.requirements.availability_slo = Some(0.9995);
    let frac = avail(&a, &c);
    assert_eq!(pct.target, frac.target);
    assert!((pct.budget_seconds_per_year.unwrap() - 0.0005 * year).abs() < 1e-6);
    assert!((pct.budget_used().unwrap() - pct.downtime_seconds_per_year / pct.budget_seconds_per_year.unwrap()).abs() < 1e-12);
    // a target of 100 % allows nothing
    a.requirements.availability_slo = Some(100.0);
    let all = avail(&a, &c);
    assert_eq!(all.standing(), Standing::Over);
}

#[test]
fn standing_follows_the_share_of_the_budget_and_the_rule_follows_the_standing() {
    let c = cfg();
    let a0 = small(3, None);
    let est = avail(&a0, &c);
    let year = c.settings.reliability.days_per_year * 86_400.0;
    let target_for = |used: f64| 100.0 * (1.0 - est.downtime_seconds_per_year / used / year);
    for (used, standing, status) in [(0.5, Standing::Within, Status::Ok), (0.9, Standing::Close, Status::Risk), (1.5, Standing::Over, Status::Critical)] {
        let mut a = a0.clone();
        a.requirements.availability_slo = Some(target_for(used));
        let rep = evaluate(&a, &c, &[]);
        assert_eq!(rep.availability.as_ref().unwrap().standing(), standing, "{used}");
        assert_eq!(rule_status(&rep, "AV-SLO-01"), status, "{used}");
        let tile = rep.objectives.iter().find(|o| o.id == "availability").expect("a tile");
        assert_eq!(tile.status, status, "the tile and the rule never disagree ({used})");
    }
}

#[test]
fn without_a_target_the_estimate_is_still_there_and_asks_for_one() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    a.requirements.availability_slo = None;
    let rep = evaluate(&a, &c, &[]);
    assert!(rep.availability.as_ref().unwrap().target.is_none());
    let f = rep.findings.iter().find(|f| f.rule_id == "AV-SLO-01").expect("a finding");
    assert_eq!(f.status, Status::NoData);
    assert!(f.id.ends_with(":req"));
    assert!(f.needs.iter().any(|n| n.target == "requirements" && n.prop == "availabilitySlo"), "{:?}", f.needs);
    let rule = c.rules.iter().find(|r| r.id == "AV-SLO-01").unwrap();
    let text = render_message(rule, f, "en");
    assert!(text.contains("No availability target") && text.contains('%') && !text.contains('{'), "{text}");
}

#[test]
fn the_message_names_the_estimate_the_budget_and_the_costliest_service() {
    let c = cfg();
    let mut a = small(1, None);
    a.requirements.availability_slo = Some(99.99);
    let rep = evaluate(&a, &c, &[]);
    let f = rep.findings.iter().find(|f| f.rule_id == "AV-SLO-01").unwrap();
    assert_eq!(f.status, Status::Critical);
    let rule = c.rules.iter().find(|r| r.id == "AV-SLO-01").unwrap();
    let av = rep.availability.as_ref().unwrap();
    for lang in ["en", "ru"] {
        let text = render_message(rule, f, lang);
        assert!(!text.contains('{') && !text.contains('}'), "{lang}: {text}");
        assert!(text.contains(&engine::text::format_availability(av.estimate)), "{lang}: {text}");
        assert!(text.contains("99.99%"), "{lang}: {text}");
        assert!(text.contains("App"), "{lang}: {text}");
    }
    // the finding points at the failure that costs most, so the canvas can replay it
    assert_eq!(f.scenario.as_ref(), Some(&av.top[0].scenario));
    assert!(f.subjects.nodes.contains(&"app".to_string()));
}

#[test]
fn switching_the_estimate_off_removes_it_everywhere() {
    let mut c = cfg();
    c.settings.reliability.enabled = false;
    let rep = evaluate(&template("two-dc-witness"), &c, &[]);
    assert!(rep.availability.is_none());
    assert_eq!(rule_status(&rep, "AV-SLO-01"), Status::Na);
    assert!(rep.objectives.iter().all(|o| o.id != "availability"));
}

#[test]
fn a_rate_of_zero_takes_a_cause_out() {
    let mut c = cfg();
    let a = small(3, None);
    assert!(avail(&a, &c).causes.iter().any(|x| x.cause == Cause::Unit));
    c.settings.reliability.failures.unit = 0.0;
    let av = avail(&a, &c);
    assert!(av.causes.iter().all(|x| x.cause != Cause::Unit), "{:?}", av.causes);
    c.settings.reliability.failures.zone.insert("dc".into(), 0.0);
    let av = avail(&a, &c);
    assert!(av.causes.iter().all(|x| x.cause != Cause::Site), "{:?}", av.causes);
}

#[test]
fn a_catalog_entry_sets_the_failures_of_its_own_product() {
    let mut c = cfg();
    let a = small(1, None);
    let base = avail(&a, &c);
    let kind = c.catalog.kinds.iter_mut().find(|k| k.id == "kubernetes").unwrap();
    kind.reliability = Some(ReliabilityDef { failures_per_year: Some(4.0), repair_seconds: Some(3600.0), ..Default::default() });
    let worse = avail(&a, &c);
    let top = worse.top.iter().find(|t| t.cause == Cause::Instance).unwrap();
    assert_eq!(top.per_year, 4.0);
    assert_eq!(top.downtime_seconds, 3600.0);
    assert!((top.seconds_per_year - 14_400.0).abs() < 1e-9);
    // 4 failures a year of an hour each cost half of 2 a year of four hours
    assert!(worse.estimate > base.estimate, "{} vs {}", worse.estimate, base.estimate);
}

#[test]
fn capacity_a_failure_takes_away_counts_unless_it_is_switched_off() {
    let mut c = cfg();
    c.settings.reliability.top = 200;
    // a peak the two data centers carry together but not one of them alone
    let mut a = template("two-dc-witness");
    a.requirements.rps_peak = Some(200_000.0);
    a.requirements.growth_percent = Some(0.0);
    let with = avail(&a, &c);
    assert!(with.top.iter().any(|t| t.basis == "capacity"), "{:?}", with.top.iter().map(|t| (t.cause, &t.basis)).collect::<Vec<_>>());
    c.settings.reliability.partial_capacity = false;
    let without = avail(&a, &c);
    assert!(with.downtime_seconds_per_year > without.downtime_seconds_per_year, "{} vs {}", with.downtime_seconds_per_year, without.downtime_seconds_per_year);
    assert!(without.top.iter().all(|t| t.basis != "capacity"));
}

#[test]
fn an_undersized_design_is_not_charged_for_the_same_shortfall_in_every_failure() {
    let c = cfg();
    // the application cannot carry the peak even when nothing has failed: that is a finding of the capacity rules
    let mut a = small(2, None);
    a.requirements.rps_peak = Some(1_000_000.0);
    let rep = evaluate(&a, &c, &[]);
    let av = rep.availability.unwrap();
    let year = c.settings.reliability.days_per_year * 86_400.0;
    assert!(av.downtime_seconds_per_year < 0.01 * year, "the baseline shortfall must not multiply into every failure: {}", av.downtime_seconds_per_year);
}

#[test]
fn a_service_that_does_not_work_at_all_is_not_estimated_failure_by_failure() {
    let c = cfg();
    // the application writes to a Kafka that wants five in-sync replicas of the three it has: it never accepts a write
    let mut a = small(3, None);
    a.nodes.push(
        serde_json::from_value(json!({ "id": "kafka", "kind": "kafka", "name": "Kafka", "zoneId": "dc1", "group": "kafka", "props": { "replicas": 3, "replicationFactor": 3, "minInsync": 5 } }))
            .unwrap(),
    );
    a.edges.push(serde_json::from_value(json!({ "id": "e2", "source": "app", "target": "kafka", "type": "traffic" })).unwrap());
    let rep = evaluate(&a, &c, &[]);
    let av = rep.availability.as_ref().unwrap();
    assert!(av.service_down);
    assert_eq!(av.estimate, 0.0);
    assert!(av.top.is_empty() && av.causes.is_empty());
    assert_eq!(av.units.first().map(|u| u.name.as_str()), Some("Kafka"));
    let f = rep.findings.iter().find(|f| f.rule_id == "AV-SLO-01").unwrap();
    assert_eq!(f.status, Status::Critical);
    let rule = c.rules.iter().find(|r| r.id == "AV-SLO-01").unwrap();
    let text = render_message(rule, f, "en");
    assert!(text.contains("does not work even while nothing has failed") && text.contains("Kafka") && !text.contains('{'), "{text}");

    // with the setting fixed the same design is estimated like any other
    a.nodes.iter_mut().find(|n| n.id == "kafka").unwrap().props.insert("minInsync".into(), json!(2));
    let ok = avail(&a, &c);
    assert!(!ok.service_down && ok.estimate > 0.99);
}

#[test]
fn failures_that_were_not_looked_at_do_not_pass_for_a_green_estimate() {
    let mut c = cfg();
    let a = template("two-dc-witness");
    assert_eq!(rule_status(&evaluate(&a, &c, &[]), "AV-SLO-01"), Status::Ok);
    // the scenario cap cuts the sites, links and disasters the estimate is built from
    c.settings.scenarios.max_count = 3;
    let rep = evaluate(&a, &c, &[]);
    assert!(rep.availability.as_ref().unwrap().truncated);
    assert_eq!(rule_status(&rep, "AV-SLO-01"), Status::NoData);
    let tile = rep.objectives.iter().find(|o| o.id == "availability").unwrap();
    assert_eq!(tile.status, Status::NoData);
}

#[test]
fn the_report_carries_the_assumptions_the_number_rests_on() {
    let c = cfg();
    let av = avail(&template("two-dc-witness"), &c);
    assert_eq!(av.basis.failures.instance, c.settings.reliability.failures.instance);
    assert_eq!(av.basis.provider.default, c.settings.reliability.provider.default);
    assert_eq!(av.basis.warn_percent, 80.0);
    assert!(av.scenarios > 10);
}

#[test]
fn a_design_that_survives_a_site_pays_little_for_losing_one() {
    let c = cfg();
    let site = |av: &Availability| av.causes.iter().find(|x| x.cause == Cause::Site).map(|x| x.seconds_per_year).unwrap_or(0.0);
    let two = avail(&template("two-dc-witness"), &c);
    let one = avail(&small(3, None), &c);
    // one data center that goes down is down until it is repaired; with a second one the service only blinks
    assert!(site(&one) > 3_000.0, "{}", site(&one));
    assert!(site(&two) < site(&one) / 10.0, "{} vs {}", site(&two), site(&one));
}

#[test]
fn what_the_defaults_say_is_what_a_configuration_without_the_block_gets() {
    // a configuration stored before the estimate existed has no `reliability` block: it must get the shipped rates and tables, not an
    // empty set that would leave every site, rack and provider out of the estimate
    let shipped = defaults::engine_settings().reliability;
    assert!(!shipped.failures.zone.is_empty() && !shipped.provider.by_function.is_empty() && !shipped.tier_factor.is_empty() && !shipped.presets.is_empty());
    let without: engine::config::EngineSettings = serde_json::from_value(json!({ "scoring": {} })).unwrap();
    assert_eq!(serde_json::to_value(&without.reliability).unwrap(), serde_json::to_value(&shipped).unwrap());
    assert!(!without.reliability.failures.zone.is_empty());
    // a block that lacks keys takes them from the plain numbers, never from nowhere
    let partial: engine::config::EngineSettings = serde_json::from_value(json!({ "reliability": { "failures": { "instance": 5 } } })).unwrap();
    assert_eq!(partial.reliability.failures.instance, 5.0);
    assert_eq!(partial.reliability.days_per_year, 365.25);
    assert!(partial.reliability.enabled);
    // the shipped file lists every key the settings have, so nothing falls back silently (and it says the same as the defaults)
    let file: serde_json::Value = serde_json::from_str(engine::defaults::ENGINE_JSON).unwrap();
    let keys = |v: &serde_json::Value| -> Vec<String> { v.as_object().map(|o| o.keys().cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect()).unwrap_or_default() };
    let now = serde_json::to_value(&shipped).unwrap();
    assert_eq!(keys(&file["reliability"]), keys(&now));
    for part in ["failures", "repair", "provider"] {
        assert_eq!(keys(&file["reliability"][part]), keys(&now[part]), "{part}");
    }
    let parsed: engine::config::ReliabilitySettings = serde_json::from_value(file["reliability"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), now);
}
