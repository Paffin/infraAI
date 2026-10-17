//! One validation gate: every code `validate` can say is said by a test that corrupts a shipped reference architecture in exactly one way;
//! numbers outside the range the catalog gives them are refused (or, when a stored design is only looked at, reported) instead of
//! being summed into a wrong availability; and whatever hostile numbers a form, a model or an old project carries, the engine
//! does not panic and does not invent a figure.

use engine::catalog::Catalog;
use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::evaluate;
use engine::graph::Index;
use engine::model::{validate, Architecture, Issue, IssueLevel, Node, ValidationLimits};
use engine::ops::{apply_ops, Op};
use engine::recommend::recommend;
use engine::sim::{simulate_timeline, Scenario};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

fn cfg() -> EngineConfig {
    defaults::engine_config("en")
}

fn template(id: &str) -> Architecture {
    defaults::templates().into_iter().find(|t| t.id == id).unwrap_or_else(|| panic!("template {id}")).architecture
}

fn base() -> Architecture {
    template("two-dc-witness")
}

fn node_mut<'a>(a: &'a mut Architecture, id: &str) -> &'a mut Node {
    a.nodes.iter_mut().find(|n| n.id == id).unwrap_or_else(|| panic!("node {id}"))
}

fn errors(issues: &[Issue]) -> BTreeSet<String> {
    issues.iter().filter(|i| i.level == IssueLevel::Error).map(|i| i.code.clone()).collect()
}

// ---------------------------------------------------------------------------------------------
// A test for every code
// ---------------------------------------------------------------------------------------------

/// Every code `validate` can say, with the test that says it. `codes_without_a_test_are_not_allowed` reads `model.rs` and fails when a code is
/// said there and listed nowhere in this file.
const TESTED: &[&str] = &[
    "bad_id",
    "dangling_edge",
    "dangling_parent",
    "dangling_zone",
    "duplicate_id",
    "limit_edges",
    "limit_nodes",
    "limit_zones",
    "prop_out_of_range",
    "prop_type",
    "requirement_out_of_range",
    "self_loop",
    "unknown_edge_type",
    "unknown_kind",
    "unknown_role",
    "unknown_unit_ref",
    "unknown_variant",
    "unknown_zone_type",
    "zone_cycle",
];

/// A shipped reference architecture corrupted in exactly one way must say `code` at `level` and nothing else that is an error, apart from what
/// that corruption necessarily brings with it (`also`). The architecture before it says none of them.
macro_rules! says {
    ($name:ident, $code:literal, $level:expr, also: [$($also:literal),*], limits: $lim:expr, |$a:ident| $corrupt:block) => {
        #[test]
        fn $name() {
            let c = cfg();
            let healthy = base();
            let lim: ValidationLimits = $lim;
            assert!(validate(&healthy, &c.catalog, &ValidationLimits::default()).is_empty(), "the reference architecture is clean");
            let mut $a = healthy.clone();
            $corrupt
            assert_ne!($a, healthy, "the test corrupts nothing");
            let issues = validate(&$a, &c.catalog, &lim);
            let said: Vec<&Issue> = issues.iter().filter(|i| i.code == $code).collect();
            assert!(!said.is_empty(), "{} was not said: {:?}", $code, issues);
            assert!(said.iter().all(|i| i.level == $level), "{} at the wrong level: {:?}", $code, said);
            assert!(said.iter().all(|i| !i.message.is_empty()), "an issue explains itself");
            let allowed: BTreeSet<String> = [$code $(, $also)*].iter().map(|s| s.to_string()).collect();
            let unexpected: Vec<String> = errors(&issues).difference(&allowed).cloned().collect();
            assert!(unexpected.is_empty(), "this corruption also brought errors {:?}: {:?}", unexpected, issues);
        }
    };
    ($name:ident, $code:literal, $level:expr, also: [$($also:literal),*], |$a:ident| $corrupt:block) => {
        says!($name, $code, $level, also: [$($also),*], limits: ValidationLimits::default(), |$a| $corrupt);
    };
}

use IssueLevel::{Error, Warning};

says!(bad_id, "bad_id", Error, also: [], |a| { a.edges[0].id = "an id with spaces".into(); });
says!(dangling_edge, "dangling_edge", Error, also: [], |a| { a.edges[0].target = "nowhere".into(); });
says!(dangling_parent, "dangling_parent", Error, also: [], |a| { a.zones[1].parent_id = Some("nowhere".into()); });
says!(dangling_zone, "dangling_zone", Error, also: [], |a| { node_mut(&mut a, "app-dc1").zone_id = Some("nowhere".into()); });
says!(duplicate_id, "duplicate_id", Error, also: [], |a| { a.edges[1].id = a.edges[0].id.clone(); });
says!(self_loop, "self_loop", Error, also: [], |a| { a.edges[0].target = a.edges[0].source.clone(); });
says!(unknown_edge_type, "unknown_edge_type", Error, also: [], |a| { a.edges[0].edge_type = "carrier-pigeon".into(); });
says!(unknown_kind, "unknown_kind", Error, also: [], |a| { node_mut(&mut a, "app-dc1").kind = "no-such-product".into(); });
says!(unknown_zone_type, "unknown_zone_type", Error, also: [], |a| { a.zones[0].zone_type = "no-such-zone".into(); });
says!(unknown_unit_ref, "unknown_unit_ref", Error, also: [], |a| { node_mut(&mut a, "pg-primary").props.insert("dcs".into(), json!("no-such-cluster")); });
says!(unknown_variant, "unknown_variant", Warning, also: [], |a| { node_mut(&mut a, "pg-primary").props.insert("variant".into(), json!("no-such-way-to-deploy")); });
says!(unknown_role, "unknown_role", Warning, also: [], |a| { node_mut(&mut a, "pg-primary").role = Some("no-such-role".into()); });
says!(zone_cycle, "zone_cycle", Error, also: [], |a| {
    a.zones[0].parent_id = Some(a.zones[1].id.clone());
    a.zones[1].parent_id = Some(a.zones[0].id.clone());
});
// the limit is the corruption; the design is only renamed so that it is not the shipped one
says!(limit_nodes, "limit_nodes", Error, also: [], limits: ValidationLimits { max_nodes: 3, ..Default::default() }, |a| { a.meta.name = Some("too many nodes".into()); });
says!(limit_edges, "limit_edges", Error, also: [], limits: ValidationLimits { max_edges: 3, ..Default::default() }, |a| { a.meta.name = Some("too many edges".into()); });
says!(limit_zones, "limit_zones", Error, also: [], limits: ValidationLimits { max_zones: 1, ..Default::default() }, |a| { a.meta.name = Some("too many zones".into()); });
says!(prop_out_of_range, "prop_out_of_range", Error, also: [], |a| { node_mut(&mut a, "app-dc1").props.insert("replicas".into(), json!(1_000_000_000_000u64)); });
says!(prop_type, "prop_type", Error, also: [], |a| { node_mut(&mut a, "app-dc1").props.insert("replicas".into(), json!("many")); });
says!(requirement_out_of_range, "requirement_out_of_range", Error, also: [], |a| { a.requirements.availability_slo = Some(150.0); });

