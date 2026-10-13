//! Developer tool: print the evaluation report of a bundled template (or a JSON file).
//!   cargo run -p engine --example report -- two-dc-witness
//!   cargo run -p engine --example report -- demo
//!   cargo run -p engine --example report -- path/to/architecture.json [lang]
//!   cargo run -p engine --example report -- three-az-region en --json     (the whole report as JSON – how the
//!                                                                        frontend test fixtures are recorded)
//!   … --today 2026-10-13     (give the engine a clock: without one the lifecycle rules are not applicable)
//!   … --recommend            (what to improve first: the plan of `engine::recommend`; with --json the whole plan as JSON)

use engine::defaults;
use engine::evaluate::{evaluate, render_message, Status};
use engine::model::Architecture;

static TODAY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn clock() -> String {
    TODAY.get().cloned().unwrap_or_default()
}

fn main() {
    let mut raw: Vec<String> = std::env::args().collect();
    if let Some(i) = raw.iter().position(|a| a == "--today") {
        let day = raw.get(i + 1).cloned().expect("--today needs a date (YYYY-MM-DD)");
        raw.drain(i..=i + 1);
        TODAY.set(day).ok();
    }
    let json = raw.iter().any(|a| a == "--json");
    let recommend = raw.iter().any(|a| a == "--recommend");
    let args: Vec<String> = raw.into_iter().filter(|a| a != "--json" && a != "--recommend").collect();
    let what = args.get(1).cloned().unwrap_or_else(|| "two-dc-witness".into());
    let lang = args.get(2).cloned().unwrap_or_else(|| "ru".into());
    let arch: Architecture = if what == "demo" {
        defaults::demo_project().architecture
    } else if let Some(t) = defaults::templates().into_iter().find(|t| t.id == what) {
        t.architecture
    } else {
        serde_json::from_str(&std::fs::read_to_string(&what).expect("read file")).expect("parse architecture")
    };
    let cfg = engine::config::EngineConfig { today: clock, ..defaults::engine_config(&lang) };
    let rep = evaluate(&arch, &cfg, &[]);
    if recommend {
        let plan = engine::recommend::recommend_from(&arch, &rep, &cfg, &[], &engine::model::ValidationLimits::default());
        if json {
            println!("{}", serde_json::to_string_pretty(&plan).expect("serialize the plan"));
            return;
        }
        println!("{} fixes tried{}; {} for a person", plan.considered, if plan.truncated { " (more were left)" } else { "" }, plan.manual.len());
        for (i, r) in plan.items.iter().enumerate() {
            let e = &r.effect;
            println!(
                "  {}. {} [{}]  critical {}→{}  risk {}→{}  score {}→{}  cost {:+.0}  resolved {}  introduced {}",
                i + 1,
                r.id,
                r.rule_id,
                e.critical.before,
                e.critical.after,
                e.risk.before,
                e.risk.after,
                e.score.before,
                e.score.after,
                e.cost.after - e.cost.before,
                e.resolved.len(),
                e.introduced.len()
            );
        }
        if let Some(b) = &plan.bundle {
            println!("  all {}: critical {}→{}  score {}→{}", b.ids.len(), b.effect.critical.before, b.effect.critical.after, b.effect.score.before, b.effect.score.after);
        }
        return;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&rep).expect("serialize the report"));
        return;
    }
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
