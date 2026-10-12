//! Developer tool: print the evaluation report of a bundled template (or a JSON file).
//!   cargo run -p engine --example report -- two-dc-witness
//!   cargo run -p engine --example report -- demo
//!   cargo run -p engine --example report -- path/to/architecture.json [lang]

use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::{evaluate, render_message, Status};
use engine::model::Architecture;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let what = args.get(1).cloned().unwrap_or_else(|| "two-dc-witness".into());
    let lang = args.get(2).cloned().unwrap_or_else(|| "ru".into());
    let arch: Architecture = if what == "demo" {
        defaults::demo_project().architecture
    } else if let Some(t) = defaults::templates().into_iter().find(|t| t.id == what) {
        t.architecture
    } else {
        serde_json::from_str(&std::fs::read_to_string(&what).expect("read file")).expect("parse architecture")
    };
    let cfg = EngineConfig { catalog: defaults::catalog(), rules: defaults::rules(), settings: defaults::engine_settings(), lang: lang.clone() };
    let rep = evaluate(&arch, &cfg, &[]);
    println!(
        "score {} (coverage {:.0}%)  critical {}  risk {}  no_data {}  ok {}  n/a {}  cost {} {}",
        rep.score.value,
        rep.score.coverage * 100.0,
        rep.counts.critical,
        rep.counts.risk,
        rep.counts.no_data,
        rep.counts.ok,
        rep.counts.na,
        rep.cost.total,
        rep.cost.symbol
    );
    for o in &rep.objectives {
        println!("  objective {:<10} {:?}  required={:?} actual={:?}", o.id, o.status, o.required, o.actual);
    }
    for f in &rep.findings {
        if f.status == Status::Ok {
            continue;
        }
        let rule = cfg.rules.iter().find(|r| r.id == f.rule_id).unwrap();
        println!("  [{:?}] {} — {}  (fixes: {})", f.status, f.rule_id, render_message(rule, f, &lang), f.fixes.len());
    }
    println!("scenarios:");
    for s in &rep.scenarios {
        println!(
            "  {:<40} {:?} recovery={:?}/{:?} rpo={:?}{} cap={:?}{}{}",
            s.failed.join(", "),
            s.state,
            s.recovery_kind,
            s.recovery_seconds,
            s.rpo_seconds,
            if s.rpo_assumed { "*" } else { "" },
            s.capacity_ratio.map(|c| (c * 100.0).round() / 100.0),
            if s.split_brain { " SPLIT-BRAIN" } else { "" },
            if s.data_lost { " DATA-LOST" } else { "" }
        );
    }
}
