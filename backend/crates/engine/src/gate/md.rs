//! The report as Markdown: the summary of a pipeline job, the comment on a change.

use super::{open_findings, Run};
use crate::evaluate::Status;
use serde_json::json;

/// A table cell: no pipes, no line breaks.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ").trim().to_string()
}

pub fn render(run: &Run) -> String {
    let v = run.verdict;
    let mut out = String::new();
    out.push_str(&format!("## {} {} — {}\n\n", if v.passed { "✅" } else { "❌" }, run.name(), run.say(if v.passed { "passed" } else { "failed" }, &[])));
    out.push_str(&format!("`{}` · {}\n\n", run.file, run.figures().join(" · ")));
    if !v.reasons.is_empty() {
        for r in &v.reasons {
            out.push_str(&format!("- {}\n", cell(&run.reason_text(r))));
        }
        out.push('\n');
    }
    if v.known > 0 {
        out.push_str(&format!("{}\n\n", run.say("known", &[("n", json!(v.known))])));
    }

    let findings = open_findings(run.report);
    out.push_str(&format!("### {}\n\n", run.say("md.findings", &[])));
    if findings.is_empty() {
        out.push_str(&run.say("none", &[]));
        out.push_str("\n\n");
    } else {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n|---|---|---|---|---|\n",
            run.say("md.severity", &[]),
            run.say("md.rule", &[]),
            run.say("md.what", &[]),
            run.say("md.where", &[]),
            run.say("md.fix", &[])
        ));
        for f in findings {
            let at: Vec<String> = run.subjects(f).iter().take(3).map(|s| format!("`{}`", s.label())).collect();
            let fix = f.fixes.iter().find(|x| !x.ops.is_empty()).map(|x| run.fix_label(f, x)).unwrap_or_default();
            let fix = if f.status == Status::NoData && fix.is_empty() { run.needs(f).join(", ") } else { fix };
            out.push_str(&format!("| **{}** | `{}` | {} | {} | {} |\n", run.status_word(f.status), f.rule_id, cell(&run.message(f)), cell(&at.join(", ")), cell(&fix)));
        }
        out.push('\n');
    }

    if let Some(plan) = run.plan {
        if !plan.items.is_empty() {
            out.push_str(&format!("### {}\n\n| {} | {} | {} |\n|---|---|---|\n", run.say("first", &[]), run.say("md.step", &[]), run.say("md.fix", &[]), run.say("md.effect", &[])));
            for (i, r) in plan.items.iter().enumerate() {
                out.push_str(&format!("| {} | {} `{}` | {} |\n", i + 1, cell(&run.recommendation_label(r)), r.rule_id, cell(&run.effect_chips(&r.effect).join(" · "))));
            }
            if let Some(b) = &plan.bundle {
                out.push_str(&format!("| | **{}** | {} |\n", run.say("together", &[("n", json!(b.ids.len()))]), cell(&run.effect_chips(&b.effect).join(" · "))));
            }
            out.push('\n');
        }
        if !plan.manual.is_empty() {
            out.push_str(&format!("{}\n\n", run.say("manual", &[("n", json!(plan.manual.len()))])));
        }
        if plan.truncated {
            out.push_str(&format!("{}\n\n", run.say("truncated", &[("n", json!(plan.considered))])));
        }
    }
    if !run.expired.is_empty() {
        out.push_str(&format!("{}\n\n", run.say("expired", &[("ids", json!(run.expired.join(", ")))])));
    }
    out
}
