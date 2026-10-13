//! What to improve first, on the real engine: the fixes are tried, the effect is what evaluating the result says, and a fix that
//! does not make the design better is never recommended.

use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::{evaluate, Status};
use engine::model::Architecture;
use engine::ops::apply_ops;
use engine::recommend::{effect, improves, recommend};
use serde_json::json;

fn cfg() -> EngineConfig {
    defaults::engine_config("en")
}

fn template(id: &str) -> Architecture {
    defaults::templates().into_iter().find(|t| t.id == id).unwrap().architecture
}

/// A design for two data centers whose entry points run as a single instance each: single points of failure with a fix apiece.
fn weak() -> Architecture {
    let mut a = template("two-dc-witness");
    for n in a.nodes.iter_mut().filter(|n| n.id == "gslb" || n.id == "waf") {
        n.props.insert("replicas".into(), json!(1));
    }
    a
}

#[test]
fn every_recommendation_is_a_fix_that_makes_the_design_better_and_its_effect_is_what_evaluating_it_says() {
    let c = cfg();
    let a = weak();
    let base = evaluate(&a, &c, &[]);
    let plan = recommend(&a, &c, &[], &Default::default());
    assert!(plan.items.len() >= 2, "{:?}", plan.items.iter().map(|r| &r.id).collect::<Vec<_>>());
    for r in &plan.items {
        let out = apply_ops(&a, &r.ops, &c.catalog, &Default::default()).expect("the fix applies");
        let after = evaluate(&out.architecture, &c, &[]);
        assert!(improves(&after, &base), "{} does not make the design better", r.id);
        // the numbers shown are not estimates of an effect: they are the effect
        assert_eq!(serde_json::to_value(&r.effect).unwrap(), serde_json::to_value(effect(&base, &after)).unwrap(), "{}", r.id);
        assert_eq!(r.effect.score.before, base.score.value);
        assert!(r.effect.score.after >= r.effect.score.before || r.effect.critical.after < r.effect.critical.before);
        assert!(r.effect.introduced.is_empty() || r.effect.critical.after < r.effect.critical.before, "{} opens new findings without closing a critical one", r.id);
        assert!(!r.effect.resolved.is_empty(), "{} closes nothing", r.id);
        assert!(r.effect.resolved.contains(&r.finding_id) || r.effect.critical.after < r.effect.critical.before, "{}", r.id);
    }
}

#[test]
fn the_list_is_ordered_by_what_each_fix_closes_gains_and_costs() {
    let c = cfg();
    let plan = recommend(&weak(), &c, &[], &Default::default());
    let key = |r: &engine::recommend::Recommendation| (r.effect.critical.before as i64 - r.effect.critical.after as i64, r.effect.score.after - r.effect.score.before);
    for w in plan.items.windows(2) {
        let (a, b) = (key(&w[0]), key(&w[1]));
        assert!(a.0 > b.0 || (a.0 == b.0 && a.1 >= b.1 - 1e-9), "{:?} before {:?}", a, b);
    }
    // the fix for the single entry point that costs availability comes first: it also closes the availability finding
    assert_eq!(plan.items[0].id, "set_replicas:gslb:replicas");
    let first = &plan.items[0].effect;
    let av = first.availability.expect("an availability estimate on both sides");
    assert!(av.after > av.before, "{av:?}");
    assert!(first.downtime_seconds.unwrap().after < first.downtime_seconds.unwrap().before);
    assert!(first.cost.after > first.cost.before, "an instance costs money and the effect says so");
}

#[test]
fn applying_them_all_is_one_step_that_ends_better_than_any_single_fix() {
    let c = cfg();
    let a = weak();
    let base = evaluate(&a, &c, &[]);
    let plan = recommend(&a, &c, &[], &Default::default());
    let bundle = plan.bundle.as_ref().expect("two fixes make a bundle");
    assert_eq!(bundle.ids.len(), plan.items.len());
    let out = apply_ops(&a, &bundle.ops, &c.catalog, &Default::default()).unwrap();
    let after = evaluate(&out.architecture, &c, &[]);
    assert_eq!(serde_json::to_value(&bundle.effect).unwrap(), serde_json::to_value(effect(&base, &after)).unwrap());
    assert!(plan.items.iter().all(|r| bundle.effect.score.after >= r.effect.score.after));
    assert!(bundle.effect.critical.after <= plan.items.iter().map(|r| r.effect.critical.after).min().unwrap());
    // a single recommendation is not a bundle
    let mut one = c.clone();
    one.settings.recommendations.kinds = vec!["set_replicas".into()];
    one.settings.recommendations.max_candidates = 1;
    let plan = recommend(&a, &one, &[], &Default::default());
    assert_eq!(plan.items.len(), 1);
    assert!(plan.bundle.is_none());
    assert!(plan.truncated && plan.considered == 1);
}

