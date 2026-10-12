//! Documents and infrastructure-as-code generated from the single architecture model.
//!
//! * Kubernetes manifests, Helm (helmfile) and Terraform/OpenTofu: see [`crate::iac`] – real
//!   artifacts generated from admin-editable templates, with an honest report of what could
//!   not be generated. No secrets are ever generated.
//! * ADR and Runbook: Markdown with every number traceable to the engine's report.
//! * Mermaid: a diagram for documentation.

use crate::catalog::{Semantics, ZoneLevel};
use crate::config::{EngineConfig, Waiver};
use crate::evaluate::{render_message, Ctx, Report, Status};
use crate::iac;
use crate::model::{as_f64, Architecture};
use crate::sim::{standard_scenarios, RecoveryKind, Scenario, ServiceState};
use crate::text::{display_value, fmt_seconds, interpolate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    pub path: String,
    pub content: String,
}

pub const FORMATS: &[&str] = &["json", "kubernetes", "helm", "terraform", "adr", "runbook", "mermaid"];

/// UI group of a format: `iac` (deployable code), `docs` (documents) or `data`.
pub fn group_of(format: &str) -> &'static str {
    match format {
        "kubernetes" | "helm" | "terraform" => "iac",
        "json" => "data",
        _ => "docs",
    }
}

/// Result of one export: the files and, for IaC formats, what was and was not generated.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Export {
    pub files: Vec<ExportFile>,
    pub report: Option<iac::Report>,
}

impl Export {
    fn docs(files: Vec<ExportFile>) -> Option<Export> {
        Some(Export { files, report: None })
    }
    fn iac(b: iac::Bundle) -> Option<Export> {
        Some(Export { files: b.files, report: Some(b.report) })
    }
}

fn file(path: impl Into<String>, content: impl Into<String>) -> ExportFile {
    ExportFile { path: path.into(), content: content.into() }
}

fn ident(s: &str, taken: &mut HashSet<String>) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let mut out = out.trim_matches('_').to_string();
    if out.is_empty() {
        out = "n".into();
    }
    if out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        out = format!("n_{}", out);
    }
    let base = out.clone();
    let mut i = 2;
    while taken.contains(&out) {
        out = format!("{}_{}", base, i);
        i += 1;
    }
    taken.insert(out.clone());
    out
}

// -------------------------------------------------------------------------------------------
// Mermaid
// -------------------------------------------------------------------------------------------

pub fn mermaid(arch: &Architecture, cfg: &EngineConfig) -> String {
    let ctx = Ctx::new(arch, cfg);
    let ix = &ctx.ix;
    let mut taken = HashSet::new();
    let mut nid: HashMap<String, String> = HashMap::new();
    for n in &arch.nodes {
        nid.insert(n.id.clone(), ident(&n.id, &mut taken));
    }
    let mut zid: HashMap<String, String> = HashMap::new();
    for z in &arch.zones {
        zid.insert(z.id.clone(), format!("z_{}", ident(&z.id, &mut taken)));
    }
    let esc = |s: &str| s.replace('"', "'");
    let mut out = String::from("flowchart TB\n");
    fn emit_zone(
        arch: &Architecture,
        zid: &HashMap<String, String>,
        nid: &HashMap<String, String>,
        parent: Option<&str>,
        indent: usize,
        out: &mut String,
        esc: &dyn Fn(&str) -> String,
        ix: &crate::graph::Index<'_>,
    ) {
        let pad = "  ".repeat(indent);
        for z in arch.zones.iter().filter(|z| z.parent_id.as_deref() == parent) {
            out.push_str(&format!("{pad}subgraph {}[\"{}\"]\n", zid[&z.id], esc(&z.name)));
            emit_zone(arch, zid, nid, Some(&z.id), indent + 1, out, esc, ix);
            for (ni, n) in arch.nodes.iter().enumerate() {
                if n.zone_id.as_deref() == Some(&z.id) {
                    let sub = ix.kind[ni].map(|k| k.abbr.clone()).unwrap_or_default();
                    out.push_str(&format!("{pad}  {}[\"{}<br/><small>{}</small>\"]\n", nid[&n.id], esc(&n.name), esc(&sub)));
                }
            }
            out.push_str(&format!("{pad}end\n"));
        }
    }
    emit_zone(arch, &zid, &nid, None, 1, &mut out, &esc, ix);
    for (ni, n) in arch.nodes.iter().enumerate() {
        if n.zone_id.is_none() {
            let sub = ix.kind[ni].map(|k| k.abbr.clone()).unwrap_or_default();
            out.push_str(&format!("  {}[\"{}<br/><small>{}</small>\"]\n", nid[&n.id], esc(&n.name), esc(&sub)));
        }
    }
    for (ei, e) in arch.edges.iter().enumerate() {
        let (Some(s), Some(t)) = (nid.get(&e.source), nid.get(&e.target)) else { continue };
        let label = e.label.clone().unwrap_or_default();
        let lab = if label.is_empty() { String::new() } else { format!("|\"{}\"|", esc(&label)) };
        let arrow = match (ix.sem(ei), e.edge_type.as_str()) {
            (Some(Semantics::Replication), "sync") => format!("=={}=>", lab),
            (Some(Semantics::Replication), _) => format!("-.{}.->", lab),
            (Some(Semantics::Membership), _) => format!("-.-{}", lab),
            (Some(Semantics::Backup), _) => format!("-.{}.->", lab),
            (Some(Semantics::Control), _) => format!("-.-{}", lab),
            _ => format!("-->{}", lab),
        };
        let arrow = if arrow.starts_with("==") { format!("=={}==>", lab) } else { arrow };
        out.push_str(&format!("  {} {} {}\n", s, arrow, t));
    }
    out
}