#[test]
fn a_number_out_of_range_is_a_warning_when_a_stored_design_is_only_looked_at() {
    let c = cfg();
    let mut a = base();
    node_mut(&mut a, "app-dc1").props.insert("replicas".into(), json!(1_000_000_000_000u64));
    a.requirements.availability_slo = Some(150.0);
    a.edges[4].props.insert("fanout".into(), json!(1e308));
    let strict = validate(&a, &c.catalog, &ValidationLimits::default());
    let lenient = validate(&a, &c.catalog, &ValidationLimits::default().lenient());
    for code in ["prop_out_of_range", "requirement_out_of_range"] {
        assert!(strict.iter().any(|i| i.code == code && i.level == Error), "{code} is an error where it is written: {strict:?}");
        assert!(lenient.iter().any(|i| i.code == code && i.level == Warning), "{code} is a warning where it is looked at: {lenient:?}");
    }
    // the edge's prop is checked like the node's, and so is a zone's
    assert!(lenient.iter().any(|i| i.code == "prop_out_of_range" && i.subject.as_deref() == Some(a.edges[4].id.as_str())), "{lenient:?}");
    assert!(errors(&lenient).is_empty(), "nothing else is wrong: {lenient:?}");
    // a flag that is not a flag is a type error, never softened: there is no value to count at
    node_mut(&mut a, "kafka-dc1").props.insert("encryptionAtRest".into(), json!("maybe"));
    assert!(validate(&a, &c.catalog, &ValidationLimits::default().lenient()).iter().any(|i| i.code == "prop_type" && i.level == Error));
}

#[test]
fn the_issue_names_the_node_the_prop_and_the_range() {
    let c = cfg();
    let mut a = base();
    node_mut(&mut a, "app-dc1").props.insert("replicas".into(), json!(1_000_000_000_000u64));
    let i = validate(&a, &c.catalog, &ValidationLimits::default()).into_iter().find(|i| i.code == "prop_out_of_range").expect("said");
    assert_eq!(i.subject.as_deref(), Some("app-dc1"));
    for part in ["app-dc1", "replicas", "1000000000000", "1 to 100000"] {
        assert!(i.message.contains(part), "'{part}' is missing in: {}", i.message);
    }
    let mut r = base();
    r.requirements.growth_percent = Some(-500.0);
    let i = validate(&r, &c.catalog, &ValidationLimits::default()).into_iter().find(|i| i.code == "requirement_out_of_range").expect("said");
    assert_eq!(i.subject.as_deref(), Some("requirements.growthPercent"));
    assert!(i.message.contains("growthPercent") && i.message.contains("-500") && i.message.contains("above -100"), "{}", i.message);
}

#[test]
fn numbers_sent_as_text_are_numbers_and_what_is_not_set_is_not_wrong() {
    let c = cfg();
    let mut a = base();
    let n = node_mut(&mut a, "kafka-dc1");
    n.props.insert("replicas".into(), json!("3,5"));
    n.props.insert("vcpu".into(), json!(" 2 "));
    n.props.insert("ramGb".into(), Value::Null);
    n.props.insert("diskGb".into(), json!(""));
    n.props.insert("encryptionAtRest".into(), json!("да"));
    assert!(validate(&a, &c.catalog, &ValidationLimits::default()).is_empty());
    // "inf" and "NaN" are numbers to Rust's parser and to nobody else
    for word in ["inf", "-inf", "NaN", "infinity", "1e999"] {
        node_mut(&mut a, "kafka-dc1").props.insert("vcpu".into(), json!(word));
        assert!(validate(&a, &c.catalog, &ValidationLimits::default()).iter().any(|i| i.code == "prop_type"), "{word}");
    }
    // a prop the catalog does not describe is left alone: the model may carry notes of its own
    node_mut(&mut a, "kafka-dc1").props.insert("vcpu".into(), json!(2));
    node_mut(&mut a, "kafka-dc1").props.insert("owner".into(), json!({"team": "payments", "deep": [1, 2, {"x": null}]}));
    assert!(validate(&a, &c.catalog, &ValidationLimits::default()).is_empty());
}

#[test]
fn every_requirement_has_its_domain_and_only_the_ceilings_are_settings() {
    let c = cfg();
    let check = |set: &dyn Fn(&mut Architecture), want: bool| {
        let mut a = base();
        set(&mut a);
        let said = validate(&a, &c.catalog, &ValidationLimits::default()).iter().any(|i| i.code == "requirement_out_of_range");
        assert_eq!(said, want, "{:?}", a.requirements);
    };
    // availability: above 0 and at most 100
    check(&|a| a.requirements.availability_slo = Some(0.0), true);
    check(&|a| a.requirements.availability_slo = Some(-1.0), true);
    check(&|a| a.requirements.availability_slo = Some(100.0), false);
    check(&|a| a.requirements.availability_slo = Some(100.01), true);
    check(&|a| a.requirements.availability_slo = Some(99.95), false);
    // times and money: zero (no data may be lost) is a value, below it is not
    for set in [
        (|a: &mut Architecture, v| a.requirements.rto_seconds = Some(v)) as fn(&mut Architecture, f64),
        |a, v| a.requirements.rpo_seconds = Some(v),
        |a, v| a.requirements.budget_monthly = Some(v),
        |a, v| a.requirements.rps_peak = Some(v),
    ] {
        check(&|a| set(a, 0.0), false);
        check(&|a| set(a, -0.001), true);
        check(&|a| set(a, 1e308), true);
    }
    // growth: a load cannot shrink by a hundred percent or more
    check(&|a| a.requirements.growth_percent = Some(-100.0), true);
    check(&|a| a.requirements.growth_percent = Some(-99.9), false);
    check(&|a| a.requirements.growth_percent = Some(10_000.0), false);
    check(&|a| a.requirements.growth_percent = Some(10_001.0), true);
    // the ceilings are the engine's settings
    let mut lim = ValidationLimits::default();
    lim.numbers.max_rps_peak = 100.0;
    let mut a = base();
    a.requirements.rps_peak = Some(5000.0);
    assert!(validate(&a, &c.catalog, &lim).iter().any(|i| i.code == "requirement_out_of_range"));
    assert!(validate(&a, &c.catalog, &ValidationLimits::default()).is_empty());
}

// ---------------------------------------------------------------------------------------------
// The shipped data passes its own gate
// ---------------------------------------------------------------------------------------------

#[test]
fn every_shipped_template_and_the_demo_project_pass_the_gate_that_refuses_numbers_out_of_range() {
    let c = cfg();
    let mut n = 0;
    for t in defaults::templates() {
        let issues = validate(&t.architecture, &c.catalog, &ValidationLimits::default());
        assert!(issues.is_empty(), "template {}: {:?}", t.id, issues);
        n += 1;
    }
    let demo = defaults::demo_project().architecture;
    let issues = validate(&demo, &c.catalog, &ValidationLimits::default());
    assert!(issues.iter().all(|i| i.level != Error), "the demo project: {issues:?}");
    assert!(!issues.iter().any(|i| matches!(i.code.as_str(), "prop_out_of_range" | "prop_type" | "requirement_out_of_range")), "{issues:?}");
    assert!(n >= 20, "{n} templates");
}

