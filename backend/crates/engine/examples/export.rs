//! Developer tool: write an export of a bundled template into a directory.
//!   cargo run -p engine --example export -- two-dc-witness terraform /tmp/out

use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::evaluate;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (tpl, fmt, dir) = (&a[1], &a[2], &a[3]);
    let arch = if tpl == "demo" { defaults::demo_project().architecture } else { defaults::templates().into_iter().find(|t| &t.id == tpl).expect("template").architecture };
    let cfg = EngineConfig { catalog: defaults::catalog(), rules: defaults::rules(), settings: defaults::engine_settings(), lang: "ru".into() };
    let rep = evaluate(&arch, &cfg, &[]);
    for f in engine::export::generate(fmt, &arch, &cfg, &rep, &[], "ru", "2026-10-12").expect("format") {
        let p = std::path::Path::new(dir).join(&f.path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, f.content).unwrap();
        println!("wrote {}", p.display());
    }
}
