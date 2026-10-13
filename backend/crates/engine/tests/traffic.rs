//! Where the traffic goes: in normal operation and while a failure is being recovered from. Real engine, real templates.

use engine::config::EngineConfig;
use engine::defaults;
use engine::graph::Index;
use engine::model::Architecture;
use engine::sim::{simulate, simulate_timeline, standard_scenarios, EdgeState, NodeState, Scenario, ServiceState, SimResult, Traffic};
use std::collections::BTreeMap;

fn cfg() -> EngineConfig {
    defaults::engine_config("en")
}

fn template(id: &str) -> Architecture {
    defaults::templates().into_iter().find(|t| t.id == id).unwrap().architecture
}

fn all_architectures() -> Vec<(String, Architecture)> {
    let mut v: Vec<(String, Architecture)> = defaults::templates().into_iter().map(|t| (t.id.clone(), t.architecture)).collect();
    v.push(("demo".into(), defaults::demo_project().architecture));
    v
}

fn story(arch: &Architecture, sc: Scenario) -> SimResult {
    let c = cfg();
    let ix = Index::build(arch, &c.catalog, &c.settings);
    simulate_timeline(&ix, &sc)
}

fn flow(t: &Traffic, edge: &str) -> f64 {
    t.flows.get(edge).copied().unwrap_or(0.0)
}