#[test]
fn the_catalog_that_ships_is_consistent_about_its_own_ranges() {
    let c = defaults::catalog();
    assert!(c.validate().is_empty(), "{:?}", c.validate());
}

#[test]
fn a_kind_that_restates_a_prop_keeps_the_bound_it_does_not_say() {
    let cat = defaults::catalog();
    // 662 kinds describe their own `replicas` (default, help) with a minimum and no maximum: the bound written once for all kinds holds for them too
    let own = cat.kind("kubernetes").unwrap().props.iter().find(|p| p.key == "replicas").expect("kubernetes describes its replicas");
    assert_eq!((own.min, own.max), (Some(1.0), None), "the premise: the kind's own definition has no maximum");
    assert_eq!(cat.prop_bounds("kubernetes", "replicas"), (Some(1.0), Some(100_000.0)));
    // every kind has a maximum for its instances, however it describes them
    for k in &cat.kinds {
        let (lo, hi) = cat.prop_bounds(&k.id, "replicas");
        assert_eq!((lo, hi), (Some(1.0), Some(100_000.0)), "{}", k.id);
    }
    // a bound the kind states itself wins, field by field
    let mut cat = cat;
    let k = cat.kinds.iter_mut().find(|k| k.id == "kubernetes").unwrap();
    k.props.iter_mut().find(|p| p.key == "replicas").unwrap().max = Some(50.0);
    assert_eq!(cat.prop_bounds("kubernetes", "replicas"), (Some(1.0), Some(50.0)));
    // an unknown kind and an unknown key have nothing but what is common
    assert_eq!(cat.prop_bounds("no-such-kind", "replicas"), (Some(1.0), Some(100_000.0)));
    assert_eq!(cat.prop_bounds("kubernetes", "no-such-key"), (None, None));
}

#[test]
fn a_catalog_with_a_range_that_allows_nothing_or_a_default_outside_it_is_reported() {
    let ok = |c: &Catalog| c.validate();
    let mut cat = defaults::catalog();
    assert!(ok(&cat).is_empty());
    cat.common_props.iter_mut().find(|p| p.key == "replicas").unwrap().max = Some(0.0);
    let said = ok(&cat);
    assert!(said.iter().any(|m| m.contains("common prop 'replicas'") && m.contains("min 1 above max 0")), "{said:?}");
    let mut cat = defaults::catalog();
    cat.kinds.iter_mut().find(|k| k.id == "redis").unwrap().props.push(engine::catalog::PropDef {
        key: "shards".into(),
        prop_type: engine::catalog::PropType::Number,
        min: Some(1.0),
        max: Some(8.0),
        default: Some(json!(9)),
        ..Default::default()
    });
    let said = ok(&cat);
    assert!(said.iter().any(|m| m.contains("kind 'redis' prop 'shards' has default 9 outside its range")), "{said:?}");
    let mut cat = defaults::catalog();
    let k = cat.kinds.iter_mut().find(|k| k.id == "postgresql").unwrap();
    k.variants[0].defaults.insert("vcpu".into(), json!(-4));
    let said = ok(&cat);
    assert!(said.iter().any(|m| m.contains("variant") && m.contains("vcpu") && m.contains("outside")), "{said:?}");
}

// ---------------------------------------------------------------------------------------------
// The same gate for operations: all or nothing
// ---------------------------------------------------------------------------------------------

fn op(v: Value) -> Op {
    serde_json::from_value(v).unwrap()
}

