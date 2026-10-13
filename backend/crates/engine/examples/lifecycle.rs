//! Developer tool: the lifecycle verdicts of the shipped release tables, as JSON. The web UI has its own port of the
//! lifecycle arithmetic; this is what its tests are checked against (`frontend/tests/fixtures/lifecycle.json`).
//!   cargo run -q -p engine --example lifecycle > ../../../frontend/tests/fixtures/lifecycle.json

use engine::defaults;
use engine::lifecycle::parse_date;
use serde_json::json;

fn main() {
    let tables = defaults::versions();
    let dates = ["2025-01-01", "2026-10-13", "2027-06-01"];
    let mut cases = vec![];
    for (ti, set) in tables.iter().enumerate().filter(|(i, _)| i % 11 == 0) {
        let n = set.cycles.len();
        let mut versions: Vec<String> = [0, n / 2, n.saturating_sub(1)].iter().filter_map(|&i| set.cycles.get(i)).flat_map(|c| [c.cycle.clone(), format!("{}.5", c.cycle)]).collect();
        if let Some(c) = set.cycles.first() {
            versions.push(format!("v{}", c.cycle));
        }
        versions.push("0.0.1".into());
        versions.push("9999".into());
        versions.dedup();
        for date in dates {
            let today = parse_date(date).unwrap();
            for v in &versions {
                cases.push(json!({"table": set.id, "version": v, "today": date, "lifecycle": set.lifecycle(v, today)}));
            }
            for (prefer_lts, mature) in [(true, 90), (false, 0)] {
                cases.push(json!({"table": set.id, "today": date, "preferLts": prefer_lts, "matureDays": mature, "recommended": set.recommended(today, prefer_lts, mature).map(|c| c.cycle.clone())}));
            }
        }
        let _ = ti;
    }
    println!("{}", serde_json::to_string_pretty(&json!({"dates": dates, "cases": cases})).unwrap());
}