/// What was sent into the link and did not get through, whether or not the request survives it.
fn lost(t: &Traffic, edge: &str) -> f64 {
    t.lost.get(edge).copied().unwrap_or(0.0) + t.missed.get(edge).copied().unwrap_or(0.0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn in_normal_operation_every_link_carries_its_share_and_nothing_is_lost() {
    let a = template("two-dc-witness");
    let r = story(&a, Scenario::None);
    let t = r.traffic.as_ref().expect("the story carries the traffic");
    assert_eq!(r.traffic_before.as_ref(), Some(t), "with no failure the normal picture is the picture");
    assert!(near(t.served, 1.0));
    assert!(t.lost.is_empty(), "nothing is dropped: {:?}", t.lost);
    assert!(t.detours.is_empty(), "everything goes along the drawn links: {:?}", t.detours);
    // the load enters once and is split between the two sites
    assert!(near(flow(t, "e-users--gslb"), 1.0));
    assert!(near(flow(t, "e-gslb--waf"), 1.0));
    assert!(near(flow(t, "e-waf--ingress-dc1"), 0.5));
    assert!(near(flow(t, "e-waf--ingress-dc2"), 0.5));
    assert!(near(flow(t, "e-ingress-dc1--app-dc1"), 0.5));
    // a link carries the calls it makes: the application calls the database 0.3 times per request
    assert!(near(flow(t, "e-app-dc1--pg-primary"), 0.15));
    assert!(near(flow(t, "e-app-dc2--pg-replica"), 0.15), "the replica takes the connection and hands it to the writer");
    // links that are not dependencies (replication, quorum, backup, control) carry no requests
    assert_eq!(flow(t, "e-pg-primary--pg-replica"), 0.0);
    assert_eq!(flow(t, "e-gitops--app-dc1"), 0.0);
}

/// What arrives at each unit by the links: the links into it, the detours into it, and the entry itself.
fn arriving(arch: &Architecture, ix: &Index<'_>, t: &Traffic) -> BTreeMap<String, f64> {
    let mut arriving: BTreeMap<String, f64> = BTreeMap::new();
    for (ei, e) in arch.edges.iter().enumerate() {
        let Some((_, to)) = ix.edge_ends[ei] else { continue };
        *arriving.entry(ix.units[ix.unit_of[to]].key.clone()).or_default() += flow(t, &e.id) + lost(t, &e.id);
    }
    for d in &t.detours {
        let to = ix.node_ix[d.to.as_str()];
        *arriving.entry(ix.units[ix.unit_of[to]].key.clone()).or_default() += d.flow + d.lost;
    }
    for (u, unit) in ix.units.iter().enumerate() {
        if ix.unit_has_trait(u, "entry") {
            *arriving.entry(unit.key.clone()).or_default() += 1.0;
        }
    }
    arriving
}

#[test]
fn the_links_add_up_to_the_unit_loads_of_the_capacity_model_wherever_nothing_is_stalled() {
    let c = cfg();
    let mut compared = 0;
    for (name, arch) in all_architectures() {
        let Some(rps) = arch.requirements.rps_peak else { continue };
        let ix = Index::build(&arch, &c.catalog, &c.settings);
        let mut scenarios = vec![Scenario::None];
        scenarios.extend(standard_scenarios(&ix));
        for sc in scenarios {
            let settled = simulate(&ix, &sc);
            // an overloaded unit sheds at its door, which the capacity model does not (it reports the overload and goes on
            // counting the full load downstream): that difference is intended and has its own test. A member that is alive
            // but cannot serve passes the requests on, which the capacity model does not count either.
            let overloaded = settled.capacity_ratio.map(|r| r < 1.0).unwrap_or(false);
            if settled.state == ServiceState::Down || overloaded || settled.node_states.values().any(|s| matches!(s, NodeState::Stalled | NodeState::Isolated | NodeState::Fenced)) {
                continue;
            }
            let t = simulate_timeline(&ix, &sc).traffic.unwrap();
            let by_links = arriving(&arch, &ix, &t);
            for l in &settled.unit_loads {
                let mine = by_links.get(&l.unit).copied().unwrap_or(0.0) * rps;
                assert!((mine - l.load_rps).abs() <= l.load_rps.abs() * 1e-6 + 1e-6, "{name} {}: unit {} gets {mine} rps by the links but the capacity model says {}", sc.key(), l.unit, l.load_rps);
                compared += 1;
            }
        }
    }
    assert!(compared > 200, "the comparison covers plenty of units: {compared}");
}

#[test]
fn a_member_gets_the_share_the_capacity_model_gives_it_and_the_rest_of_its_sources_go_by_detours() {
    // Without ingress-dc1 the only ingress left is ingress-dc2, whose own link leads to the application of its site. The
    // capacity model gives both applications half of the load, so half of what ingress-dc2 sends goes to the other one, along
    // no drawn link.
    let a = template("two-dc-witness");
    let r = story(&a, Scenario::Node { node_id: "ingress-dc1".into() });
    let t = r.traffic.as_ref().unwrap();
    assert_eq!(flow(t, "e-ingress-dc1--app-dc1"), 0.0);
    assert!(near(flow(t, "e-ingress-dc2--app-dc2"), 0.5), "{:?}", t.flows);
    let d = t.detours.iter().find(|d| d.from == "ingress-dc2" && d.to == "app-dc1").expect("the other half goes to the site without an ingress");
    assert!(near(d.flow, 0.5));
    assert!(near(flow(t, "e-app-dc1--pg-primary"), 0.15), "and that application calls the database as usual");
    // a pilot light is idle: its share is small and the traffic of its balancer is spread over the active sites
    let p = template("multi-region-pilot-light");
    let r = story(&p, Scenario::None);
    let t = r.traffic.as_ref().unwrap();
    assert!(flow(t, "e-lb-dr--app-dr") < 0.1, "the standby application takes its share and no more: {:?}", t.flows);
    assert!(near(t.served, 1.0));
}

#[test]
fn no_link_carries_more_than_its_source_receives_times_its_fan_out() {
    let c = cfg();
    let mut checked = 0;
    for (name, arch) in all_architectures() {
        let ix = Index::build(&arch, &c.catalog, &c.settings);
        let mut scenarios = vec![Scenario::None];
        scenarios.extend(standard_scenarios(&ix));
        for sc in scenarios {
            let r = simulate_timeline(&ix, &sc);
            let mut pictures: Vec<&Traffic> = r.timeline.iter().filter_map(|f| f.traffic.as_ref()).collect();
            pictures.push(r.traffic.as_ref().unwrap());
            for t in pictures {
                // what arrives at each node: its links in, the detours in
                let mut inflow: BTreeMap<&str, f64> = BTreeMap::new();
                for e in &arch.edges {
                    *inflow.entry(e.target.as_str()).or_default() += flow(t, &e.id);
                }
                for d in &t.detours {
                    *inflow.entry(d.to.as_str()).or_default() += d.flow;
                }
                for (ei, e) in arch.edges.iter().enumerate() {
                    let Some((s, _)) = ix.edge_ends[ei] else { continue };
                    if ix.has_trait(s, "entry") {
                        continue;
                    }
                    let sent = flow(t, &e.id) + lost(t, &e.id);
                    if sent == 0.0 {
                        continue;
                    }
                    // the most the source can send over one link: everything it receives, times the largest fan-out it has
                    let fan = ix.out_edges[s].iter().map(|&o| ix.edge_eff[o].num("fanout").unwrap_or(1.0).max(ix.edge_eff[o].num("fanoutColdCache").unwrap_or(0.0))).fold(1.0, f64::max);
                    let got = inflow.get(e.source.as_str()).copied().unwrap_or(0.0);
                    // balancers keep sending to a pool they believe in, so what they send can exceed what reached them only by rounding
                    assert!(sent <= got * fan + 1e-6, "{name} {}: {} sends {sent} over {} but receives {got} (fan-out {fan})", sc.key(), e.source, e.id);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 5000, "the check covers plenty of links: {checked}");
}

#[test]
fn losing_a_data_center_traffic_dies_at_the_database_until_the_replica_is_promoted() {
    let a = template("two-dc-witness");
    let r = story(&a, Scenario::Zone { zone_id: "dc1".into() });
    let before = r.traffic_before.as_ref().unwrap();
    assert!(near(before.served, 1.0));
    let first = r.timeline[0].traffic.as_ref().unwrap();
    // the moment of the failure: the requests still travel to the application in the surviving site and die at the database
    assert_eq!(r.timeline[0].state, ServiceState::Down);
    assert!(near(first.served, 0.0), "down: nothing completes ({})", first.served);
    assert_eq!(flow(first, "e-waf--ingress-dc1"), 0.0, "the lost site takes nothing");
    assert!(near(flow(first, "e-waf--ingress-dc2"), 1.0), "the whole load goes to the surviving site");
    assert!(near(flow(first, "e-ingress-dc2--app-dc2"), 1.0));
    assert!(lost(first, "e-app-dc2--pg-replica") > 0.0, "nobody has promoted the replica yet, so the calls to it are lost");
    assert_eq!(flow(first, "e-app-dc2--pg-replica"), 0.0);
    // the database is a hard dependency: the requests fail with it. The cache is not: its calls go unanswered and the requests go on
    assert!(first.lost.contains_key("e-app-dc2--pg-replica") && !first.missed.contains_key("e-app-dc2--pg-replica"));
    assert!(first.missed.contains_key("e-app-dc2--redis-replica") && !first.lost.contains_key("e-app-dc2--redis-replica"), "{:?} / {:?}", first.lost, first.missed);

    // the promotion is done: the calls go through and the load is what the surviving site can bear
    let promoted = r.events.iter().find(|e| e.code == "promoted").unwrap().at.unwrap();
    let after = r.timeline.iter().find(|f| (f.at - promoted).abs() < 1e-9).unwrap().traffic.as_ref().unwrap();
    assert!(flow(after, "e-app-dc2--pg-replica") > 0.0);
    assert_eq!(lost(after, "e-app-dc2--pg-replica"), 0.0);
    assert!(after.served > 0.9, "most requests complete again: {}", after.served);
    // and it shows in the numbers of the capacity model: what the site cannot bear is shed at its door
    let cap = r.capacity_ratio.unwrap();
    assert!(cap < 1.0);
    assert!(near(r.traffic.as_ref().unwrap().served, cap), "served {} vs capacity {cap}", r.traffic.as_ref().unwrap().served);
    assert!(lost(r.traffic.as_ref().unwrap(), "e-ingress-dc2--app-dc2") > 0.0, "the overload is dropped where it arrives");
}

#[test]
fn until_the_balancer_notices_it_keeps_sending_to_the_lost_member_and_then_the_traffic_goes_around() {
    let a = template("three-az-region");
    let r = story(&a, Scenario::Node { node_id: "app-b".into() });
    let reroute = r.events.iter().find(|e| e.code == "reroute").unwrap().at.unwrap();
    let first = r.timeline[0].traffic.as_ref().unwrap();
    // the load balancer of zone B still sends its third of the traffic to the application that is gone
    assert!(near(lost(first, "e-lb-b--app-b"), 1.0 / 3.0), "{:?}", first.lost);
    assert!(near(first.served, 2.0 / 3.0), "a third of the requests fail: {}", first.served);
    assert!(first.detours.iter().all(|d| d.from != "lb-b"), "it does not know yet that there is anywhere else to go");
    assert!(near(flow(first, "e-lb-a--app-a"), 1.0 / 3.0));

    // health checks have noticed: the third goes to the applications that are left, along no drawn link
    let done = r.timeline.iter().find(|f| (f.at - reroute).abs() < 1e-9).unwrap().traffic.as_ref().unwrap();
    assert!(done.lost.is_empty(), "{:?}", done.lost);
    assert!(near(done.served, 1.0));
    let from_b: Vec<_> = done.detours.iter().filter(|d| d.from == "lb-b").collect();
    assert_eq!(from_b.len(), 2, "to both of the others: {:?}", done.detours);
    assert!(from_b.iter().all(|d| near(d.flow, 1.0 / 6.0) && (d.to == "app-a" || d.to == "app-c")));
}

#[test]
fn a_link_that_is_cut_takes_no_traffic_and_a_site_that_keeps_running_is_not_a_failed_one() {
    let a = template("two-dc-witness");
    let c = cfg();
    let ix = Index::build(&a, &c.catalog, &c.settings);
    let r = simulate_timeline(&ix, &Scenario::Link { a: "dc1".into(), b: "dc2".into() });
    let t = r.traffic.as_ref().unwrap();
    for (id, st) in &r.edge_states {
        if *st != EdgeState::Ok {
            assert_eq!(flow(t, id), 0.0, "{id} is {st:?} and carries nothing");
        }
    }
}

#[test]
fn the_picture_never_contradicts_the_states() {
    let c = cfg();
    let mut pictures = 0;
    for (name, arch) in all_architectures() {
        let ix = Index::build(&arch, &c.catalog, &c.settings);
        let mut scenarios = vec![Scenario::None];
        scenarios.extend(standard_scenarios(&ix));
        for sc in scenarios {
            let r = simulate_timeline(&ix, &sc);
            let mut pics: Vec<(String, ServiceState, BTreeMap<String, EdgeState>, &Traffic)> = vec![];
            let mut edges: BTreeMap<String, EdgeState> = BTreeMap::new();
            for f in &r.timeline {
                edges.extend(f.edges.iter().map(|(k, v)| (k.clone(), *v)));
                pics.push((format!("{} at {}s", sc.key(), f.at), f.state, edges.clone(), f.traffic.as_ref().expect("every frame carries the traffic")));
            }
            pics.push((format!("{} settled", sc.key()), r.state, r.edge_states.clone(), r.traffic.as_ref().unwrap()));
            pics.push((format!("{} before", sc.key()), ServiceState::Up, BTreeMap::new(), r.traffic_before.as_ref().unwrap()));
            for (what, state, edge_states, t) in pics {
                pictures += 1;
                assert!((0.0..=1.0).contains(&t.served), "{name} {what}: served {}", t.served);
                if state == ServiceState::Down {
                    assert!(near(t.served, 0.0), "{name} {what}: the service is down but {} of the requests complete", t.served);
                }
                if state == ServiceState::Up {
                    assert!(near(t.served, 1.0), "{name} {what}: the service is up but only {} of the requests complete", t.served);
                }
                for (id, v) in t.flows.iter().chain(t.lost.iter()).chain(t.missed.iter()) {
                    assert!(v.is_finite() && *v > 0.0, "{name} {what}: {id} = {v}");
                    assert!(arch.edges.iter().any(|e| &e.id == id), "{name} {what}: {id} is not an edge");
                }
                for (id, v) in &t.flows {
                    assert_ne!(edge_states.get(id), Some(&EdgeState::Dead), "{name} {what}: {id} is dead but carries {v}");
                    assert_ne!(edge_states.get(id), Some(&EdgeState::Cut), "{name} {what}: {id} is cut but carries {v}");
                }
                for d in &t.detours {
                    assert!(arch.nodes.iter().any(|n| n.id == d.from) && arch.nodes.iter().any(|n| n.id == d.to), "{name} {what}: {d:?}");
                    assert!(d.flow > 0.0 || d.lost > 0.0);
                    assert!(
                        !arch.edges.iter().any(|e| e.source == d.from && e.target == d.to && edge_states.get(&e.id) == Some(&EdgeState::Ok)),
                        "{name} {what}: a detour {d:?} runs where a working link is drawn"
                    );
                }
            }
        }
    }
    assert!(pictures > 500, "the check covers plenty of pictures: {pictures}");
}

#[test]
fn only_the_story_carries_the_traffic_and_the_wire_format_is_camel_case() {
    let a = template("two-dc-witness");
    let c = cfg();
    let ix = Index::build(&a, &c.catalog, &c.settings);
    let sc = Scenario::Zone { zone_id: "dc1".into() };
    assert!(simulate(&ix, &sc).traffic.is_none(), "the plain simulation is what the evaluation runs a hundred times: no traffic");
    let v = serde_json::to_value(simulate_timeline(&ix, &sc)).unwrap();
    for key in ["traffic", "trafficBefore"] {
        assert!(v[key]["flows"].is_object() && v[key]["served"].is_number(), "{key}");
    }
    assert!(v["timeline"][0]["traffic"]["flows"].is_object());
    assert!(v["traffic"]["detours"].is_null() || v["traffic"]["detours"].is_array());
    let back: SimResult = serde_json::from_value(v).unwrap();
    assert_eq!(back.timeline.len(), back.timeline.iter().filter(|f| f.traffic.is_some()).count());
}

#[test]
fn an_architecture_without_entry_points_has_nothing_to_serve_and_nothing_to_draw() {
    let a = Architecture::default();
    let r = story(&a, Scenario::None);
    let t = r.traffic.unwrap();
    assert!(t.flows.is_empty() && t.lost.is_empty() && t.detours.is_empty());
    assert!(near(t.served, 1.0));
}