#[test]
fn operations_that_would_make_a_code_are_refused_and_the_design_they_were_applied_to_is_not_touched() {
    let c = cfg();
    let a = base();
    let before = a.clone();
    let bad_node = |extra: Value| {
        let mut n = json!({"id": "new-node", "kind": "redis", "name": "New", "zoneId": "dc1", "props": {}});
        n.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        op(json!({"op": "add_node", "node": n}))
    };
    let table: Vec<(&str, Vec<Op>)> = vec![
        ("bad_id", vec![bad_node(json!({"id": "has a space"}))]),
        ("dangling_zone", vec![bad_node(json!({"zoneId": "nowhere"}))]),
        ("unknown_kind", vec![bad_node(json!({"kind": "no-such-product"}))]),
        ("unknown_unit_ref", vec![bad_node(json!({"kind": "postgresql", "props": {"dcs": "no-such-cluster"}}))]),
        ("dangling_edge", vec![op(json!({"op": "add_edge", "edge": {"id": "e-new", "source": "app-dc1", "target": "nowhere", "type": "traffic"}}))]),
        ("self_loop", vec![op(json!({"op": "add_edge", "edge": {"id": "e-new", "source": "app-dc1", "target": "app-dc1", "type": "traffic"}}))]),
        ("unknown_edge_type", vec![op(json!({"op": "add_edge", "edge": {"id": "e-new", "source": "app-dc1", "target": "app-dc2", "type": "carrier-pigeon"}}))]),
        ("unknown_zone_type", vec![op(json!({"op": "add_zone", "zone": {"id": "z-new", "name": "Z", "type": "no-such-zone"}}))]),
        ("dangling_parent", vec![op(json!({"op": "add_zone", "zone": {"id": "z-new", "name": "Z", "type": "rack", "parentId": "nowhere"}}))]),
        ("zone_cycle", vec![op(json!({"op": "update_zone", "id": "dc1", "set": {"parentId": "dc1"}}))]),
        ("prop_out_of_range", vec![op(json!({"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 1e12}}))]),
        ("prop_out_of_range", vec![op(json!({"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 0}}))]),
        ("prop_type", vec![op(json!({"op": "update_node", "id": "app-dc1", "set": {"props.replicas": "many"}}))]),
        ("requirement_out_of_range", vec![op(json!({"op": "set_requirements", "set": {"availabilitySlo": 150}}))]),
        ("op_failed", vec![op(json!({"op": "remove_node", "id": "no-such-node"}))]),
    ];
    for (code, ops) in table {
        let err = apply_ops(&a, &ops, &c.catalog, &ValidationLimits::default()).expect_err(code);
        assert!(err.issues.iter().any(|i| i.code == code && i.level == Error), "{code}: {:?}", err.issues);
        assert_eq!(a, before, "{code}: the design the operations were applied to changed");
    }
}

#[test]
fn a_batch_is_atomic_one_bad_operation_takes_the_good_ones_down_with_it() {
    let c = cfg();
    let a = base();
    let before = a.clone();
    let good = op(json!({"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 40}}));
    for bad in [
        op(json!({"op": "remove_node", "id": "no-such-node"})),
        op(json!({"op": "add_edge", "edge": {"id": "e-new", "source": "app-dc1", "target": "nowhere", "type": "traffic"}})),
        op(json!({"op": "update_node", "id": "app-dc2", "set": {"props.replicas": 1e12}})),
    ] {
        let r = apply_ops(&a, &[good.clone(), bad, good.clone()], &c.catalog, &ValidationLimits::default());
        assert!(r.is_err());
        assert_eq!(a, before, "nothing of a refused batch is applied");
    }
    // and the good operation alone is applied
    let out = apply_ops(&a, &[good], &c.catalog, &ValidationLimits::default()).unwrap();
    assert_eq!(out.architecture.node("app-dc1").unwrap().props["replicas"], json!(40));
    assert_eq!(a, before);
}

#[test]
fn a_colliding_id_is_renamed_and_a_warning_does_not_stop_a_batch() {
    let c = cfg();
    let a = base();
    // `duplicate_id` is the code for what arrives in a document; operations rename a collision instead of failing
    let out =
        apply_ops(&a, &[op(json!({"op": "add_node", "node": {"id": "app-dc1", "kind": "redis", "name": "Again", "zoneId": "dc1", "props": {}}}))], &c.catalog, &ValidationLimits::default()).unwrap();
    assert_eq!(out.renamed.get("app-dc1").map(String::as_str), Some("app-dc1-2"));
    assert!(validate(&out.architecture, &c.catalog, &ValidationLimits::default()).iter().all(|i| i.code != "duplicate_id"));
    // a variant the kind does not have is a warning of the design, not a reason to refuse the operation
    let out = apply_ops(&a, &[op(json!({"op": "update_node", "id": "pg-primary", "set": {"props.variant": "no-such-way", "role": "no-such-role"}}))], &c.catalog, &ValidationLimits::default());
    let out = out.expect("warnings do not fail an operation");
    let said = validate(&out.architecture, &c.catalog, &ValidationLimits::default());
    assert!(said.iter().any(|i| i.code == "unknown_variant" && i.level == Warning), "{said:?}");
}

#[test]
fn every_code_the_gate_says_has_a_test_here() {
    // the codes are read off the source of the gate itself, so a code added later cannot go without a test
    let src = include_str!("../src/model.rs");
    let mut said: BTreeSet<String> = BTreeSet::new();
    for opener in ["Issue::err(", "Issue::warn(", "Some(("] {
        for part in src.split(opener).skip(1) {
            // the code is the first argument, on the same line or the next
            let Some(quoted) = part.trim_start().strip_prefix('"') else { continue };
            if let Some(code) = quoted.split('"').next() {
                if code.chars().all(|c| c.is_ascii_lowercase() || c == '_') && !code.is_empty() {
                    said.insert(code.to_string());
                }
            }
        }
    }
    let tested: BTreeSet<String> = TESTED.iter().map(|s| s.to_string()).collect();
    assert_eq!(said.difference(&tested).collect::<Vec<_>>(), Vec::<&String>::new(), "said in model.rs and tested nowhere here");
    assert_eq!(tested.difference(&said).collect::<Vec<_>>(), Vec::<&String>::new(), "listed here and said nowhere");
    assert_eq!(said.len(), 19);
}

// ---------------------------------------------------------------------------------------------
// Hostile numbers
// ---------------------------------------------------------------------------------------------

/// A `serde` serializer that writes nothing and refuses a number that is not finite, saying where. `serde_json` writes a NaN or an infinity as
/// `null` without a word, so "every number is finite" cannot be asked of the JSON; it can be asked of the typed report before it becomes JSON.
mod finite {
    use serde::ser::{self, Serialize};
    use std::fmt;

    #[derive(Debug)]
    pub struct NotFinite(pub String);
    impl fmt::Display for NotFinite {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for NotFinite {}
    impl ser::Error for NotFinite {
        fn custom<T: fmt::Display>(m: T) -> Self {
            NotFinite(m.to_string())
        }
    }

    /// `Ok` when every number in `v` is finite; the error names the first that is not (`/report/capacity/*/loadRps = inf`).
    pub fn all_finite<T: Serialize + ?Sized>(v: &T) -> Result<(), NotFinite> {
        v.serialize(Check(String::new()))
    }

    pub struct Check(String);
    pub struct Inner(String);

    macro_rules! nothing {
        ($($f:ident($t:ty)),*) => { $(fn $f(self, _: $t) -> Result<(), NotFinite> { Ok(()) })* };
    }

    impl ser::Serializer for Check {
        type Ok = ();
        type Error = NotFinite;
        type SerializeSeq = Inner;
        type SerializeTuple = Inner;
        type SerializeTupleStruct = Inner;
        type SerializeTupleVariant = Inner;
        type SerializeMap = Inner;
        type SerializeStruct = Inner;
        type SerializeStructVariant = Inner;
        nothing!(
            serialize_bool(bool),
            serialize_i8(i8),
            serialize_i16(i16),
            serialize_i32(i32),
            serialize_i64(i64),
            serialize_u8(u8),
            serialize_u16(u16),
            serialize_u32(u32),
            serialize_u64(u64),
            serialize_char(char),
            serialize_str(&str),
            serialize_bytes(&[u8])
        );
        fn serialize_f32(self, v: f32) -> Result<(), NotFinite> {
            self.serialize_f64(v as f64)
        }
        fn serialize_f64(self, v: f64) -> Result<(), NotFinite> {
            if v.is_finite() {
                Ok(())
            } else {
                Err(NotFinite(format!("{} = {v}", self.0)))
            }
        }
        fn serialize_none(self) -> Result<(), NotFinite> {
            Ok(())
        }
        fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> Result<(), NotFinite> {
            v.serialize(self)
        }
        fn serialize_unit(self) -> Result<(), NotFinite> {
            Ok(())
        }
        fn serialize_unit_struct(self, _: &'static str) -> Result<(), NotFinite> {
            Ok(())
        }
        fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<(), NotFinite> {
            Ok(())
        }
        fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> Result<(), NotFinite> {
            v.serialize(self)
        }
        fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, _: &'static str, v: &T) -> Result<(), NotFinite> {
            v.serialize(self)
        }
        fn serialize_seq(self, _: Option<usize>) -> Result<Inner, NotFinite> {
            Ok(Inner(self.0))
        }
        fn serialize_tuple(self, _: usize) -> Result<Inner, NotFinite> {
            Ok(Inner(self.0))
        }
        fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Inner, NotFinite> {
            Ok(Inner(self.0))
        }
        fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Inner, NotFinite> {
            Ok(Inner(self.0))
        }
        fn serialize_map(self, _: Option<usize>) -> Result<Inner, NotFinite> {
            Ok(Inner(self.0))
        }
        fn serialize_struct(self, _: &'static str, _: usize) -> Result<Inner, NotFinite> {
            Ok(Inner(self.0))
        }
        fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Inner, NotFinite> {
            Ok(Inner(self.0))
        }
    }

    impl Inner {
        fn at<T: Serialize + ?Sized>(&self, step: &str, v: &T) -> Result<(), NotFinite> {
            v.serialize(Check(format!("{}/{step}", self.0)))
        }
    }
    impl ser::SerializeSeq for Inner {
        type Ok = ();
        type Error = NotFinite;
        fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), NotFinite> {
            self.at("*", v)
        }
        fn end(self) -> Result<(), NotFinite> {
            Ok(())
        }
    }
    impl ser::SerializeTuple for Inner {
        type Ok = ();
        type Error = NotFinite;
        fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), NotFinite> {
            self.at("*", v)
        }
        fn end(self) -> Result<(), NotFinite> {
            Ok(())
        }
    }
    impl ser::SerializeTupleStruct for Inner {
        type Ok = ();
        type Error = NotFinite;
        fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), NotFinite> {
            self.at("*", v)
        }
        fn end(self) -> Result<(), NotFinite> {
            Ok(())
        }
    }
    impl ser::SerializeTupleVariant for Inner {
        type Ok = ();
        type Error = NotFinite;
        fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), NotFinite> {
            self.at("*", v)
        }
        fn end(self) -> Result<(), NotFinite> {
            Ok(())
        }
    }
    impl ser::SerializeMap for Inner {
        type Ok = ();
        type Error = NotFinite;
        fn serialize_key<T: Serialize + ?Sized>(&mut self, _: &T) -> Result<(), NotFinite> {
            Ok(())
        }
        fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), NotFinite> {
            self.at("*", v)
        }
        fn end(self) -> Result<(), NotFinite> {
            Ok(())
        }
    }
    impl ser::SerializeStruct for Inner {
        type Ok = ();
        type Error = NotFinite;
        fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, v: &T) -> Result<(), NotFinite> {
            self.at(key, v)
        }
        fn end(self) -> Result<(), NotFinite> {
            Ok(())
        }
    }
    impl ser::SerializeStructVariant for Inner {
        type Ok = ();
        type Error = NotFinite;
        fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, v: &T) -> Result<(), NotFinite> {
            self.at(key, v)
        }
        fn end(self) -> Result<(), NotFinite> {
            Ok(())
        }
    }
}

/// Where the `vars` of a finding (free-form JSON, where a `NaN` has already become `null` when the variable was written) hold `null` (an array
/// index is `*`).
fn null_vars(v: &Value) -> BTreeSet<String> {
    fn walk(v: &Value, at: &str, out: &mut BTreeSet<String>) {
        match v {
            Value::Null if at.contains("/vars/") => {
                out.insert(at.to_string());
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, &format!("{at}/*"), out)),
            Value::Object(o) => o.iter().for_each(|(k, x)| walk(x, &format!("{at}/{k}"), out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(v, "", &mut out);
    out
}

/// A variable that is `null` because there is nothing to say (an unknown time, an unknown loss), not because a number went wrong.
const UNKNOWN_BY_DESIGN: &[&str] = &["/vars/rpo", "/vars/seconds"];

/// Every kind of scenario there is, on the elements `a` has.
fn every_scenario(a: &Architecture) -> Vec<Scenario> {
    let node = a.nodes.iter().find(|n| n.zone_id.is_some()).map(|n| n.id.clone()).unwrap_or_default();
    let zones: Vec<String> = a.zones.iter().map(|z| z.id.clone()).collect();
    let z = |i: usize| zones.get(i % zones.len().max(1)).cloned().unwrap_or_default();
    let group = a.nodes.iter().find_map(|n| n.group.clone()).unwrap_or_default();
    vec![
        Scenario::None,
        Scenario::Instance { node_id: node.clone(), count: 1 },
        Scenario::Instance { node_id: node.clone(), count: u32::MAX },
        Scenario::Node { node_id: node },
        Scenario::Unit { unit: group },
        Scenario::Zone { zone_id: z(0) },
        Scenario::Link { a: z(0), b: z(1) },
        Scenario::Isolate { zone_ids: vec![z(0)] },
        Scenario::Sites { zone_ids: vec![z(0), z(1)] },
        Scenario::Geo { geo: "moscow".into() },
        Scenario::Combo { items: vec![Scenario::Zone { zone_id: z(0) }, Scenario::Zone { zone_id: z(1) }] },
    ]
}

struct Probe {
    issues: Vec<Issue>,
    /// The first number that is not finite in anything the engine answered with, if there is one.
    not_finite: Option<String>,
    /// Variables of findings that came out `null`.
    nulls: BTreeSet<String>,
}

/// Run everything the engine does with a design: validate, evaluate, simulate every kind of scenario, recommend, apply operations. A panic fails the test
/// (the tests are built with overflow checks, so a sum that would wrap in a release build panics here).
fn probe(a: &Architecture, with_recommendations: bool) -> Probe {
    let c = cfg();
    let issues = validate(a, &c.catalog, &ValidationLimits::default().lenient());
    let mut not_finite: Option<String> = None;
    let mut note = |what: &str, r: Result<(), finite::NotFinite>| {
        if let (Err(e), None) = (r, &not_finite) {
            not_finite = Some(format!("{what}{e}"));
        }
    };
    let report = evaluate(a, &c, &[]);
    note("report", finite::all_finite(&report));
    let mut nulls: BTreeSet<String> = null_vars(&serde_json::to_value(&report).unwrap()).into_iter().map(|p| format!("report{p}")).collect();
    let ix = Index::build(a, &c.catalog, &c.settings);
    for sc in every_scenario(a) {
        let r = simulate_timeline(&ix, &sc);
        let key = serde_json::to_value(&sc).unwrap()["type"].as_str().unwrap().to_string();
        note(&format!("sim[{key}]"), finite::all_finite(&r));
    }
    if with_recommendations {
        let plan = recommend(a, &c, &[], &ValidationLimits::default().lenient());
        note("plan", finite::all_finite(&plan));
        nulls.extend(null_vars(&serde_json::to_value(&plan).unwrap()).into_iter().map(|p| format!("plan{p}")));
    }
    // the preview path of the server: operations on a copy, then an evaluation of what they made
    if let Some(n) = a.nodes.iter().find(|n| n.kind == "kubernetes") {
        let set: Map<String, Value> = [("props.replicas".to_string(), json!(2))].into_iter().collect();
        if let Ok(out) = apply_ops(a, &[Op::UpdateNode { id: n.id.clone(), set }], &c.catalog, &ValidationLimits::default().lenient()) {
            let _ = evaluate(&out.architecture, &c, &[]);
        }
    }
    Probe { issues, not_finite, nulls }
}

fn declared(p: &Probe) -> bool {
    p.issues.iter().any(|i| matches!(i.code.as_str(), "prop_out_of_range" | "prop_type" | "requirement_out_of_range"))
}

fn structural(p: &Probe) -> bool {
    p.issues.iter().any(|i| i.level == Error)
}

#[test]
fn hostile_numbers_never_panic() {
    let healthy = base();
    let baseline = probe(&healthy, true);
    assert!(baseline.issues.is_empty());

    let huge = 1e308;
    let mut cases: Vec<(String, Architecture)> = vec![];
    let mut add = |label: String, f: &dyn Fn(&mut Architecture)| {
        let mut a = healthy.clone();
        f(&mut a);
        cases.push((label, a));
    };
    // instance counts: negative, fractional, past a u32, past anything, not numbers at all
    let counts: Vec<Value> = vec![
        json!(-1),
        json!(0),
        json!(0.5),
        json!(4e9),
        json!(4294967295u64),
        json!(4294967296u64),
        json!(1e12),
        json!(huge),
        json!("abc"),
        json!("NaN"),
        json!("inf"),
        json!(true),
        Value::Null,
        json!([1]),
        json!({"n": 1}),
    ];
    for v in &counts {
        for key in ["replicas", "failoverReplicas"] {
            let v = v.clone();
            add(format!("app-dc1 {key} = {v}"), &move |a| {
                node_mut(a, "app-dc1").props.insert(key.into(), v.clone());
            });
        }
    }
    // every instance count at once is the sum that wrapped
    add("every replicas = 4294967295".into(), &|a| {
        for n in a.nodes.iter_mut() {
            n.props.insert("replicas".into(), json!(4294967295u64));
        }
    });
    add("every replicas = 100000 (the most there is)".into(), &|a| {
        for n in a.nodes.iter_mut() {
            n.props.insert("replicas".into(), json!(100_000));
        }
    });
    // sizes and rates
    for key in ["vcpu", "ramGb", "diskGb", "rpsPerReplica", "scaleUpSeconds", "healthCheckSeconds", "failThreshold", "hpaMin", "hpaMax", "votes"] {
        for v in [json!(0), json!(-5), json!(huge), json!("abc")] {
            add(format!("app-dc1 {key} = {v}"), &|a| {
                node_mut(a, "app-dc1").props.insert(key.into(), v.clone());
            });
            add(format!("pg-primary {key} = {v}"), &|a| {
                node_mut(a, "pg-primary").props.insert(key.into(), v.clone());
            });
        }
    }
    // requirements
    for v in [-5.0, 0.0, huge, 1.7e308] {
        add(format!("rpsPeak = {v}"), &|a| a.requirements.rps_peak = Some(v));
    }
    for v in [-500.0, -100.0, huge] {
        add(format!("growthPercent = {v}"), &|a| a.requirements.growth_percent = Some(v));
    }
    for v in [-1.0, 0.0, 100.0, 150.0, huge] {
        add(format!("availabilitySlo = {v}"), &|a| a.requirements.availability_slo = Some(v));
    }
    for v in [-1.0, huge] {
        add(format!("rto = {v}"), &|a| a.requirements.rto_seconds = Some(v));
        add(format!("rpo = {v}"), &|a| a.requirements.rpo_seconds = Some(v));
        add(format!("budget = {v}"), &|a| a.requirements.budget_monthly = Some(v));
    }
    for v in [0u32, 4294967295] {
        add(format!("siteFailures = {v}"), &|a| a.requirements.site_failures = Some(v));
    }
    add("failureTolerance nonsense".into(), &|a| a.requirements.failure_tolerance = Some("nonsense".into()));
    // the traffic between the services
    for v in [json!(-1), json!(huge), json!(1e18), json!("abc"), json!(0)] {
        add(format!("fanout = {v}"), &|a| {
            for e in a.edges.iter_mut().filter(|e| e.edge_type == "traffic") {
                e.props.insert("fanout".into(), v.clone());
            }
        });
    }
    // the shape of the graph
    add("a zone that is its own parent".into(), &|a| a.zones[0].parent_id = Some(a.zones[0].id.clone()));
    add("a cycle of zones".into(), &|a| {
        a.zones[0].parent_id = Some(a.zones[1].id.clone());
        a.zones[1].parent_id = Some(a.zones[0].id.clone());
    });
    add("a self loop".into(), &|a| a.edges[3].target = a.edges[3].source.clone());
    add("a dangling edge".into(), &|a| a.edges[3].target = "nowhere".into());
    add("a dangling zone".into(), &|a| node_mut(a, "app-dc1").zone_id = Some("nowhere".into()));
    add("duplicate ids".into(), &|a| {
        let first = a.nodes[3].id.clone();
        a.nodes[4].id = first.clone();
        a.zones[1].id = a.zones[0].id.clone();
    });
    add("a node without anything".into(), &|a| {
        a.nodes.clear();
        a.edges.clear();
    });
    // size (the HTTP layer caps these; the engine must stand them anyway)
    add("500 levels of props".into(), &|a| {
        let mut deep = json!(1);
        for _ in 0..500 {
            deep = json!({ "x": deep });
        }
        node_mut(a, "app-dc1").props.insert("owner".into(), deep);
    });
    add("a 200 KB name".into(), &|a| node_mut(a, "app-dc1").name = "N".repeat(200 * 1024));
    add("a zone type that is not one".into(), &|a| a.zones[0].zone_type = "no-such-zone".into());

    assert!(cases.len() >= 140, "{} cases", cases.len());
    let mut holes: Vec<String> = vec![];
    for (label, a) in &cases {
        // the recommender tries the engine's fixes one by one: it is run on the cases that decide what the fixes are
        let heavy = label.contains("replicas") || label.contains("rpsPeak") || label.contains("growth") || label.contains("fanout") || label.contains("rpsPerReplica") || label.contains("vcpu");
        let p = probe(a, heavy);
        // a number the gate reports is the person's to put right; one it does not report must not bring a hole the healthy design has not
        if !declared(&p) && !structural(&p) {
            if let Some(at) = &p.not_finite {
                holes.push(format!("{label}: {at}"));
            }
            let new: Vec<&String> = p.nulls.difference(&baseline.nulls).filter(|path| !UNKNOWN_BY_DESIGN.iter().any(|u| path.ends_with(u))).collect();
            if !new.is_empty() {
                holes.push(format!("{label}: {new:?}"));
            }
        }
    }
    assert!(holes.is_empty(), "inputs the gate lets through and the engine answers with a number it cannot write:\n{}", holes.join("\n"));
    // the check can see: an infinity put into a report is found, and so is a variable that went null
    assert!(finite::all_finite(&json!({"a": [1.0]})).is_ok());
    #[derive(serde::Serialize)]
    struct Holder {
        ratios: Vec<Option<f64>>,
    }
    let err = finite::all_finite(&Holder { ratios: vec![Some(1.0), None, Some(f64::INFINITY)] }).unwrap_err();
    assert_eq!(err.0, "/ratios/* = inf");
    assert!(null_vars(&json!({"findings": [{"vars": {"percent": null}}]})).contains("/findings/*/vars/percent"));
}

#[test]
fn a_sum_of_instance_counts_stops_at_the_largest_count_instead_of_wrapping() {
    // in a release build an overflowing sum wraps without a word (the tests are built with overflow checks and would panic), so the helper is
    // asked directly: two nodes of 3 000 000 000 instances each (the count is taken as written when the limits allow it) are more than a u32
    let c = cfg();
    let mut a = base();
    let mut settings = c.settings.clone();
    settings.limits.max_replicas = u32::MAX;
    let mut cat = c.catalog.clone();
    for p in cat.common_props.iter_mut().filter(|p| p.key == "replicas") {
        p.max = None;
    }
    for k in cat.kinds.iter_mut() {
        for p in k.props.iter_mut().filter(|p| p.key == "replicas") {
            p.max = None;
        }
    }
    for id in ["app-dc1", "app-dc2"] {
        node_mut(&mut a, id).props.insert("replicas".into(), json!(3_000_000_000u64));
    }
    let ix = Index::build(&a, &cat, &settings);
    let (i, j) = (ix.node_ix["app-dc1"], ix.node_ix["app-dc2"]);
    assert_eq!((ix.replicas(i), ix.replicas(j)), (3_000_000_000, 3_000_000_000));
    assert_eq!(ix.instances([i, j]), u32::MAX);
    assert_eq!(ix.instances([i]), 3_000_000_000);
    assert_eq!(ix.instances(std::iter::empty()), 0);
    // and with the shipped limits the count itself is held to what a node may have
    let ix = Index::build(&a, &c.catalog, &c.settings);
    assert_eq!((ix.replicas(i), ix.most_instances(i)), (100_000, 100_000));
    assert_eq!(ix.instances([i, j]), 200_000);
}

#[test]
fn counts_are_whole_numbers_between_one_and_the_most_a_node_may_have() {
    let c = cfg();
    let count = |v: Value| {
        let mut a = base();
        node_mut(&mut a, "app-dc1").props.insert("replicas".into(), v);
        let ix = Index::build(&a, &c.catalog, &c.settings);
        ix.replicas(ix.node_ix["app-dc1"])
    };
    assert_eq!(count(json!(3)), 3);
    assert_eq!(count(json!(2.9)), 2, "a count is truncated, as it always was");
    assert_eq!(count(json!(0.4)), 1);
    assert_eq!(count(json!(-7)), 1);
    assert_eq!(count(json!("12")), 12);
    assert_eq!(count(json!("abc")), 1, "what cannot be read is the default of one");
    assert_eq!(count(json!("NaN")), 1);
    assert_eq!(count(json!(1e308)), 100_000);
    assert_eq!(count(json!(4294967295u64)), 100_000);
    // the engine's ceiling is a setting
    let mut settings = c.settings.clone();
    settings.limits.max_replicas = 10;
    let mut a = base();
    node_mut(&mut a, "app-dc1").props.insert("replicas".into(), json!(32));
    let ix = Index::build(&a, &c.catalog, &settings);
    assert_eq!(ix.replicas(ix.node_ix["app-dc1"]), 10);
    // a node does not scale out to more than it may have either
    node_mut(&mut a, "app-dc1").props.insert("failoverReplicas".into(), json!(1e12));
    let ix = Index::build(&a, &c.catalog, &c.settings);
    assert_eq!(ix.failover_replicas(ix.node_ix["app-dc1"]), Some(100_000));
}

#[test]
fn the_engine_does_not_offer_a_fix_it_would_refuse_itself() {
    // a load nothing can carry: the capacity fix would be a million instances; there is no fix, and no panic
    let c = cfg();
    let mut a = base();
    a.requirements.rps_peak = Some(1e308);
    let report = evaluate(&a, &c, &[]);
    let scale = report.findings.iter().flat_map(|f| f.fixes.iter()).filter(|x| x.kind == "scale" || x.kind == "set_replicas");
    for fix in scale {
        for op in &fix.ops {
            if let Op::UpdateNode { set, .. } = op {
                if let Some(n) = set.get("props.replicas").and_then(Value::as_f64) {
                    assert!(n <= 100_000.0, "a fix of {n} instances is one the gate refuses: {fix:?}");
                }
            }
        }
    }
    let plan = recommend(&a, &c, &[], &ValidationLimits::default());
    for item in &plan.items {
        assert!(apply_ops(&a, &item.ops, &c.catalog, &ValidationLimits::default().lenient()).is_ok());
    }
    // a load that is large but sane: the fixes that set the number of instances are taken by the strict gate as they are
    let mut b = base();
    b.requirements.rps_peak = Some(2_000_000.0);
    let lim = ValidationLimits::default();
    let mut seen = 0;
    for f in evaluate(&b, &c, &[]).findings.iter() {
        for fix in f.fixes.iter().filter(|x| x.kind == "scale" || x.kind == "set_replicas") {
            let out = apply_ops(&b, &fix.ops, &c.catalog, &lim).unwrap_or_else(|e| panic!("{}: {e}", fix.id));
            assert!(validate(&out.architecture, &c.catalog, &lim).iter().all(|i| i.level != Error));
            seen += 1;
        }
    }
    assert!(seen > 0, "the load asks for more instances: there are fixes to try");
}

// ---------------------------------------------------------------------------------------------
// The generator and the gate
// ---------------------------------------------------------------------------------------------

fn generate_for(rps: f64) -> engine::placement::PlacementResult {
    let c = cfg();
    let req = engine::placement::PlacementRequest {
        pattern: "n-sites".into(),
        sites: Some(3),
        lang: Some("en".into()),
        requirements: Some(engine::model::Requirements { rps_peak: Some(rps), ..Default::default() }),
        ..Default::default()
    };
    engine::placement::generate(&c, &c.placement, &req).unwrap_or_else(|e| panic!("rpsPeak {rps}: {e}"))
}

fn most_instances_of(r: &engine::placement::PlacementResult) -> u64 {
    r.architecture.nodes.iter().filter_map(|n| n.props.get("replicas").and_then(Value::as_u64)).max().unwrap_or(0)
}

#[test]
fn the_generator_sizes_for_the_load_up_to_what_a_node_may_have_and_says_when_it_stopped() {
    let c = cfg();
    let max = c.settings.limits.max_replicas as u64;
    // a big load that fits is sized as it is, and passes the strict gate as it is
    let big = generate_for(1e6);
    let n = most_instances_of(&big);
    assert!(n > 1000 && n <= max, "{n} instances for a million requests per second");
    assert!(!big.notes.iter().any(|x| x.code == "replicas_capped"), "{:?}", big.notes);
    assert!(validate(&big.architecture, &c.catalog, &ValidationLimits::default()).iter().all(|i| i.level != Error));
    // a load nothing carries: the design gets the most a node may have, with a note that says so
    let absurd = generate_for(1e9);
    assert_eq!(most_instances_of(&absurd), max);
    let note = absurd.notes.iter().find(|x| x.code == "replicas_capped").expect("capped, and said");
    assert_eq!(note.level, "warn");
    assert_eq!(note.vars["max"], json!(max));
    assert!(validate(&absurd.architecture, &c.catalog, &ValidationLimits::default()).iter().all(|i| i.level != Error), "the design is still one the gate takes");
    // and a load that is not a number anyone could mean: no panic, the same cap
    let nonsense = generate_for(1e308);
    assert_eq!(most_instances_of(&nonsense), max);
    let report = evaluate(&nonsense.architecture, &c, &[]);
    assert!(report.issues.iter().any(|i| i.code == "requirement_out_of_range" && i.level == Warning), "{:?}", report.issues);
}

// ---------------------------------------------------------------------------------------------
// A scenario names what the design has
// ---------------------------------------------------------------------------------------------

use engine::sim::{check_scenario, MissingRef, ScenarioFault};

fn asked(a: &Architecture, sc: Value) -> Result<(), ScenarioFault> {
    let c = cfg();
    let ix = Index::build(a, &c.catalog, &c.settings);
    check_scenario(&ix, &serde_json::from_value(sc).unwrap())
}

fn missing(kind: &'static str, id: &str) -> MissingRef {
    MissingRef { kind, id: id.into() }
}

#[test]
fn a_scenario_that_names_what_the_design_does_not_have_is_not_a_question_the_simulator_can_answer() {
    let a = base();
    let unknown = |sc: Value, refs: Vec<MissingRef>| assert_eq!(asked(&a, sc.clone()), Err(ScenarioFault::Unknown(refs)), "{sc}");
    unknown(json!({"type": "zone", "zoneId": "dc9"}), vec![missing("zone", "dc9")]);
    unknown(json!({"type": "node", "nodeId": "ghost"}), vec![missing("node", "ghost")]);
    unknown(json!({"type": "instance", "nodeId": "ghost", "count": 2}), vec![missing("node", "ghost")]);
    unknown(json!({"type": "unit", "unit": "no-such-cluster"}), vec![missing("unit", "no-such-cluster")]);
    unknown(json!({"type": "geo", "geo": "atlantis"}), vec![missing("geo", "atlantis")]);
    unknown(json!({"type": "link", "a": "dc1", "b": "dc9"}), vec![missing("zone", "dc9")]);
    unknown(json!({"type": "link", "a": "x", "b": "y"}), vec![missing("zone", "x"), missing("zone", "y")]);
    // a part of the list that is wrong is wrong, and all of what is wrong is said, once each
    unknown(json!({"type": "sites", "zoneIds": ["x1", "dc1", "x2", "x1"]}), vec![missing("zone", "x1"), missing("zone", "x2")]);
    unknown(json!({"type": "isolate", "zoneIds": ["dc1", "x2"]}), vec![missing("zone", "x2")]);
    // an id of the wrong kind of thing is an unknown id
    unknown(json!({"type": "zone", "zoneId": "app-dc1"}), vec![missing("zone", "app-dc1")]);
    unknown(json!({"type": "node", "nodeId": "dc1"}), vec![missing("node", "dc1")]);
    unknown(
        json!({"type": "combo", "items": [{"type": "zone", "zoneId": "dc1"}, {"type": "node", "nodeId": "ghost"}, {"type": "combo", "items": [{"type": "zone", "zoneId": "dc9"}]}]}),
        vec![missing("node", "ghost"), missing("zone", "dc9")],
    );

    let invalid = |sc: Value, why: &str| match asked(&a, sc.clone()) {
        Err(ScenarioFault::Invalid { reason, message }) => assert!(reason == why && !message.is_empty(), "{sc}: {reason} {message}"),
        other => panic!("{sc}: {other:?}"),
    };
    invalid(json!({"type": "isolate", "zoneIds": []}), "no_zones");
    invalid(json!({"type": "sites", "zoneIds": []}), "no_zones");
    invalid(json!({"type": "combo", "items": []}), "empty_combo");
    invalid(json!({"type": "link", "a": "dc1", "b": "dc1"}), "link_to_itself");
    invalid(json!({"type": "instance", "nodeId": "app-dc1", "count": 0}), "count_out_of_range");
    invalid(json!({"type": "instance", "nodeId": "app-dc1", "count": 100001}), "count_out_of_range");
    // the shape is judged before the names: a combination of nothing is not asked what it names
    invalid(json!({"type": "combo", "items": [{"type": "zone", "zoneId": "nowhere"}, {"type": "combo", "items": []}]}), "empty_combo");
    let items: Vec<Value> = (0..101).map(|_| json!({"type": "zone", "zoneId": "dc1"})).collect();
    invalid(json!({"type": "combo", "items": items}), "too_many_items");
    let mut deep = json!({"type": "zone", "zoneId": "dc1"});
    for _ in 0..5 {
        deep = json!({"type": "combo", "items": [deep]});
    }
    invalid(deep.clone(), "too_deep");
    // four levels are allowed (the numbers are settings: `scenarios.maxComboItems`, `maxComboDepth`)
    let Value::Object(mut outer) = deep else { panic!() };
    outer["items"] = outer["items"][0]["items"].clone();
    assert_eq!(asked(&a, Value::Object(outer)), Ok(()));
}

#[test]
fn what_is_real_is_accepted_at_its_edges() {
    let a = base();
    for sc in [
        json!({"type": "none"}),
        json!({"type": "instance", "nodeId": "app-dc1"}),
        json!({"type": "instance", "nodeId": "app-dc1", "count": 1}),
        json!({"type": "instance", "nodeId": "app-dc1", "count": 100000}),
        json!({"type": "node", "nodeId": "pg-primary"}),
        json!({"type": "unit", "unit": "pg"}),
        json!({"type": "unit", "unit": "gslb"}),
        json!({"type": "zone", "zoneId": "dc1"}),
        json!({"type": "link", "a": "dc1", "b": "site3"}),
        json!({"type": "isolate", "zoneIds": ["dc1"]}),
        json!({"type": "sites", "zoneIds": ["dc1", "dc2"]}),
        json!({"type": "geo", "geo": "moscow"}),
        json!({"type": "combo", "items": [{"type": "zone", "zoneId": "dc1"}, {"type": "instance", "nodeId": "app-dc2", "count": 3}]}),
    ] {
        assert_eq!(asked(&a, sc.clone()), Ok(()), "{sc}");
    }
    // the engine's ceiling on instances is the ceiling of a count of failed ones
    let c = cfg();
    let mut settings = c.settings.clone();
    settings.limits.max_replicas = 50;
    let ix = Index::build(&a, &c.catalog, &settings);
    let sc = |n: u32| Scenario::Instance { node_id: "app-dc1".into(), count: n };
    assert!(check_scenario(&ix, &sc(50)).is_ok() && check_scenario(&ix, &sc(51)).is_err());
}

#[test]
fn every_failure_the_engine_lists_for_a_design_is_one_it_accepts() {
    let c = cfg();
    let mut checked = 0;
    for t in defaults::templates() {
        let a = t.architecture;
        let ix = Index::build(&a, &c.catalog, &c.settings);
        let report = evaluate(&a, &c, &[]);
        assert!(!report.scenarios.is_empty(), "{}", t.id);
        for s in &report.scenarios {
            assert_eq!(check_scenario(&ix, &s.scenario), Ok(()), "template {}: the engine lists {:?}", t.id, s.scenario);
            checked += 1;
        }
        for sc in engine::sim::standard_scenarios(&ix) {
            assert_eq!(check_scenario(&ix, &sc), Ok(()), "template {}: {sc:?}", t.id);
        }
    }
    assert!(checked > 500, "{checked}");
}

#[test]
fn a_disaster_domain_is_named_the_way_the_engine_names_it() {
    // a zone with a `geo` prop is in the domain of that name; one without is in the domain named by the id of the region zone around it. The check is made
    // against the labels of the engine, not against the `geo` props, or the failures it lists itself (here: the loss of the whole region) would be refused
    let c = cfg();
    let mut a = template("aws-three-az");
    for z in a.zones.iter_mut() {
        z.props.remove("geo");
    }
    let ix = Index::build(&a, &c.catalog, &c.settings);
    let listed = engine::sim::standard_scenarios(&ix);
    assert!(listed.iter().any(|s| matches!(s, Scenario::Geo { geo } if geo == "region")), "the premise: the engine lists the loss of the region: {listed:?}");
    for s in &listed {
        assert_eq!(check_scenario(&ix, s), Ok(()), "{s:?}");
    }
    assert_eq!(asked(&a, json!({"type": "geo", "geo": "region"})), Ok(()));
    assert_eq!(asked(&a, json!({"type": "geo", "geo": "region-dr"})), Ok(()), "a region with nodes is a domain");
    assert_eq!(asked(&a, json!({"type": "geo", "geo": "azs"})), Err(ScenarioFault::Unknown(vec![missing("geo", "azs")])));
    // and a `geo` prop on a zone names the domain of what is in it, wherever the zone stands
    let mut b = base();
    assert_eq!(asked(&b, json!({"type": "geo", "geo": "moscow"})), Ok(()));
    b.zones[0].props.insert("geo".into(), json!("tver"));
    assert_eq!(asked(&b, json!({"type": "geo", "geo": "tver"})), Ok(()));
}
