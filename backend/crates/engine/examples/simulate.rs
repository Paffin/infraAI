//! Developer tool: the simulation of one failure scenario of a bundled template (or a JSON file), with its timeline, as JSON.
//! It is how the frontend's playback fixtures are recorded (`deploy/record-sim.sh`).
//!   cargo run -q -p engine --example simulate -- two-dc-witness '{"type":"zone","zoneId":"dc1"}'
//!   cargo run -q -p engine --example simulate -- path/to/architecture.json '{"type":"node","nodeId":"app-a"}' en

use engine::defaults;
use engine::graph::Index;
use engine::model::Architecture;
use engine::sim::{simulate_timeline, Scenario};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let what = args.get(1).cloned().unwrap_or_else(|| "two-dc-witness".into());
    let scenario: Scenario = serde_json::from_str(args.get(2).map(|s| s.as_str()).unwrap_or(r#"{"type":"none"}"#)).expect("the scenario is JSON like {\"type\":\"zone\",\"zoneId\":\"dc1\"}");
    let lang = args.get(3).cloned().unwrap_or_else(|| "en".into());
    let arch: Architecture = if what == "demo" {
        defaults::demo_project().architecture
    } else if let Some(t) = defaults::templates().into_iter().find(|t| t.id == what) {
        t.architecture
    } else {
        serde_json::from_str(&std::fs::read_to_string(&what).expect("read file")).expect("parse architecture")
    };
    let cfg = defaults::engine_config(&lang);
    let ix = Index::build(&arch, &cfg.catalog, &cfg.settings);
    println!("{}", serde_json::to_string_pretty(&simulate_timeline(&ix, &scenario)).expect("serialize the result"));
}