// -------------------------------------------------------------------------------------------
// ADR
// -------------------------------------------------------------------------------------------

fn dur(s: f64) -> String {
    fmt_seconds(s).replace('s', " s").replace('m', " min").replace('h', " h")
}

pub fn adr(arch: &Architecture, cfg: &EngineConfig, report: &Report, waivers: &[Waiver], lang: &str, date: &str) -> ExportFile {
    let ex = &cfg.settings.export;
    let t = |k: &str| ex.text(lang, k);
    let name = arch.meta.name.clone().unwrap_or_else(|| "architecture".into());
    let ctx = Ctx::new(arch, cfg);
    let ix = &ctx.ix;
    let mut out = String::new();
    let mut vars = BTreeMap::new();
    vars.insert("n".to_string(), json!("0001"));
    vars.insert("name".to_string(), json!(name));
    out.push_str(&format!("# {}\n\n", interpolate(&t("adr.title"), &vars)));
    out.push_str(&format!("**{}:** {} · **{}:** {}\n\n", t("adr.status"), ex.adr_status, t("adr.date"), date));
    if let Some(d) = &arch.meta.description {
        out.push_str(&format!("{}\n\n", d));
    }

    // context
    let r = &arch.requirements;
    let ns = t("adr.not_set");
    let num = |v: Option<f64>, unit: &str| v.map(|x| format!("{} {}", display_value(&json!(x)), unit).trim().to_string()).unwrap_or_else(|| ns.clone());
    out.push_str(&format!("## {}\n\n", t("adr.context")));
    out.push_str(&format!("- {}: {}\n", t("adr.req.rps"), num(r.rps_peak, "rps")));
    out.push_str(&format!("- {}: {}\n", t("adr.req.growth"), num(r.growth_percent, "%")));
    out.push_str(&format!("- {}: {}\n", t("adr.req.rto"), r.rto_seconds.map(dur).unwrap_or_else(|| ns.clone())));
    out.push_str(&format!("- {}: {}\n", t("adr.req.rpo"), r.rpo_seconds.map(dur).unwrap_or_else(|| ns.clone())));
    out.push_str(&format!("- {}: {}\n", t("adr.req.slo"), num(r.availability_slo, "%")));
    out.push_str(&format!("- {}: {}\n", t("adr.req.budget"), r.budget_monthly.map(|b| format!("{} {}", b, report.cost.symbol)).unwrap_or_else(|| ns.clone())));
    let tol = r.failure_tolerance.clone().unwrap_or_else(|| cfg.settings.assumptions.default_failure_tolerance.clone());
    out.push_str(&format!("- {}: {}\n", t("adr.req.tolerance"), t(&format!("tolerance.{tol}"))));
    if let Some(n) = &r.notes {
        out.push_str(&format!("- {}: {}\n", t("adr.req.notes"), n));
    }

    // decision
    out.push_str(&format!("\n## {}\n\n### {}\n\n", t("adr.decision"), t("adr.zones")));
    out.push_str(&format!("| {} | {} | {} |\n|---|---|---|\n", t("adr.col.zone"), t("adr.col.role"), t("adr.col.geo")));
    for z in &arch.zones {
        if ix.zone_level(arch.zones.iter().position(|x| x.id == z.id).unwrap()) == ZoneLevel::Group {
            continue;
        }
        out.push_str(&format!("| {} | {} | {} |\n", z.name, z.role.clone().unwrap_or_default(), z.props.get("geo").and_then(|v| v.as_str()).unwrap_or("")));
    }
    out.push_str(&format!("\n### {}\n\n| {} | {} | {} |\n|---|---|---|\n", t("adr.units"), t("adr.col.service"), t("adr.col.sites"), t("adr.col.instances")));
    for (u, unit) in ix.units.iter().enumerate() {
        if unit.traits.contains("entry") {
            continue;
        }
        let sites: Vec<String> = ix.unit_sites(u).iter().map(|&s| ix.zone_name(s).to_string()).collect();
        let inst: u32 = unit.members.iter().map(|&m| ix.replicas(m)).sum();
        out.push_str(&format!("| {} | {} | {} |\n", unit.name, if sites.is_empty() { "—".into() } else { sites.join(", ") }, inst));
    }

    // consequences
    out.push_str(&format!("\n## {}\n\n", t("adr.consequences")));
    out.push_str(&format!("**{}: {}/100** ({}: {:.0}%)\n\n", t("adr.score"), report.score.value, t("adr.coverage"), report.score.coverage * 100.0));
    for o in &report.objectives {
        let mark = match o.status {
            Status::Ok => "✓",
            Status::Critical | Status::Risk => "✗",
            _ => "?",
        };
        let fmt = |v: &Option<Value>| v.as_ref().map(|x| if let Some(f) = as_f64(x) { dur_or_num(&o.id, f) } else { display_value(x) }).unwrap_or_else(|| ns.clone());
        out.push_str(&format!("- {} **{}**: {} / {}{}\n", mark, o.id, fmt(&o.actual), fmt(&o.required), if o.assumed { " *(assumed)*" } else { "" }));
    }
    out.push_str(&format!(
        "\n### {}\n\n| {} | {} | {} | {} | {} |\n|---|---|---|---|---|\n",
        t("adr.scenarios"),
        t("adr.col.scenario"),
        t("adr.col.state"),
        t("adr.col.recovery"),
        t("adr.col.rpo"),
        t("adr.col.capacity")
    ));
    for s in &report.scenarios {
        let state = match s.state {
            ServiceState::Up => t("state.up"),
            ServiceState::Degraded => t("state.degraded"),
            ServiceState::Down => t("state.down"),
        };
        let mut v = BTreeMap::new();
        v.insert("seconds".to_string(), json!(s.recovery_seconds.map(dur).unwrap_or_default()));
        let rec = interpolate(
            &t(&format!(
                "recovery.{}",
                match s.recovery_kind {
                    RecoveryKind::None => "none",
                    RecoveryKind::Auto => "auto",
                    RecoveryKind::Manual => "manual",
                    RecoveryKind::Never => "never",
                }
            )),
            &v,
        );
        let flags = format!("{}{}", if s.split_brain { " ⚠ split-brain" } else { "" }, if s.data_lost { " ⚠ data lost" } else { "" });
        out.push_str(&format!(
            "| {} | {}{} | {} | {}{} | {} |\n",
            s.failed.join(" + "),
            state,
            flags,
            rec,
            s.rpo_seconds.map(dur).unwrap_or_else(|| "?".into()),
            if s.rpo_assumed { "*" } else { "" },
            s.capacity_ratio.map(|c| format!("{:.0}%", c * 100.0)).unwrap_or_else(|| "?".into())
        ));
    }

    // findings
    let open: Vec<_> = report.findings.iter().filter(|f| f.accepted.is_none() && matches!(f.status, Status::Critical | Status::Risk)).collect();
    out.push_str(&format!("\n## {}\n\n", t("adr.risks")));
    if open.is_empty() {
        out.push_str(&format!("{}\n", t("adr.none")));
    }
    for f in open {
        if let Some(rule) = cfg.rules.iter().find(|r| r.id == f.rule_id) {
            out.push_str(&format!("- **{}** `{}` — {}\n", t(&format!("status.{}", status_key(f.status))), f.rule_id, render_message(rule, f, lang)));
        }
    }
    out.push_str(&format!("\n## {}\n\n", t("adr.accepted")));
    let accepted: Vec<_> = report.findings.iter().filter(|f| f.accepted.is_some()).collect();
    if accepted.is_empty() && waivers.is_empty() {
        out.push_str(&format!("{}\n", t("adr.none")));
    }
    for f in accepted {
        if let (Some(rule), Some(a)) = (cfg.rules.iter().find(|r| r.id == f.rule_id), &f.accepted) {
            out.push_str(&format!(
                "- `{}` — {}\n  - {}: {}{}{}\n",
                f.rule_id,
                render_message(rule, f, lang),
                t("adr.reason"),
                a.reason,
                a.by.as_ref().map(|b| format!(" ({}: {})", t("adr.by"), b)).unwrap_or_default(),
                a.at.as_ref().map(|b| format!(" ({}: {})", t("adr.at"), b)).unwrap_or_default()
            ));
        }
    }
    out.push_str(&format!("\n## {}\n\n", t("adr.mitigations")));
    let mut any = false;
    for (ei, e) in arch.edges.iter().enumerate() {
        if let Some(m) = ix.edge_eff[ei].text("mitigation") {
            any = true;
            let (s, tg) = ix.edge_ends[ei].map(|(s, tg)| (ix.node_name(s).to_string(), ix.node_name(tg).to_string())).unwrap_or_default();
            out.push_str(&format!("- {} → {}: {}\n", s, tg, m));
            let _ = e;
        }
    }
    if !any {
        out.push_str(&format!("{}\n", t("adr.none")));
    }
    out.push_str(&format!("\n## {}\n\n", t("adr.nodata")));
    let nd: Vec<_> = report.findings.iter().filter(|f| f.status == Status::NoData).collect();
    if nd.is_empty() {
        out.push_str(&format!("{}\n", t("adr.none")));
    }
    for f in nd {
        if let Some(rule) = cfg.rules.iter().find(|r| r.id == f.rule_id) {
            out.push_str(&format!("- `{}` — {}\n", f.rule_id, render_message(rule, f, lang)));
        }
    }
    out.push_str(&format!("\n## {}\n\n**{} {}/month**", t("adr.cost"), display_value(&json!(report.cost.total)), report.cost.symbol));
    if report.cost.incomplete > 0 {
        out.push_str(&format!(" (incomplete: {} nodes)", report.cost.incomplete));
    }
    out.push_str("\n\n");
    for (k, v) in &report.cost.by_category {
        out.push_str(&format!("- {}: {} {}\n", k, display_value(&json!(v)), report.cost.symbol));
    }
    out.push_str(&format!("\n---\n*{}*\n", t("adr.generated")));
    file("ADR-0001.md", out)
}