#[test]
fn a_sound_design_has_nothing_to_recommend_and_findings_without_a_fix_are_left_to_a_person() {
    let c = cfg();
    let plan = recommend(&template("two-dc-witness"), &c, &[], &Default::default());
    assert!(plan.items.is_empty() && plan.bundle.is_none() && plan.manual.is_empty(), "{:?}", plan.manual);
    // Redis without a replica is a finding no mechanical fix settles
    let demo = defaults::demo_project().architecture;
    let plan = recommend(&demo, &c, &[], &Default::default());
    assert!(plan.manual.iter().any(|id| id.starts_with("HA-CACHE-01")), "{:?}", plan.manual);
    let report = evaluate(&demo, &c, &[]);
    assert!(plan.manual.iter().all(|id| report.findings.iter().any(|f| &f.id == id && matches!(f.status, Status::Critical | Status::Risk))));
}

#[test]
fn it_is_deterministic_and_follows_its_settings() {
    let c = cfg();
    let a = weak();
    let one = serde_json::to_string(&recommend(&a, &c, &[], &Default::default())).unwrap();
    let two = serde_json::to_string(&recommend(&a, &c, &[], &Default::default())).unwrap();
    assert_eq!(one, two);
    let mut off = c.clone();
    off.settings.recommendations.enabled = false;
    let plan = recommend(&a, &off, &[], &Default::default());
    assert!(plan.items.is_empty() && plan.considered == 0);
    // kinds the administrator did not allow are not tried
    let mut only = c.clone();
    only.settings.recommendations.kinds = vec!["add_backup".into()];
    assert!(recommend(&a, &only, &[], &Default::default()).items.is_empty());
}

#[test]
fn an_accepted_risk_is_not_recommended_away() {
    let c = cfg();
    let a = weak();
    let base = evaluate(&a, &c, &[]);
    let waivers: Vec<engine::config::Waiver> =
        base.findings.iter().filter(|f| f.rule_id == "HA-SPOF-01").map(|f| engine::config::Waiver { fingerprint: f.id.clone(), reason: "accepted".into(), by: None, at: None }).collect();
    let plan = recommend(&a, &c, &waivers, &Default::default());
    assert!(plan.items.iter().all(|r| r.rule_id != "HA-SPOF-01"), "{:?}", plan.items.iter().map(|r| &r.id).collect::<Vec<_>>());
}

#[test]
fn a_configuration_without_the_block_gets_the_shipped_numbers() {
    // a configuration stored before the recommendations existed has no `recommendations` block: it must work as shipped
    let shipped = defaults::engine_settings().recommendations;
    assert!(shipped.enabled && shipped.max_candidates > 0 && shipped.show > 0);
    let without: engine::config::EngineSettings = serde_json::from_value(json!({ "scoring": {} })).unwrap();
    assert_eq!(serde_json::to_value(&without.recommendations).unwrap(), serde_json::to_value(&shipped).unwrap());
    // a block that lacks keys takes the rest from the same numbers
    let partial: engine::config::EngineSettings = serde_json::from_value(json!({ "recommendations": { "show": 9 } })).unwrap();
    assert_eq!((partial.recommendations.show, partial.recommendations.max_candidates, partial.recommendations.enabled), (9, shipped.max_candidates, true));
    // the shipped file lists every key and says what the defaults say
    let file: serde_json::Value = serde_json::from_str(defaults::ENGINE_JSON).unwrap();
    let parsed: engine::config::RecommendSettings = serde_json::from_value(file["recommendations"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), serde_json::to_value(&shipped).unwrap());
    let keys = |v: &serde_json::Value| -> Vec<String> { v.as_object().map(|o| o.keys().cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect()).unwrap_or_default() };
    assert_eq!(keys(&file["recommendations"]), keys(&serde_json::to_value(&shipped).unwrap()));
}

#[test]
fn a_limit_on_the_fixes_tried_is_said_and_keeps_the_best_of_what_was_tried() {
    let c = cfg();
    let a = weak();
    let mut one = c.clone();
    one.settings.recommendations.max_candidates = 1;
    let plan = recommend(&a, &one, &[], &Default::default());
    assert!(plan.truncated && plan.considered == 1 && plan.items.len() == 1);
    // a finding whose fix was not tried is not claimed to be for a person only: nobody looked at it
    assert!(plan.manual.is_empty(), "{:?}", plan.manual);
    let full = recommend(&a, &c, &[], &Default::default());
    assert!(!full.truncated && full.considered >= 2 && full.manual.is_empty());
    // a kind that may not be recommended is not tried, and its findings are not called "for a person" either
    let mut none = c.clone();
    none.settings.recommendations.kinds = vec!["add_backup".into()];
    let plan = recommend(&a, &none, &[], &Default::default());
    assert!(plan.items.is_empty() && plan.manual.is_empty(), "{:?}", plan.manual);
}
