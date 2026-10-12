//! The AI pipelines. The language model is *advisory*: it proposes a design or a list of
//! operations; everything it produces is parsed, sanitised, validated against the catalog,
//! evaluated by the deterministic engine, optionally sent back to the model for repair/refinement
//! and finally shown to the user as a diff with before/after scores. Nothing is applied without
//! the user's explicit confirmation.

use super::openrouter::{call, extract_json, CallCtx, LlmError, LlmRequest, ModelRole, Msg};
use super::prompts::*;
use crate::api::engine_api::preview_ops;
use crate::api::projects::engine_waivers;
use crate::api_support::{limits, parse_arch, run_evaluate, summarize};
use crate::config_store::Loaded;
use crate::jobs::{Job, Progress};
use crate::state::AppState;
use engine::evaluate::{Report, Status};
use engine::model::{validate, Architecture, IssueLevel, Requirements};
use engine::ops::{apply_ops, diff, Op};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

pub struct PipelineOutput {
    pub result: Value,
    /// Assistant message shown in the chat.
    pub message: String,
    pub proposal_id: Option<Uuid>,
}

#[derive(Debug)]
pub struct PipelineError {
    pub code: String,
    pub message: String,
}

impl From<LlmError> for PipelineError {
    fn from(e: LlmError) -> Self {
        PipelineError { code: e.code().to_string(), message: e.to_string() }
    }
}
impl From<sqlx::Error> for PipelineError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = %e, "database error in AI pipeline");
        PipelineError { code: "internal".into(), message: "internal error".into() }
    }
}
impl From<crate::error::ApiError> for PipelineError {
    fn from(e: crate::error::ApiError) -> Self {
        PipelineError { code: e.code.to_string(), message: e.message }
    }
}

fn err(code: &str, msg: impl Into<String>) -> PipelineError {
    PipelineError { code: code.into(), message: msg.into() }
}

type PResult<T> = Result<T, PipelineError>;

pub async fn run(state: &AppState, job: &Job, p: &Progress) -> PResult<PipelineOutput> {
    let cfg = state.cfg().await?;
    let lang = job.input["lang"].as_str().unwrap_or("en").to_string();
    let lang = if cfg.settings.list("app.languages").contains(&lang) { lang } else { cfg.settings.str("app.defaultLanguage") };
    let (draft, rev): (Value, i64) = sqlx::query_as("select draft, draft_rev from projects where id = $1 and deleted_at is null").bind(job.project_id).fetch_optional(&state.pool).await?.ok_or_else(|| err("not_found", "project not found"))?;
    let base = parse_arch(draft)?;
    let ctx = CallCtx { job_id: Some(job.id), project_id: Some(job.project_id), user_id: job.user_id };
    let prompt = job.input["prompt"].as_str().unwrap_or("").to_string();
    p.step("preparing", json!({})).await;
    match job.kind.as_str() {
        "generate" => generate(state, &cfg, &ctx, p, job, base, rev, &prompt, &lang).await,
        "patch" => patch(state, &cfg, &ctx, p, job, base, rev, &prompt, &lang).await,
        "fix" => {
            let fid = job.input["findingId"].as_str().unwrap_or("").to_string();
            let waivers = engine_waivers(state, job.project_id).await?;
            let report = run_evaluate(cfg.clone(), base.clone(), waivers, lang.clone()).await?;
            let f = report.findings.iter().find(|f| f.id == fid).ok_or_else(|| err("unknown_finding", "the finding no longer exists"))?;
            let rule = cfg.rules.iter().find(|r| r.id == f.rule_id).ok_or_else(|| err("unknown_finding", "unknown rule"))?;
            let text = format!(
                "Fix this finding of the rule engine with the smallest possible change.\nRule: {} — {}\nProblem: {}\nAffected nodes: {}\nFingerprint: {}\n{}",
                rule.id,
                rule.title.get(&lang),
                engine::evaluate::render_message(rule, f, &lang),
                f.subjects.nodes.join(", "),
                f.id,
                prompt
            );
            patch(state, &cfg, &ctx, p, job, base, rev, &text, &lang).await
        }
        "chat" => chat(state, &cfg, &ctx, p, job, base, &prompt, &lang).await,
        other => Err(err("bad_kind", format!("unknown job kind {other}"))),
    }
}