fn status_key(s: Status) -> &'static str {
    match s {
        Status::Critical => "critical",
        Status::Risk => "risk",
        Status::NoData => "no_data",
        _ => "ok",
    }
}

fn dur_or_num(id: &str, f: f64) -> String {
    match id {
        "rto" | "rpo" => dur(f),
        _ => display_value(&json!(f)),
    }
}

// -------------------------------------------------------------------------------------------
// Runbook
// -------------------------------------------------------------------------------------------

pub fn runbook(arch: &Architecture, cfg: &EngineConfig, lang: &str) -> ExportFile {
    let ex = &cfg.settings.export;
    let t = |k: &str| ex.text(lang, k);
    let ctx = Ctx::new(arch, cfg);
    let ix = &ctx.ix;
    let name = arch.meta.name.clone().unwrap_or_else(|| "architecture".into());
    let mut v = BTreeMap::new();
    v.insert("name".to_string(), json!(name));
    let mut out = format!("# {}\n\n{}\n\n", interpolate(&t("runbook.title"), &v), t("runbook.intro"));
    out.push_str(&format!("## {}\n\n1. {}\n2. {}\n3. {}\n\n", t("runbook.general"), t("runbook.general.1"), t("runbook.general.2"), t("runbook.general.3")));

    for sc in standard_scenarios(ix).into_iter().filter(|s| !matches!(s, Scenario::Instance { .. } | Scenario::Node { .. })) {
        let r = ctx.sim(&sc);
        let mut sv = BTreeMap::new();
        sv.insert("failed".to_string(), json!(r.failed.join(" + ")));
        out.push_str(&format!("## {}\n\n", interpolate(&t("runbook.scenario"), &sv)));
        let state = match r.state {
            ServiceState::Up => t("state.up"),
            ServiceState::Degraded => t("state.degraded"),
            ServiceState::Down => t("state.down"),
        };
        out.push_str(&format!("**{}:** {}", t("runbook.impact"), state));
        if !r.down_units.is_empty() {
            out.push_str(&format!(" — {}: {}", t("adr.reason"), r.down_units.join(", ")));
        }
        if let Some(c) = r.capacity_ratio {
            out.push_str(&format!(" · {}: {:.0}%", t("adr.col.capacity"), c * 100.0));
        }
        out.push_str("\n\n");
        let mut rv = BTreeMap::new();
        rv.insert("seconds".to_string(), json!(r.recovery.seconds.map(dur).unwrap_or_default()));
        let kind = match r.recovery.kind {
            RecoveryKind::None => "none",
            RecoveryKind::Auto => "auto",
            RecoveryKind::Manual => "manual",
            RecoveryKind::Never => "never",
        };
        out.push_str(&format!("**{}:** {}\n\n", t("runbook.rto"), interpolate(&t(&format!("recovery.{kind}")), &rv)));
        out.push_str(&format!("**{}:** {}\n\n", t("runbook.rpo"), r.rpo_seconds.map(dur).unwrap_or_else(|| "?".into())));

        if !r.events.is_empty() {
            out.push_str(&format!("### {}\n\n", t("runbook.auto")));
            for e in &r.events {
                let vars: BTreeMap<String, Value> = e
                    .vars
                    .iter()
                    .map(|(k, v)| {
                        let v2 = match (k.as_str(), v) {
                            ("seconds", Value::Number(n)) => json!(dur(n.as_f64().unwrap_or(0.0))),
                            ("rpo", Value::Number(n)) => json!(dur(n.as_f64().unwrap_or(0.0))),
                            _ => v.clone(),
                        };
                        (k.clone(), v2)
                    })
                    .collect();
                let key = format!("event.{}", e.code);
                let txt = ex.text(lang, &key);
                if txt != key {
                    out.push_str(&format!("- {}\n", interpolate(&txt, &vars)));
                }
            }
            out.push('\n');
        }

        // operator steps from the catalog snippets of the involved units
        let mut steps: Vec<String> = vec![];
        let mut verify: Vec<String> = vec![];
        for e in &r.events {
            let Some(uk) = &e.unit else { continue };
            let Some(&u) = ix.unit_ix.get(uk) else { continue };
            let Some(kind) = ix.kind[ix.units[u].members[0]] else { continue };
            let Some(rb) = &kind.runbook else { continue };
            let pick: &Vec<crate::text::LText> = match e.code.as_str() {
                "promoted" | "manual_failover" | "primary_lost" | "fenced" => &rb.failover,
                "restore_from_backup" => &rb.restore,
                _ => continue,
            };
            let mut vars: BTreeMap<String, Value> = BTreeMap::new();
            vars.insert("node".into(), e.vars.get("node").cloned().unwrap_or(json!(ix.units[u].name)));
            vars.insert("group".into(), json!(ix.units[u].key));
            for st in pick {
                let line = st.render(lang, &vars);
                if !steps.contains(&line) {
                    steps.push(line);
                }
            }
            for st in &rb.verify {
                let line = st.render(lang, &vars);
                if !verify.contains(&line) {
                    verify.push(line);
                }
            }
        }
        out.push_str(&format!("### {}\n\n", t("runbook.steps")));
        if steps.is_empty() {
            out.push_str(&format!("{}\n\n", t("runbook.no_steps")));
        } else {
            for (i, s) in steps.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", i + 1, s));
            }
            out.push('\n');
        }
        if !verify.is_empty() {
            out.push_str(&format!("### {}\n\n", t("runbook.verify")));
            for s in verify {
                out.push_str(&format!("- {}\n", s));
            }
            out.push('\n');
        }
        out.push_str(&format!("_{}_\n\n", t("runbook.after")));
    }
    file("RUNBOOK.md", out)
}

/// Generate one export format.
pub fn generate(format: &str, arch: &Architecture, cfg: &EngineConfig, report: &Report, waivers: &[Waiver], lang: &str, date: &str) -> Option<Export> {
    match format {
        "json" => Export::docs(vec![file("architecture.json", serde_json::to_string_pretty(arch).ok()?)]),
        "kubernetes" => Export::iac(iac::kubernetes(arch, cfg, lang)),
        "helm" => Export::iac(iac::helm(arch, cfg, lang)),
        "terraform" => Export::iac(iac::terraform(arch, cfg, lang)),
        "adr" => Export::docs(vec![adr(arch, cfg, report, waivers, lang, date)]),
        "runbook" => Export::docs(vec![runbook(arch, cfg, lang)]),
        "mermaid" => Export::docs(vec![file("architecture.mmd", mermaid(arch, cfg))]),
        _ => None,
    }
}
