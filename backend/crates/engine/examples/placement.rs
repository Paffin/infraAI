//! Generate an architecture from a placement pattern and print what the rule engine says about it.
//!
//!     cargo run -p engine --example placement -- <pattern> <sites> [racks] [regions] [siteFailures]
//!
//! The generator proposes; this prints the verdict of the engine (the only judge of correctness).

use engine::defaults;
use engine::evaluate::{evaluate, Status};
use engine::placement::{generate, PlacementRequest};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pattern = args.first().cloned().unwrap_or_else(|| "n-sites".into());
    let num = |i: usize| args.get(i).and_then(|v| v.parse::<u32>().ok());
    let cfg = defaults::engine_config("en");
    let req = PlacementRequest { pattern, sites: num(1), racks: num(2), regions: num(3), site_failures: num(4), lang: Some("en".into()), ..Default::default() };
    match generate(&cfg, &cfg.placement, &req) {
        Ok(res) => {
            let rep = evaluate(&res.architecture, &cfg, &[]);
            println!("sites: {:?}", res.sites.iter().map(|s| format!("{}:{}:{}", s.id, s.role, s.geo)).collect::<Vec<_>>());
            println!("k={} nodes={} edges={} zones={}", res.site_failures, res.architecture.nodes.len(), res.architecture.edges.len(), res.architecture.zones.len());
            println!("notes: {}", res.notes.iter().map(|n| format!("{}{:?}", n.code, n.vars)).collect::<Vec<_>>().join("; "));
            println!("score {} cost {:.0} scenarios {:?}", rep.score.value, rep.cost.total, rep.scenario_stats);
            let all = std::env::var("ALL").is_ok();
            for f in rep.findings.iter().filter(|f| f.accepted.is_none() && (all || f.status != Status::NoData)) {
                println!("  {:?} {} {}", f.status, f.id, serde_json::to_string(&f.vars).unwrap_or_default());
            }
            let nd = rep.findings.iter().filter(|f| f.status == Status::NoData).count();
            println!("  (+{nd} no-data findings)");
        }
        Err(e) => println!("ERROR {e}"),
    }
}
