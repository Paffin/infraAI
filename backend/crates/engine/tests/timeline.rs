//! The story of a recovery: what the picture looks like at the moment of a failure and after every step that takes time.

use engine::config::EngineConfig;
use engine::defaults;
use engine::graph::Index;
use engine::model::Architecture;
use engine::sim::{simulate, simulate_timeline, EdgeState, NodeState, Scenario, ServiceState, SimResult};
use std::collections::BTreeMap;

fn cfg() -> EngineConfig {
    defaults::engine_config("en")
}

fn template(id: &str) -> Architecture {
    defaults::templates().into_iter().find(|t| t.id == id).unwrap().architecture
}

fn story(arch: &Architecture, sc: Scenario) -> SimResult {
    let c = cfg();
    let ix = Index::build(arch, &c.catalog, &c.settings);
    simulate_timeline(&ix, &sc)
}

/// The picture at one moment: the time and the state of every node and edge.
type Picture = (f64, BTreeMap<String, NodeState>, BTreeMap<String, EdgeState>);

/// The states of every node and edge in each frame, with the differences applied one after another.
fn replay(r: &SimResult) -> Vec<Picture> {
    let mut nodes = BTreeMap::new();
    let mut edges = BTreeMap::new();
    r.timeline
        .iter()
        .map(|f| {
            nodes.extend(f.nodes.iter().map(|(k, v)| (k.clone(), *v)));
            edges.extend(f.edges.iter().map(|(k, v)| (k.clone(), *v)));
            (f.at, nodes.clone(), edges.clone())
        })
        .collect()
}

#[test]
fn losing_a_data_center_is_a_story_that_ends_where_the_settled_result_does() {
    let a = template("two-dc-witness");
    let r = story(&a, Scenario::Zone { zone_id: "dc1".into() });
    assert!(r.timeline.len() >= 2, "a promotion takes time: {:?}", r.timeline.iter().map(|f| f.at).collect::<Vec<_>>());
    assert_eq!(r.timeline[0].at, 0.0);
    assert!(r.timeline.windows(2).all(|w| w[0].at < w[1].at), "frames are in time order, one per moment");

    // the first frame has everything, the last one is the settled picture
    let frames = replay(&r);
    assert_eq!(frames[0].1.len(), a.nodes.len());
    assert_eq!(frames[0].2.len(), a.edges.len());
    let last = frames.last().unwrap();
    assert_eq!(last.1, r.node_states, "the last frame equals the settled node states");
    assert_eq!(last.2, r.edge_states, "… and edge states");
    assert_eq!(r.timeline.last().unwrap().state, r.state);

    // nothing gets worse while the recovery runs
    assert!(r.timeline.windows(2).all(|w| w[1].state <= w[0].state), "service states: {:?}", r.timeline.iter().map(|f| f.state).collect::<Vec<_>>());
    assert!(r.timeline[0].state > r.timeline.last().unwrap().state || r.timeline[0].state == ServiceState::Down, "the moment of the failure is worse than the end of it");

    // every step that takes time is a frame, and a promoted replica is promoted exactly when its step is done
    for e in r.events.iter().filter(|e| e.at.map(|t| t > 0.0).unwrap_or(false)) {
        let t = e.at.unwrap();
        assert!(r.timeline.iter().any(|f| (f.at - t).abs() < 1e-9), "{} at {t} has a frame", e.code);
    }
    let promoted = r.events.iter().find(|e| e.code == "promoted").expect("PostgreSQL fails over by itself");
    let (node, at) = (promoted.node.clone().unwrap(), promoted.at.unwrap());
    let state_at = |t: f64| frames.iter().rev().find(|f| f.0 <= t + 1e-9).map(|f| f.1[&node]).unwrap();
    assert_eq!(state_at(at), NodeState::Promoted);
    assert_ne!(state_at(at - 0.001), NodeState::Promoted, "it is not the writer before the promotion is done");
}

#[test]
fn a_scenario_without_waiting_has_no_story() {
    let a = template("two-dc-witness");
    let r = story(&a, Scenario::None);
    assert!(r.timeline.is_empty());
    // and the plain simulation never carries one
    let c = cfg();
    let ix = Index::build(&a, &c.catalog, &c.settings);
    assert!(simulate(&ix, &Scenario::Zone { zone_id: "dc1".into() }).timeline.is_empty());
}

#[test]
fn a_manual_failover_takes_as_long_as_a_human() {
    let mut a = template("two-dc-witness");
    for n in a.nodes.iter_mut().filter(|n| n.kind == "postgresql") {
        n.props.insert("failover".into(), serde_json::json!("manual"));
    }
    let r = story(&a, Scenario::Zone { zone_id: "dc1".into() });
    let manual = r.events.iter().find(|e| e.code == "manual_failover").expect("an operator has to promote the replica");
    assert_eq!(manual.at, Some(cfg().settings.assumptions.manual_failover_seconds));
    assert!(r.timeline.iter().any(|f| Some(f.at) == manual.at), "the operator's step is a frame");
    let f0 = &r.timeline[0];
    assert!(f0.state > ServiceState::Up, "until somebody promotes the replica the service is not whole");
}

#[test]
fn until_the_traffic_is_rerouted_some_requests_still_go_to_the_lost_member() {
    let a = template("three-az-region");
    let c = cfg();
    let ix = Index::build(&a, &c.catalog, &c.settings);
    let mut seen = 0;
    for n in &a.nodes {
        let sc = Scenario::Node { node_id: n.id.clone() };
        let settled = simulate(&ix, &sc);
        let Some(reroute) = settled.events.iter().find(|e| e.code == "reroute") else { continue };
        if settled.state != ServiceState::Up {
            continue;
        }
        seen += 1;
        let r = simulate_timeline(&ix, &sc);
        let t = reroute.at.expect("the reroute is a step in time");
        assert!(t > 0.0);
        assert_eq!(r.timeline[0].state, ServiceState::Degraded, "{}: before health checks notice, requests fail", n.id);
        assert_eq!(r.timeline.last().unwrap().state, ServiceState::Up, "{}: after the reroute the service is whole", n.id);
    }
    assert!(seen > 0, "a node behind a balancer exists in the reference architecture (three AZ: any of the application nodes)");
}

#[test]
fn a_pilot_light_scales_out_when_its_time_comes() {
    let a = template("multi-region-pilot-light");
    let c = cfg();
    let ix = Index::build(&a, &c.catalog, &c.settings);
    // the primary region is lost: the standby region scales out
    let zones: Vec<&str> = a.zones.iter().filter(|z| z.zone_type == "region").map(|z| z.id.as_str()).collect();
    let mut found = false;
    for z in zones.iter().chain(a.zones.iter().filter(|z| z.zone_type == "dc" || z.zone_type == "az").map(|z| z.id.as_str()).collect::<Vec<_>>().iter()) {
        let r = simulate_timeline(&ix, &Scenario::Zone { zone_id: z.to_string() });
        let Some(scale) = r.events.iter().find(|e| e.code == "scale_out") else { continue };
        found = true;
        let t = scale.at.unwrap();
        let before = r.timeline.iter().rev().find(|f| f.at < t).expect("a frame before the scale-out");
        let after = r.timeline.iter().find(|f| (f.at - t).abs() < 1e-9).expect("a frame at the scale-out");
        // capacity can only improve once the instances are there
        if let (Some(b), Some(a)) = (before.capacity_ratio, after.capacity_ratio) {
            assert!(a >= b - 1e-9, "scale-out at {t}s: capacity {b} → {a}");
        }
        break;
    }
    assert!(found, "some failure of the pilot-light architecture triggers a scale-out");
}
