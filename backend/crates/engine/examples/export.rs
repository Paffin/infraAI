//! Developer tool: write an export of a bundled template (or the demo project) into a directory.
//!   cargo run -p engine --example export -- two-dc-witness terraform /tmp/out [lang]
//! The IaC report (what was and was not generated) is printed to stderr.

use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::evaluate;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: export <template-id|demo> <format> <out-dir> [lang]");
        std::process::exit(2);
    }
    let (tpl, fmt, dir) = (&a[1], &a[2], &a[3]);
    let lang = a.get(4).cloned().unwrap_or_else(|| "en".into());
    let arch = if tpl == "demo" { defaults::demo_project().architecture } else { defaults::templates().into_iter().find(|t| &t.id == tpl).expect("template").architecture };
    let cfg = EngineConfig { catalog: defaults::catalog(), rules: defaults::rules(), settings: defaults::engine_settings(), iac: defaults::iac_templates(), lang: lang.clone() };
    let rep = evaluate(&arch, &cfg, &[]);
    let out = engine::export::generate(fmt, &arch, &cfg, &rep, &[], &lang, "2026-10-12").expect("format");
    for f in &out.files {
        let p = std::path::Path::new(dir).join(&f.path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, &f.content).unwrap();
        println!("wrote {}", p.display());
    }
    if let Some(r) = &out.report {
        eprintln!("{}", serde_json::to_string_pretty(r).unwrap());
    }
}