// -------------------------------------------------------------------------------------------

struct Candidate {
    arch: Architecture,
    rationale: String,
    assumptions: Vec<String>,
    raw: String,
}

fn parse_generate(cfg: &Loaded, text: &str, base: &Architecture) -> Result<Candidate, Vec<String>> {
    let v = extract_json(text).ok_or_else(|| vec!["the answer is not a valid JSON object".to_string()])?;
    let a = v.get("architecture").cloned().ok_or_else(|| vec!["missing key \"architecture\"".to_string()])?;
    let mut arch: Architecture = serde_json::from_value(a).map_err(|e| vec![format!("architecture does not match the schema: {e}")])?;
    // precedence: top-level `requirements` > `architecture.requirements` > what the project already had
    let inner = std::mem::take(&mut arch.requirements);
    arch.requirements = base.requirements.clone();
    merge_requirements(&mut arch.requirements, inner);
    if let Some(r) = v.get("requirements") {
        if let Ok(req) = serde_json::from_value::<Requirements>(r.clone()) {
            merge_requirements(&mut arch.requirements, req);
        }
    }
    arch.meta = base.meta.clone();
    sanitize(&mut arch, &cfg.catalog);
    if arch.nodes.is_empty() {
        return Err(vec!["the architecture has no nodes".into()]);
    }
    let issues: Vec<String> = validate(&arch, &cfg.catalog, &limits(cfg)).into_iter().filter(|i| i.level == IssueLevel::Error).map(|i| i.message).collect();
    if !issues.is_empty() {
        return Err(issues);
    }
    Ok(Candidate {
        arch,
        rationale: v["rationale"].as_str().unwrap_or("").to_string(),
        assumptions: v["assumptions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        raw: text.to_string(),
    })
}

fn merge_requirements(into: &mut Requirements, from: Requirements) {
    macro_rules! m {
        ($f:ident) => {
            if from.$f.is_some() {
                into.$f = from.$f;
            }
        };
    }
    m!(rps_peak);
    m!(growth_percent);
    m!(rto_seconds);
    m!(rpo_seconds);
    m!(availability_slo);
    m!(budget_monthly);
    m!(failure_tolerance);
    m!(notes);
    if !from.compliance.is_empty() {
        into.compliance = from.compliance;
    }
}

struct PatchCandidate {
    ops: Vec<Op>,
    rationale: String,
    assumptions: Vec<String>,
    raw: String,
}

/// Sanitise operation payloads without touching the user's existing entities.
fn sanitize_ops(ops: &mut [Op], cfg: &Loaded) {
    for op in ops.iter_mut() {
        match op {
            Op::AddNode { node } => {
                let mut tmp = Architecture { nodes: vec![node.clone()], ..Default::default() };
                sanitize(&mut tmp, &cfg.catalog);
                *node = tmp.nodes.remove(0);
            }
            Op::AddZone { zone } => {
                let mut tmp = Architecture { zones: vec![zone.clone()], ..Default::default() };
                sanitize(&mut tmp, &cfg.catalog);
                *zone = tmp.zones.remove(0);
            }
            Op::AddEdge { edge } => {
                let mut tmp = Architecture { edges: vec![edge.clone()], ..Default::default() };
                sanitize(&mut tmp, &cfg.catalog);
                *edge = tmp.edges.remove(0);
            }
            Op::UpdateNode { set, .. } | Op::UpdateEdge { set, .. } | Op::UpdateZone { set, .. } => {
                set.retain(|k, v| !(k.starts_with("props.") && (v.is_object() || v.is_array())));
            }
            _ => {}
        }
    }
}

fn parse_patch(cfg: &Loaded, text: &str, base: &Architecture) -> Result<PatchCandidate, Vec<String>> {
    let v = extract_json(text).ok_or_else(|| vec!["the answer is not a valid JSON object".to_string()])?;
    let ops_v = v.get("ops").cloned().ok_or_else(|| vec!["missing key \"ops\"".to_string()])?;
    let arr = ops_v.as_array().ok_or_else(|| vec!["\"ops\" must be an array".to_string()])?;
    let max = cfg.settings.num("llm.maxOps") as usize;
    if arr.len() > max {
        return Err(vec![format!("too many operations: {} > {}", arr.len(), max)]);
    }
    let mut ops: Vec<Op> = vec![];
    let mut errors = vec![];
    for (i, o) in arr.iter().enumerate() {
        match serde_json::from_value::<Op>(o.clone()) {
            Ok(op) => ops.push(op),
            Err(e) => errors.push(format!("op #{i}: {e}")),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    sanitize_ops(&mut ops, cfg);
    if let Err(e) = apply_ops(base, &ops, &cfg.catalog, &limits(cfg)) {
        return Err(e.issues.into_iter().map(|i| i.message).collect());
    }
    Ok(PatchCandidate {
        ops,
        rationale: v["rationale"].as_str().unwrap_or("").to_string(),
        assumptions: v["assumptions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        raw: text.to_string(),
    })
}

/// Mechanical fixes of the engine for critical findings, restricted to the allowed kinds.
/// `seen` prevents re-applying a fix; `only` limits to findings introduced by the proposal.
fn pick_fixes(cfg: &Loaded, report: &Report, only: Option<&std::collections::HashSet<String>>, lang: &str, seen: &mut std::collections::HashSet<String>) -> Vec<(Vec<Op>, String)> {
    let kinds = cfg.settings.list("llm.autoFixKinds");
    let mut out = vec![];
    for f in report.findings.iter().filter(|f| f.accepted.is_none() && f.status == Status::Critical) {
        if let Some(o) = only {
            if !o.contains(&f.id) {
                continue;
            }
        }
        let Some(rule) = cfg.rules.iter().find(|r| r.id == f.rule_id) else { continue };
        if let Some(fix) = f.fixes.iter().find(|x| kinds.contains(&x.kind)) {
            if !seen.insert(fix.id.clone()) {
                continue;
            }
            let mut vars: BTreeMap<String, Value> = f.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            vars.extend(fix.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
            let label = rule.fix_labels.get(&fix.kind).map(|t| t.render(lang, &vars)).unwrap_or_else(|| fix.kind.clone());
            out.push((fix.ops.clone(), format!("{}: {}", rule.id, label)));
        }
    }
    out
}

fn criticals(r: &Report) -> usize {
    r.findings.iter().filter(|f| f.accepted.is_none() && f.status == Status::Critical).count()
}

/// With `llm.logPrompts` the raw model answers are kept in the job steps (debugging aid).
async fn log_answer(cfg: &Loaded, p: &Progress, purpose: &str, text: &str) {
    if cfg.settings.boolean("llm.logPrompts") {
        p.step("model_answer", json!({"purpose": purpose, "text": text.chars().take(30000).collect::<String>()})).await;
    }
}

fn bullet_list(items: &[String]) -> String {
    items.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")
}

fn common_vars(cfg: &Loaded, lang: &str) -> BTreeMap<&'static str, String> {
    let mut v = BTreeMap::new();
    v.insert("language", language_name(lang).to_string());
    v.insert("catalog", catalog_digest(&cfg.catalog, lang));
    v.insert("edge_types", edge_types_digest(&cfg.catalog));
    v.insert("zone_types", zone_types_digest(&cfg.catalog));
    v.insert("templates", templates_digest(cfg, lang));
    v.insert("rules", rules_digest(cfg, lang));
    v.insert("requirements_schema", requirements_schema().to_string());
    v.insert("max_ops", (cfg.settings.num("llm.maxOps") as i64).to_string());
    v
}

fn system_prompt(cfg: &Loaded, id: &str, vars: &BTreeMap<&'static str, String>) -> PResult<String> {
    let def = cfg.prompts.get(id).ok_or_else(|| err("prompt_missing", format!("prompt '{id}' is not configured")))?;
    Ok(render(&def.system, vars))
}

fn role_of(cfg: &Loaded, id: &str) -> ModelRole {
    match cfg.prompts.get(id).map(|p| p.role.as_str()) {
        Some("fast") => ModelRole::Fast,
        _ => ModelRole::Planner,
    }
}

async fn store_proposal(state: &AppState, job: &Job, base_rev: i64, change: Value, rationale: &str, preview: Value) -> PResult<Uuid> {
    let id: Uuid = sqlx::query_scalar("insert into proposals (project_id, job_id, base_rev, change, rationale, preview, created_by) values ($1, $2, $3, $4, $5, $6, $7) returning id")
        .bind(job.project_id)
        .bind(job.id)
        .bind(base_rev)
        .bind(change)
        .bind(rationale)
        .bind(preview)
        .bind(job.user_id)
        .fetch_one(&state.pool)
        .await?;
    Ok(id)
}

fn assistant_message(rationale: &str, assumptions: &[String], notes: &str) -> String {
    let mut m = rationale.trim().to_string();
    if !assumptions.is_empty() {
        m.push_str("\n\n");
        m.push_str(&bullet_list(assumptions));
    }
    if !notes.is_empty() {
        m.push_str("\n\n");
        m.push_str(notes);
    }
    m
}

// -------------------------------------------------------------------------------------------
// generate: natural-language requirements → full architecture
// -------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn generate(state: &AppState, cfg: &Arc<Loaded>, ctx: &CallCtx, p: &Progress, job: &Job, base: Architecture, rev: i64, prompt: &str, lang: &str) -> PResult<PipelineOutput> {
    let vars = common_vars(cfg, lang);
    let sys = system_prompt(cfg, "generate", &vars)?;
    let send_notes = cfg.settings.boolean("llm.sendNotes");
    let mut user = format!("<user_data>\n{}\n</user_data>", prompt);
    if !base.nodes.is_empty() {
        user.push_str(&format!("\n\nThe user already has this architecture; keep what fits and extend it (reuse ids):\n<user_data>\n{}\n</user_data>", arch_digest(&base, send_notes)));
    }
    let mut messages = vec![Msg::system(sys), Msg::user(user)];
    let schema = generate_schema(&cfg.catalog);
    let repair_rounds = cfg.settings.num("llm.repairRounds") as u32;
    let refine_rounds = cfg.settings.num("llm.refineRounds") as u32;
    let role = role_of(cfg, "generate");

    let mut cand: Option<Candidate> = None;
    let mut last_errors: Vec<String> = vec![];
    for round in 0..=repair_rounds {
        if round == 0 {
            p.step("calling_model", json!({"round": 0})).await;
        }
        let resp = call(state, cfg, ctx, LlmRequest { purpose: "generate", role, messages: messages.clone(), schema: Some(("architecture", schema.clone())) }).await?;
        log_answer(cfg, p, "generate", &resp.content).await;
        match parse_generate(cfg, &resp.content, &base) {
            Ok(c) => {
                cand = Some(c);
                break;
            }
            Err(errs) => {
                last_errors = errs.clone();
                p.step("repairing", json!({"round": round + 1, "errors": errs.len()})).await;
                messages.push(Msg::assistant(resp.content));
                let repair = cfg.prompts.get("repair").map(|d| d.system.clone()).unwrap_or_default();
                let mut v = BTreeMap::new();
                v.insert("errors", bullet_list(&errs.iter().take(40).cloned().collect::<Vec<_>>()));
                messages.push(Msg::user(render(&repair, &v)));
            }
        }
    }
    let mut cand = cand.ok_or_else(|| err("invalid_output", format!("the model could not produce a valid architecture: {}", last_errors.iter().take(5).cloned().collect::<Vec<_>>().join("; "))))?;

    p.step("evaluating", json!({})).await;
    let waivers = engine_waivers(state, job.project_id).await?;
    let mut report = run_evaluate(cfg.clone(), cand.arch.clone(), waivers.clone(), lang.to_string()).await?;

    // refine: feed critical findings back to the model
    for round in 0..refine_rounds {
        if criticals(&report) == 0 {
            break;
        }
        p.step("refining", json!({"round": round + 1, "criticals": criticals(&report)})).await;
        let refine = cfg.prompts.get("refine").map(|d| d.system.clone()).unwrap_or_default();
        let mut v = BTreeMap::new();
        v.insert("findings", findings_digest(&report, cfg, lang, true, 25));
        messages.push(Msg::assistant(cand.raw.clone()));
        messages.push(Msg::user(render(&refine, &v)));
        let Ok(resp) = call(state, cfg, ctx, LlmRequest { purpose: "refine", role, messages: messages.clone(), schema: Some(("architecture", schema.clone())) }).await else { break };
        let mut refine_base = base.clone();
        refine_base.requirements = cand.arch.requirements.clone();
        let Ok(next) = parse_generate(cfg, &resp.content, &refine_base) else { break };
        let next_report = run_evaluate(cfg.clone(), next.arch.clone(), waivers.clone(), lang.to_string()).await?;
        if criticals(&next_report) < criticals(&report) || (criticals(&next_report) == criticals(&report) && next_report.score.value > report.score.value) {
            cand = next;
            report = next_report;
        } else {
            break;
        }
    }

    // deterministic repair of what is mechanical, re-verified by the engine
    let mut applied: Vec<String> = vec![];
    let mut seen = std::collections::HashSet::new();
    for round in 0..cfg.settings.num("llm.autoFixRounds") as u32 {
        let picks = pick_fixes(cfg, &report, None, lang, &mut seen);
        if picks.is_empty() {
            break;
        }
        p.step("autofix", json!({"round": round + 1, "fixes": picks.len()})).await;
        let ops: Vec<Op> = picks.iter().flat_map(|(o, _)| o.clone()).collect();
        let Ok(out) = apply_ops(&cand.arch, &ops, &cfg.catalog, &limits(cfg)) else { break };
        cand.arch = out.architecture;
        applied.extend(picks.into_iter().map(|(_, l)| l));
        report = run_evaluate(cfg.clone(), cand.arch.clone(), waivers.clone(), lang.to_string()).await?;
    }

    // preview against the current draft
    p.step("previewing", json!({})).await;
    let before = run_evaluate(cfg.clone(), base.clone(), waivers, lang.to_string()).await?;
    let (sb, sa) = (summarize(&before), summarize(&report));
    let before_open: std::collections::HashSet<&String> = sb.open.iter().collect();
    let after_open: std::collections::HashSet<&String> = sa.open.iter().collect();
    let preview = json!({
        "diff": diff(&base, &cand.arch, false),
        "before": sb, "after": sa,
        "introduced": sa.open.iter().filter(|x| !before_open.contains(x)).collect::<Vec<_>>(),
        "resolved": sb.open.iter().filter(|x| !after_open.contains(x)).collect::<Vec<_>>(),
    });
    let proposal_id = store_proposal(state, job, rev, json!({"mode": "replace", "architecture": cand.arch}), &cand.rationale, preview).await?;
    let message = assistant_message(&cand.rationale, &cand.assumptions, &autofix_note(&applied, lang));
    Ok(PipelineOutput { result: json!({"proposalId": proposal_id, "rationale": cand.rationale, "assumptions": cand.assumptions, "autoFixes": applied}), message, proposal_id: Some(proposal_id) })
}

fn autofix_note(applied: &[String], lang: &str) -> String {
    if applied.is_empty() {
        return String::new();
    }
    let head = if lang == "ru" { "Движок автоматически применил механические исправления:" } else { "The engine applied mechanical fixes automatically:" };
    format!("{head}\n{}", bullet_list(applied))
}

// -------------------------------------------------------------------------------------------
// patch / fix: edit operations
// -------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn patch(state: &AppState, cfg: &Arc<Loaded>, ctx: &CallCtx, p: &Progress, job: &Job, base: Architecture, rev: i64, request: &str, lang: &str) -> PResult<PipelineOutput> {
    let waivers = engine_waivers(state, job.project_id).await?;
    p.step("evaluating", json!({"phase": "before"})).await;
    let before_report = run_evaluate(cfg.clone(), base.clone(), waivers.clone(), lang.to_string()).await?;
    let mut vars = common_vars(cfg, lang);
    vars.insert("architecture", arch_digest(&base, cfg.settings.boolean("llm.sendNotes")));
    vars.insert("findings", findings_digest(&before_report, cfg, lang, true, 30));
    let sys = system_prompt(cfg, "patch", &vars)?;
    let mut messages = vec![Msg::system(sys), Msg::user(format!("<user_data>\n{}\n</user_data>", request))];
    let schema = patch_schema();
    let repair_rounds = cfg.settings.num("llm.repairRounds") as u32;
    let refine_rounds = cfg.settings.num("llm.refineRounds") as u32;
    let role = role_of(cfg, "patch");

    let mut cand: Option<PatchCandidate> = None;
    let mut last_errors: Vec<String> = vec![];
    for round in 0..=repair_rounds {
        if round == 0 {
            p.step("calling_model", json!({"round": 0})).await;
        }
        let resp = call(state, cfg, ctx, LlmRequest { purpose: if job.kind == "fix" { "fix" } else { "patch" }, role, messages: messages.clone(), schema: Some(("operations", schema.clone())) }).await?;
        log_answer(cfg, p, "patch", &resp.content).await;
        match parse_patch(cfg, &resp.content, &base) {
            Ok(c) => {
                cand = Some(c);
                break;
            }
            Err(errs) => {
                last_errors = errs.clone();
                p.step("repairing", json!({"round": round + 1, "errors": errs.len()})).await;
                messages.push(Msg::assistant(resp.content));
                let repair = cfg.prompts.get("repair").map(|d| d.system.clone()).unwrap_or_default();
                let mut v = BTreeMap::new();
                v.insert("errors", bullet_list(&errs.iter().take(40).cloned().collect::<Vec<_>>()));
                messages.push(Msg::user(render(&repair, &v)));
            }
        }
    }
    let mut cand = cand.ok_or_else(|| err("invalid_output", format!("the model could not produce valid operations: {}", last_errors.iter().take(5).cloned().collect::<Vec<_>>().join("; "))))?;
    if cand.ops.is_empty() {
        return Ok(PipelineOutput {
            result: json!({"rationale": cand.rationale, "noChanges": true}),
            message: assistant_message(&cand.rationale, &cand.assumptions, ""),
            proposal_id: None,
        });
    }

    p.step("evaluating", json!({"phase": "after"})).await;
    let mut preview = preview_ops(cfg.clone(), base.clone(), cand.ops.clone(), waivers.clone(), lang.to_string()).await?;

    for round in 0..refine_rounds {
        let introduced_critical: Vec<&String> = preview.introduced.iter().filter(|id| preview.after_report.findings.iter().any(|f| &f.id == *id && f.status == Status::Critical)).collect();
        if introduced_critical.is_empty() {
            break;
        }
        p.step("refining", json!({"round": round + 1, "criticals": introduced_critical.len()})).await;
        let refine = cfg.prompts.get("refine").map(|d| d.system.clone()).unwrap_or_default();
        let mut v = BTreeMap::new();
        v.insert("findings", findings_digest(&preview.after_report, cfg, lang, true, 25));
        messages.push(Msg::assistant(cand.raw.clone()));
        messages.push(Msg::user(render(&refine, &v)));
        let Ok(resp) = call(state, cfg, ctx, LlmRequest { purpose: "refine", role, messages: messages.clone(), schema: Some(("operations", schema.clone())) }).await else { break };
        let Ok(next) = parse_patch(cfg, &resp.content, &base) else { break };
        let Ok(next_preview) = preview_ops(cfg.clone(), base.clone(), next.ops.clone(), waivers.clone(), lang.to_string()).await else { break };
        if criticals(&next_preview.after_report) < criticals(&preview.after_report) {
            cand = next;
            preview = next_preview;
        } else {
            break;
        }
    }

    // deterministic repair of the critical findings the proposal introduced
    let mut applied: Vec<String> = vec![];
    let mut seen = std::collections::HashSet::new();
    for round in 0..cfg.settings.num("llm.autoFixRounds") as u32 {
        let only: std::collections::HashSet<String> = preview.introduced.iter().cloned().collect();
        let picks = pick_fixes(cfg, &preview.after_report, Some(&only), lang, &mut seen);
        if picks.is_empty() {
            break;
        }
        p.step("autofix", json!({"round": round + 1, "fixes": picks.len()})).await;
        let mut ops = cand.ops.clone();
        ops.extend(picks.iter().flat_map(|(o, _)| o.clone()));
        let Ok(next) = preview_ops(cfg.clone(), base.clone(), ops.clone(), waivers.clone(), lang.to_string()).await else { break };
        cand.ops = ops;
        preview = next;
        applied.extend(picks.into_iter().map(|(_, l)| l));
    }

    let pv = json!({
        "diff": preview.diff, "before": preview.before, "after": preview.after, "introduced": preview.introduced, "resolved": preview.resolved,
    });
    let proposal_id = store_proposal(state, job, rev, json!({"mode": "ops", "ops": cand.ops}), &cand.rationale, pv).await?;
    let message = assistant_message(&cand.rationale, &cand.assumptions, &autofix_note(&applied, lang));
    Ok(PipelineOutput { result: json!({"proposalId": proposal_id, "rationale": cand.rationale, "assumptions": cand.assumptions, "autoFixes": applied}), message, proposal_id: Some(proposal_id) })
}

// -------------------------------------------------------------------------------------------
// chat: grounded Q&A, read-only
// -------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn chat(state: &AppState, cfg: &Arc<Loaded>, ctx: &CallCtx, p: &Progress, job: &Job, base: Architecture, question: &str, lang: &str) -> PResult<PipelineOutput> {
    let waivers = engine_waivers(state, job.project_id).await?;
    p.step("evaluating", json!({})).await;
    let report = run_evaluate(cfg.clone(), base.clone(), waivers, lang.to_string()).await?;
    let mut vars = common_vars(cfg, lang);
    vars.insert("architecture", arch_digest(&base, cfg.settings.boolean("llm.sendNotes")));
    vars.insert("report", report_digest(&report, cfg, lang));
    let sys = system_prompt(cfg, "chat", &vars)?;
    let mut messages = vec![Msg::system(sys)];
    // short conversation memory
    let hist: Vec<(String, String)> = sqlx::query_as("select role, content from chat_messages where project_id = $1 and role in ('user','assistant') and (job_id is null or job_id <> $2) order by id desc limit 6")
        .bind(job.project_id)
        .bind(job.id)
        .fetch_all(&state.pool)
        .await?;
    for (role, content) in hist.into_iter().rev() {
        let c = content.chars().take(2000).collect::<String>();
        messages.push(if role == "user" { Msg::user(format!("<user_data>\n{c}\n</user_data>")) } else { Msg::assistant(c) });
    }
    messages.push(Msg::user(format!("<user_data>\n{}\n</user_data>", question)));
    p.step("calling_model", json!({})).await;
    let resp = call(state, cfg, ctx, LlmRequest { purpose: "chat", role: role_of(cfg, "chat"), messages, schema: None }).await?;
    Ok(PipelineOutput { result: json!({"answer": resp.content}), message: resp.content, proposal_id: None })
}
