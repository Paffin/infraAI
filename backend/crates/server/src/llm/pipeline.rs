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
use engine::config::Waiver;
use engine::evaluate::{evaluate, Report, Status};
use engine::model::{validate, Architecture, IssueLevel, Requirements};
use engine::ops::{apply_ops, diff, Op};
use engine::recommend::improves;
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
    let (draft, rev): (Value, i64) = sqlx::query_as("select draft, draft_rev from projects where id = $1 and deleted_at is null")
        .bind(job.project_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| err("not_found", "project not found"))?;
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

#[derive(Debug)]
pub struct Candidate {
    pub arch: Architecture,
    pub rationale: String,
    pub assumptions: Vec<String>,
    pub raw: String,
}

pub fn parse_generate(cfg: &Loaded, text: &str, base: &Architecture) -> Result<Candidate, Vec<String>> {
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
    if let Some(t) = arch.requirements.failure_tolerance.take() {
        arch.requirements.failure_tolerance = normalize_tolerance(cfg, &t);
    }
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
    m!(site_failures);
    m!(notes);
    if !from.compliance.is_empty() {
        into.compliance = from.compliance;
    }
}

#[derive(Debug)]
pub struct PatchCandidate {
    pub ops: Vec<Op>,
    pub rationale: String,
    pub assumptions: Vec<String>,
    pub raw: String,
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
            // a level of failure tolerance is one of the engine's levels; the id of an unknown one would quietly count as "site"
            Op::SetRequirements { set } => {
                if let Some(Value::String(t)) = set.get("failureTolerance") {
                    match normalize_tolerance(cfg, t) {
                        Some(id) => {
                            set.insert("failureTolerance".into(), json!(id));
                        }
                        None => {
                            set.remove("failureTolerance");
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn parse_patch(cfg: &Loaded, text: &str, base: &Architecture) -> Result<PatchCandidate, Vec<String>> {
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

/// What the engine's mechanical fixes did to a design.
pub struct FixRun {
    pub arch: Architecture,
    pub report: Report,
    /// The fixes that stayed: their operations and the label shown to the user.
    pub kept: Vec<(Vec<Op>, String)>,
}

/// Apply the fixes one at a time and keep each one only when the engine finds the result better (fewer critical findings, or as many
/// and a higher score). A fix that does not do what it says – an arbiter that cannot make a quorum survive, a voter that makes the
/// votes even – is dropped, and its finding is left to the user instead of being made worse.
pub fn try_fixes(cfg: &Loaded, arch: Architecture, report: Report, picks: Vec<(Vec<Op>, String)>, waivers: &[Waiver], lang: &str) -> FixRun {
    let ec = cfg.engine_config(lang);
    let lim = limits(cfg);
    let mut run = FixRun { arch, report, kept: vec![] };
    for (ops, label) in picks {
        let Ok(out) = apply_ops(&run.arch, &ops, &ec.catalog, &lim) else { continue };
        let next = evaluate(&out.architecture, &ec, waivers);
        if improves(&next, &run.report) {
            run.arch = out.architecture;
            run.report = next;
            run.kept.push((ops, label));
        }
    }
    run
}

/// `try_fixes` off the async runtime (the evaluations are CPU-bound).
async fn run_fixes(cfg: &Arc<Loaded>, arch: Architecture, report: Report, picks: Vec<(Vec<Op>, String)>, waivers: &[Waiver], lang: &str) -> PResult<FixRun> {
    let (cfg, waivers, lang) = (cfg.clone(), waivers.to_vec(), lang.to_string());
    tokio::task::spawn_blocking(move || try_fixes(&cfg, arch, report, picks, &waivers, &lang)).await.map_err(|e| err("internal", e.to_string()))
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

/// How many products beyond the basic library the prompt describes in full (`llm.catalogExtraKinds`).
fn extra_kinds(cfg: &Loaded) -> usize {
    cfg.settings.num("llm.catalogExtraKinds").max(0.0) as usize
}

/// What the request is about: the words of it and the products the design already has.
fn focus_of<'a>(text: &'a str, base: &'a Architecture) -> Focus<'a> {
    Focus { text, in_design: base.nodes.iter().map(|n| n.kind.as_str()).collect() }
}

/// The products an answer of the model uses and the prompt did not describe in full: a repair tells the model about them (their properties are
/// what an answer most often gets wrong about a product that was only in the index).
fn unseen_kinds(cfg: &Loaded, focus: &Focus, answer: &str) -> Vec<String> {
    let seen: Vec<&str> = detailed(&cfg.catalog, focus, extra_kinds(cfg)).iter().map(|k| k.id.as_str()).collect();
    kinds_named(answer).into_iter().filter(|id| cfg.catalog.kind(id).is_some() && !seen.contains(&id.as_str())).take(40).collect()
}

/// The message that asks the model to repair its answer: the errors, and the description in full of the products it used without having it.
fn repair_message(cfg: &Loaded, lang: &str, errors: String, unseen: &[String]) -> String {
    if unseen.is_empty() {
        return errors;
    }
    format!("{errors}\n\nThe products you used that the catalog above lists only by name, in full (use only these properties):\n{}", kinds_digest(&cfg.catalog, lang, unseen))
}

fn common_vars(cfg: &Loaded, lang: &str, focus: &Focus) -> BTreeMap<&'static str, String> {
    let mut v = BTreeMap::new();
    v.insert("language", language_name(lang).to_string());
    v.insert("catalog", catalog_digest(&cfg.catalog, lang, focus, extra_kinds(cfg)));
    v.insert("edge_types", edge_types_digest(&cfg.catalog));
    v.insert("zone_types", zone_types_digest(&cfg.catalog));
    v.insert("templates", templates_digest(cfg, lang));
    v.insert("rules", rules_digest(cfg, lang));
    v.insert("requirements_schema", requirements_schema(cfg, lang));
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

pub async fn store_proposal(state: &AppState, job: &Job, base_rev: i64, change: Value, rationale: &str, preview: Value) -> PResult<Uuid> {
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

/// The preview stored with a proposal: diff, before/after summaries and what the change introduces or resolves.
fn preview_json(p: &crate::api::engine_api::Preview) -> Value {
    json!({"diff": p.diff, "before": p.before, "after": p.after, "introduced": p.introduced, "resolved": p.resolved})
}

/// Evaluate `ops` against `base` with the engine and store them as a pending proposal – the end of the
/// patch pipeline, usable on its own for operations that did not come from a model.
#[allow(clippy::too_many_arguments)]
pub async fn propose_ops(state: &AppState, cfg: &Arc<Loaded>, job: &Job, base: Architecture, rev: i64, ops: Vec<Op>, rationale: &str, lang: &str) -> PResult<Uuid> {
    let waivers = engine_waivers(state, job.project_id).await?;
    let preview = preview_ops(cfg.clone(), base, ops.clone(), waivers, lang.to_string()).await?;
    store_proposal(state, job, rev, json!({"mode": "ops", "ops": ops}), rationale, preview_json(&preview)).await
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
    let focus = focus_of(prompt, &base);
    let vars = common_vars(cfg, lang, &focus);
    let sys = system_prompt(cfg, "generate", &vars)?;
    let send_notes = cfg.settings.boolean("llm.sendNotes");
    let mut user = format!("<user_data>\n{}\n</user_data>", prompt);
    if !base.nodes.is_empty() {
        user.push_str(&format!("\n\nThe user already has this architecture; keep what fits and extend it (reuse ids):\n<user_data>\n{}\n</user_data>", arch_digest(&base, send_notes)));
    }
    let mut messages = vec![Msg::system(sys), Msg::user(user)];
    let schema = generate_schema(&cfg.catalog, &tolerance_ids(cfg));
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
                let unseen = unseen_kinds(cfg, &focus, &resp.content);
                messages.push(Msg::assistant(resp.content));
                let repair = cfg.prompts.get("repair").map(|d| d.system.clone()).unwrap_or_default();
                let mut v = BTreeMap::new();
                v.insert("errors", bullet_list(&errs.iter().take(40).cloned().collect::<Vec<_>>()));
                messages.push(Msg::user(repair_message(cfg, lang, render(&repair, &v), &unseen)));
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
        v.insert("findings", findings_digest(&report, cfg, lang, true, digest_limit(cfg)));
        messages.push(Msg::assistant(cand.raw.clone()));
        messages.push(Msg::user(render(&refine, &v)));
        let Ok(resp) = call(state, cfg, ctx, LlmRequest { purpose: "refine", role, messages: messages.clone(), schema: Some(("architecture", schema.clone())) }).await else { break };
        let mut refine_base = base.clone();
        refine_base.requirements = cand.arch.requirements.clone();
        let Ok(next) = parse_generate(cfg, &resp.content, &refine_base) else { break };
        let next_report = run_evaluate(cfg.clone(), next.arch.clone(), waivers.clone(), lang.to_string()).await?;
        if improves(&next_report, &report) {
            cand = next;
            report = next_report;
        } else {
            break;
        }
    }

    // deterministic repair of what is mechanical, re-verified by the engine: a fix stays only when the result is better
    let mut applied: Vec<String> = vec![];
    let mut seen = std::collections::HashSet::new();
    for round in 0..cfg.settings.num("llm.autoFixRounds") as u32 {
        let picks = pick_fixes(cfg, &report, None, lang, &mut seen);
        if picks.is_empty() {
            break;
        }
        p.step("autofix", json!({"round": round + 1, "fixes": picks.len()})).await;
        let run = run_fixes(cfg, cand.arch.clone(), report.clone(), picks, &waivers, lang).await?;
        if run.kept.is_empty() {
            break;
        }
        cand.arch = run.arch;
        report = run.report;
        applied.extend(run.kept.into_iter().map(|(_, l)| l));
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
    let head =
        if lang == "ru" { "Движок автоматически применил механические исправления:" } else { "The engine applied mechanical fixes automatically:" };
    format!("{head}\n{}", bullet_list(applied))
}

// -------------------------------------------------------------------------------------------
// patch / fix: edit operations
// -------------------------------------------------------------------------------------------

/// How many engine findings a prompt lists (`llm.digestFindings`).
fn digest_limit(cfg: &Loaded) -> usize {
    cfg.settings.num("llm.digestFindings").max(1.0) as usize
}

#[allow(clippy::too_many_arguments)]
async fn patch(state: &AppState, cfg: &Arc<Loaded>, ctx: &CallCtx, p: &Progress, job: &Job, base: Architecture, rev: i64, request: &str, lang: &str) -> PResult<PipelineOutput> {
    let waivers = engine_waivers(state, job.project_id).await?;
    p.step("evaluating", json!({"phase": "before"})).await;
    let before_report = run_evaluate(cfg.clone(), base.clone(), waivers.clone(), lang.to_string()).await?;
    let focus = focus_of(request, &base);
    let mut vars = common_vars(cfg, lang, &focus);
    vars.insert("architecture", arch_digest(&base, cfg.settings.boolean("llm.sendNotes")));
    vars.insert("findings", findings_digest(&before_report, cfg, lang, true, digest_limit(cfg)));
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
        let resp =
            call(state, cfg, ctx, LlmRequest { purpose: if job.kind == "fix" { "fix" } else { "patch" }, role, messages: messages.clone(), schema: Some(("operations", schema.clone())) }).await?;
        log_answer(cfg, p, "patch", &resp.content).await;
        match parse_patch(cfg, &resp.content, &base) {
            Ok(c) => {
                cand = Some(c);
                break;
            }
            Err(errs) => {
                last_errors = errs.clone();
                p.step("repairing", json!({"round": round + 1, "errors": errs.len()})).await;
                let unseen = unseen_kinds(cfg, &focus, &resp.content);
                messages.push(Msg::assistant(resp.content));
                let repair = cfg.prompts.get("repair").map(|d| d.system.clone()).unwrap_or_default();
                let mut v = BTreeMap::new();
                v.insert("errors", bullet_list(&errs.iter().take(40).cloned().collect::<Vec<_>>()));
                messages.push(Msg::user(repair_message(cfg, lang, render(&repair, &v), &unseen)));
            }
        }
    }
    let mut cand = cand.ok_or_else(|| err("invalid_output", format!("the model could not produce valid operations: {}", last_errors.iter().take(5).cloned().collect::<Vec<_>>().join("; "))))?;
    if cand.ops.is_empty() {
        return Ok(PipelineOutput { result: json!({"rationale": cand.rationale, "noChanges": true}), message: assistant_message(&cand.rationale, &cand.assumptions, ""), proposal_id: None });
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
        v.insert("findings", findings_digest(&preview.after_report, cfg, lang, true, digest_limit(cfg)));
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

    // deterministic repair of the critical findings the proposal introduced – each fix on its own and kept only when the engine finds
    // the result better: a fix that does not do what it says (an arbiter that cannot make a quorum survive, a voter that makes the
    // votes even) is dropped, and the finding is left to the user instead of being made worse
    let mut applied: Vec<String> = vec![];
    let mut seen = std::collections::HashSet::new();
    for round in 0..cfg.settings.num("llm.autoFixRounds") as u32 {
        let only: std::collections::HashSet<String> = preview.introduced.iter().cloned().collect();
        let picks = pick_fixes(cfg, &preview.after_report, Some(&only), lang, &mut seen);
        if picks.is_empty() {
            break;
        }
        p.step("autofix", json!({"round": round + 1, "fixes": picks.len()})).await;
        let run = run_fixes(cfg, preview.outcome.architecture.clone(), preview.after_report.clone(), picks, &waivers, lang).await?;
        if run.kept.is_empty() {
            break;
        }
        cand.ops.extend(run.kept.iter().flat_map(|(o, _)| o.clone()));
        applied.extend(run.kept.into_iter().map(|(_, l)| l));
        preview = preview_ops(cfg.clone(), base.clone(), cand.ops.clone(), waivers.clone(), lang.to_string()).await?;
    }

    let proposal_id = store_proposal(state, job, rev, json!({"mode": "ops", "ops": cand.ops}), &cand.rationale, preview_json(&preview)).await?;
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
    let focus = focus_of(question, &base);
    let mut vars = common_vars(cfg, lang, &focus);
    vars.insert("architecture", arch_digest(&base, cfg.settings.boolean("llm.sendNotes")));
    vars.insert("report", report_digest(&report, cfg, lang));
    let sys = system_prompt(cfg, "chat", &vars)?;
    let mut messages = vec![Msg::system(sys)];
    // short conversation memory
    let hist: Vec<(String, String)> =
        sqlx::query_as("select role, content from chat_messages where project_id = $1 and role in ('user','assistant') and (job_id is null or job_id <> $2) order by id desc limit 6")
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

#[cfg(test)]
mod tests {
    use super::*;
    use engine::evaluate::evaluate;

    /// Real answers of the model (recorded by `tests/live.rs`): the input of everything the pipeline does after the call.
    const PATCH_ANSWER: &str = include_str!("../../tests/fixtures/llm/patch_answer.json");
    const GENERATE_ANSWER: &str = include_str!("../../tests/fixtures/llm/generate_answer.json");
    const GENERATE_MANAGED_ANSWER: &str = include_str!("../../tests/fixtures/llm/generate_managed_answer.json");

    fn answer(fixture: &str) -> String {
        serde_json::from_str::<Value>(fixture).unwrap()["answers"][0].as_str().unwrap().to_string()
    }

    fn demo() -> Architecture {
        engine::defaults::demo_project().architecture
    }

    #[test]
    fn a_real_patch_answer_becomes_valid_operations_the_engine_can_apply() {
        let cfg = Loaded::embedded_defaults();
        let base = demo();
        let c = parse_patch(&cfg, &answer(PATCH_ANSWER), &base).expect("the recorded answer is valid");
        assert_eq!(c.ops.len(), 1);
        assert!(!c.rationale.is_empty() && !c.assumptions.is_empty());
        let out = apply_ops(&base, &c.ops, &cfg.catalog, &limits(&cfg)).unwrap();
        assert_eq!(out.architecture.node("app-dc1").unwrap().props["replicas"], json!(40));
        // the engine, not the model, judges the result
        let ec = cfg.engine_config("en");
        let (before, after) = (evaluate(&base, &ec, &[]), evaluate(&out.architecture, &ec, &[]));
        assert!(after.score.value >= before.score.value - 1.0, "{} → {}", before.score.value, after.score.value);
    }

    #[test]
    fn a_real_generate_answer_in_a_code_fence_becomes_a_valid_architecture() {
        let cfg = Loaded::embedded_defaults();
        let text = answer(GENERATE_ANSWER);
        assert!(text.trim_start().starts_with("```"), "models do wrap JSON in fences");
        let c = parse_generate(&cfg, &text, &Architecture::default()).expect("the recorded answer is valid");
        assert!(c.arch.nodes.len() >= 5 && c.arch.zones.len() >= 2, "{} nodes, {} zones", c.arch.nodes.len(), c.arch.zones.len());
        assert_eq!(c.arch.requirements.rps_peak, Some(300.0));
        assert!(c.arch.nodes.iter().all(|n| cfg.catalog.kind(&n.kind).is_some()), "only catalog kinds survive");
        assert!(c.arch.nodes.iter().any(|n| !n.props.is_empty()), "properties are kept");
        let report = evaluate(&c.arch, &cfg.engine_config("en"), &[]);
        assert!(report.score.value > 0.0);
    }

    #[test]
    fn a_real_answer_for_managed_cloud_services_keeps_the_variants_the_model_chose() {
        let cfg = Loaded::embedded_defaults();
        let c = parse_generate(&cfg, &answer(GENERATE_MANAGED_ANSWER), &Architecture::default()).expect("the recorded answer is valid");
        let variant = |id: &str| c.arch.node(id).and_then(|n| n.props.get("variant")).and_then(Value::as_str).map(String::from);
        assert_eq!(variant("pg-primary").as_deref(), Some("rds"));
        assert_eq!(variant("pg-standby").as_deref(), Some("rds"));
        assert_eq!(variant("redis-primary").as_deref(), Some("elasticache"));
        assert_eq!(variant("eks-a").as_deref(), Some("eks"));
        assert_eq!(c.arch.nodes.iter().filter(|n| n.props.contains_key("variant")).count(), 14, "every product the model named by variant keeps it");
        // the engine takes it from there: a managed database keeps its own quorum, so no store for one is asked of it
        let report = evaluate(&c.arch, &cfg.engine_config("en"), &[]);
        let asked = report.findings.iter().any(|f| f.rule_id == "HA-DB-02" && f.status == Status::Critical && f.subjects.nodes.iter().any(|n| n.starts_with("pg-")));
        assert!(!asked, "{:?}", report.findings.iter().filter(|f| f.rule_id == "HA-DB-02").map(|f| (&f.status, &f.subjects.nodes)).collect::<Vec<_>>());
    }

    /// A scheme drawn inside one data center, and what the model made of "make it fault tolerant within one data center" (recorded by
    /// `tests/live.rs`): the answer the pipeline gets after the call.
    const ONE_DC_BASE: &str = include_str!("../../tests/fixtures/llm/one_dc_base.json");
    const ONE_DC_ANSWER: &str = include_str!("../../tests/fixtures/llm/patch_one_dc_answer.json");

    fn one_dc_base() -> Architecture {
        serde_json::from_str(ONE_DC_BASE).unwrap()
    }

    fn sites(cfg: &Loaded, a: &Architecture) -> Vec<String> {
        a.zones
            .iter()
            .filter(|z| matches!(cfg.catalog.zone_type(&z.zone_type).map(|t| t.level), Some(engine::catalog::ZoneLevel::Site) | Some(engine::catalog::ZoneLevel::Geo)))
            .map(|z| z.id.clone())
            .collect()
    }

    #[test]
    fn a_real_answer_for_one_data_center_declares_the_rack_level_and_stays_in_the_data_center() {
        let cfg = Loaded::embedded_defaults();
        let base = one_dc_base();
        let c = parse_patch(&cfg, &answer(ONE_DC_ANSWER), &base).expect("the recorded answer is valid");
        let out = apply_ops(&base, &c.ops, &cfg.catalog, &limits(&cfg)).unwrap().architecture;
        // the model says what the failure to survive is, with a level the engine has
        assert_eq!(out.requirements.failure_tolerance.as_deref(), Some("rack"));
        assert!(tolerance_ids(&cfg).contains(&"rack".to_string()));
        // nothing leaves the data center
        assert_eq!(sites(&cfg, &out), ["dc1"], "no other site, cloud or region");
        // the racks are parts of the data center and none is an empty box
        let racks: Vec<_> = out.zones.iter().filter(|z| z.zone_type == "rack").collect();
        assert!(racks.len() >= 2);
        for r in &racks {
            assert_eq!(r.parent_id.as_deref(), Some("dc1"));
            assert!(out.nodes.iter().any(|n| n.zone_id.as_deref() == Some(r.id.as_str())), "rack {} is empty", r.id);
        }
        assert!(out.nodes.iter().all(|n| n.zone_id.is_none() || n.zone_id.as_deref() == Some("dc1") || racks.iter().any(|r| Some(r.id.as_str()) == n.zone_id.as_deref())));
        // the engine, not the model, judges it: verified against racks the design holds and the findings about the loss of a site are not asked
        let ec = cfg.engine_config("en");
        let (before, after) = (evaluate(&base, &ec, &[]), evaluate(&out, &ec, &[]));
        assert!(before.counts.critical >= 5, "the scheme as drawn is a single point of failure: {}", before.counts.critical);
        assert!(after.counts.critical <= 1, "{:?}", after.findings.iter().filter(|f| f.status == Status::Critical).map(|f| &f.id).collect::<Vec<_>>());
        assert!(after.score.value > before.score.value + 40.0, "{} → {}", before.score.value, after.score.value);
        assert_eq!(after.objectives.iter().find(|o| o.id == "spof").map(|o| o.status), Some(Status::Ok));
        for id in ["HA-SITE-01", "HA-QRM-02", "HA-WIT-01"] {
            let st = after.rules.iter().find(|r| r.rule_id == id).map(|r| r.status);
            assert!(matches!(st, Some(Status::Na) | Some(Status::Ok)), "{id}: {st:?}");
        }
    }

    #[test]
    fn the_levels_of_failure_tolerance_in_the_prompt_are_the_engines_and_include_the_one_for_a_single_data_center() {
        let mut cfg = Loaded::embedded_defaults();
        let text = requirements_schema(&cfg, "en");
        for id in ["instance", "rack", "site", "geo"] {
            assert!(text.contains(&format!("\"{id}\" =")), "{id} is not offered: {text}");
        }
        assert!(text.contains("single data center"), "the level for one data center says so: {text}");
        assert!(requirements_schema(&cfg, "ru").contains("Подходит для одного ЦОД"));
        // the schema the model is held to lists the same levels
        let schema = generate_schema(&cfg.catalog, &tolerance_ids(&cfg));
        assert_eq!(schema["properties"]["requirements"]["properties"]["failureTolerance"]["enum"], json!(["instance", "rack", "site", "geo"]));
        // a level an administrator adds is offered too, and what the model writes is matched to a level the engine has
        let mut custom = cfg.engine.tolerance.levels[1].clone();
        custom.id = "room".into();
        custom.rank = 1;
        custom.name = engine::text::LText::plain("Machine room");
        custom.help = engine::text::LText::plain("A machine room of the data center is lost.");
        cfg.engine.tolerance.levels.insert(1, custom);
        assert!(requirements_schema(&cfg, "en").contains("\"room\" = A machine room"));
        assert_eq!(tolerance_ids(&cfg)[1], "room");
        assert_eq!(normalize_tolerance(&cfg, " Rack "), Some("rack".into()));
        assert_eq!(normalize_tolerance(&cfg, "machine room"), Some("room".into()));
        assert_eq!(normalize_tolerance(&cfg, "Отказ стойки"), Some("rack".into()));
        assert_eq!(normalize_tolerance(&cfg, "planet"), None);
        // a level the engine does not know never reaches the requirements: it would quietly count as "site"
        let ops = json!({"rationale": "r", "ops": [{"op": "set_requirements", "set": {"failureTolerance": "planet", "rpsPeak": 10}}]}).to_string();
        let c = parse_patch(&cfg, &ops, &Architecture::default()).unwrap();
        let out = apply_ops(&Architecture::default(), &c.ops, &cfg.catalog, &limits(&cfg)).unwrap().architecture;
        assert_eq!((out.requirements.failure_tolerance.clone(), out.requirements.rps_peak), (None, Some(10.0)));
        let gen = json!({"architecture": {"zones": [], "nodes": [{"id": "n", "kind": "redis", "name": "R", "props": {}}], "edges": []}, "requirements": {"failureTolerance": "Rack-level"}, "rationale": "r"}).to_string();
        assert_eq!(parse_generate(&cfg, &gen, &Architecture::default()).unwrap().arch.requirements.failure_tolerance, None);
    }

    #[test]
    fn a_fix_that_makes_things_worse_is_not_applied_and_one_that_helps_is() {
        // the answer that broke a user's picture: the racks are there but the failure to survive is left at the default (a site) and
        // the backups went to a site of their own. The engine used to offer an arbiter at that site for the quorum – one more voter,
        // which can neither make the quorum survive the loss of the data center nor leave the number of votes odd – and the pipeline
        // applied it blindly.
        let cfg = Loaded::embedded_defaults();
        let base = one_dc_base();
        let c = parse_patch(&cfg, &answer(ONE_DC_ANSWER), &base).expect("the recorded answer is valid");
        let mut ops: Vec<Op> = c.ops.into_iter().filter(|o| !matches!(o, Op::SetRequirements { .. })).collect();
        ops.push(serde_json::from_value(json!({"op": "add_zone", "zone": {"id": "bkp-site", "name": "Off-site backups", "type": "cloud", "parentId": null, "role": "standby", "props": {}}})).unwrap());
        ops.push(serde_json::from_value(json!({"op": "update_node", "id": "backup", "set": {"zoneId": "bkp-site"}})).unwrap());
        let arch = apply_ops(&base, &ops, &cfg.catalog, &limits(&cfg)).unwrap().architecture;
        let ec = cfg.engine_config("en");
        let report = evaluate(&arch, &ec, &[]);
        assert!(criticals(&report) >= 3, "judged as a design for several sites it is full of criticals");

        let votes = |a: &Architecture| a.nodes.iter().filter(|n| n.group.as_deref() == Some("etcd")).count();
        let bad: Vec<Op> = serde_json::from_value(json!([
            {"op": "add_node", "node": {"id": "etcd-w", "kind": "etcd", "name": "etcd witness", "zoneId": "bkp-site", "group": "etcd", "role": "witness", "props": {"vcpu": 1, "ramGb": 1}}},
            {"op": "add_edge", "edge": {"id": "e-w", "source": "etcd-w", "target": "etcd", "type": "quorum"}}
        ]))
        .unwrap();
        let good: Vec<Op> = serde_json::from_value(json!([{"op": "set_requirements", "set": {"failureTolerance": "rack"}}])).unwrap();

        // the bad one alone: one more critical (an even number of votes) and nothing resolved – it is dropped
        let alone = try_fixes(&cfg, arch.clone(), report.clone(), vec![(bad.clone(), "bad".into())], &[], "en");
        assert!(alone.kept.is_empty(), "a fix that made things worse was kept");
        assert_eq!(votes(&alone.arch), votes(&arch));
        assert_eq!(criticals(&alone.report), criticals(&report));

        // among others: only the one that helps stays, in whatever order they come
        for picks in [vec![(bad.clone(), "bad"), (good.clone(), "good")], vec![(good.clone(), "good"), (bad.clone(), "bad")]] {
            let run = try_fixes(&cfg, arch.clone(), report.clone(), picks.into_iter().map(|(o, l)| (o, l.to_string())).collect(), &[], "en");
            assert_eq!(run.kept.iter().map(|k| k.1.as_str()).collect::<Vec<_>>(), ["good"]);
            assert_eq!(votes(&run.arch), votes(&arch), "the voter of the bad fix is not there");
            assert!(criticals(&run.report) < criticals(&report), "{} → {}", criticals(&report), criticals(&run.report));
            assert!(run.report.score.value > report.score.value);
        }

        // a fix whose operations do not apply is skipped, not fatal
        let broken: Vec<Op> = serde_json::from_value(json!([{"op": "update_node", "id": "nowhere", "set": {"props.replicas": 3}}])).unwrap();
        let run = try_fixes(&cfg, arch.clone(), report.clone(), vec![(broken, "broken".into()), (good, "good".into())], &[], "en");
        assert_eq!(run.kept.iter().map(|k| k.1.as_str()).collect::<Vec<_>>(), ["good"]);
    }

    #[test]
    fn what_the_validator_rejects_is_fed_back_to_the_model_as_repair_feedback() {
        let cfg = Loaded::embedded_defaults();
        let base = demo();
        // chatter instead of JSON
        assert_eq!(parse_patch(&cfg, "I think you should add a cache!", &base).unwrap_err(), vec!["the answer is not a valid JSON object".to_string()]);
        assert_eq!(parse_generate(&cfg, "{\"rationale\": \"x\"}", &base).unwrap_err(), vec!["missing key \"architecture\"".to_string()]);
        // the recorded answer, damaged the way models do: an operation on a node that does not exist, an unknown kind
        let broken = answer(PATCH_ANSWER).replace("app-dc1", "app-dc9");
        let errs = parse_patch(&cfg, &broken, &base).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("app-dc9")), "{errs:?}");
        let gen = answer(GENERATE_ANSWER).replace("\"kind\": \"postgresql\"", "\"kind\": \"quantum_db\"");
        let errs = parse_generate(&cfg, &gen, &Architecture::default());
        assert!(errs.is_err() || errs.as_ref().unwrap().arch.nodes.iter().all(|n| n.kind != "quantum_db"), "an unknown kind never reaches the diagram");
        // an operation list longer than the configured limit
        let many = json!({"rationale": "x", "ops": (0..500).map(|i| json!({"op": "update_node", "id": "app-dc1", "set": {"props.replicas": i}})).collect::<Vec<_>>()}).to_string();
        assert!(parse_patch(&cfg, &many, &base).unwrap_err()[0].starts_with("too many operations"));
        // the repair prompt carries the errors verbatim
        let repair = cfg.prompts.get("repair").unwrap();
        let mut v = BTreeMap::new();
        v.insert("errors", bullet_list(&["op #0: unknown field".to_string()]));
        assert!(render(&repair.system, &v).contains("- op #0: unknown field"));
    }

    #[test]
    fn model_output_is_sanitised_before_it_reaches_the_diagram() {
        let cfg = Loaded::embedded_defaults();
        let base = demo();
        let text = json!({"rationale": "r", "ops": [
            {"op": "add_node", "node": {"id": "mon-extra", "kind": "monitoring", "name": "Extra", "zoneId": "site3", "props": {"replicas": "2", "bogus": {"x": 1}}}},
            {"op": "update_node", "id": "app-dc1", "set": {"props.replicas": 40, "props.nested": {"a": 1}}}
        ]})
        .to_string();
        let c = parse_patch(&cfg, &text, &base).unwrap();
        let out = apply_ops(&base, &c.ops, &cfg.catalog, &limits(&cfg)).unwrap();
        let n = out.architecture.node("mon-extra").unwrap();
        assert_eq!(n.props["replicas"], json!(2), "numeric strings are coerced");
        assert!(!n.props.contains_key("bogus"), "non-scalar and unknown properties are dropped");
        assert!(!out.architecture.node("app-dc1").unwrap().props.contains_key("nested"));
    }
}
