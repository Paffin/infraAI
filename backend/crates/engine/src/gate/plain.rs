//! The report for a terminal: what the design scores, what is open and how to fix it, what to improve first, and the verdict.

use super::{open_findings, Run};
use crate::evaluate::Status;
use crate::model::IssueLevel;
use serde_json::json;

#[derive(Debug, Clone, Copy, Default)]
pub struct Style {
    /// Colour the levels (an ANSI terminal).
    pub color: bool,
}

fn paint(on: bool, code: &str, s: &str) -> String {
    if on {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn level_color(s: Status) -> &'static str {
    match s {
        Status::Critical => "1;31",
        Status::Risk => "1;33",
        _ => "1;36",
    }
}

/// How many fixes of a finding are listed, and how many of the places it is about.
const FIXES_SHOWN: usize = 2;
const WHERE_SHOWN: usize = 4;

pub fn render(run: &Run, style: Style) -> String {
    let c = style.color;
    let mut out = String::new();
    out.push_str(&paint(c, "1", &format!("{} — {}", run.name(), run.file)));
    out.push('\n');
    out.push_str(&run.figures().join(" · "));
    out.push('\n');

    let notes: Vec<_> = run.report.issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
    if !notes.is_empty() {
        out.push('\n');
        out.push_str(&paint(c, "1", &run.say("issues", &[])));
        out.push('\n');
        for i in notes {
            out.push_str(&format!("  - {}\n", i.message));
        }
    }

    let findings = open_findings(run.report);
    if findings.is_empty() {
        out.push('\n');
        out.push_str(&run.say("none", &[]));
        out.push('\n');
    }
    for f in findings {
        out.push('\n');
        // what is wrong comes first, like a linter says it; the rule is the second line
        out.push_str(&format!("{}  {}\n", paint(c, level_color(f.status), &run.status_word(f.status)), run.message(f)));
        out.push_str(&format!("    {}\n", paint(c, "2", &format!("{} · {}", f.rule_id, run.title(&f.rule_id)))));
        let subjects = run.subjects(f);
        let place = |s: &super::Subject| match s.line {
            Some(l) => format!("{}:{l}  {}", run.file, s.label()),
            None => s.label(),
        };
        match subjects.len() {
            0 => {}
            1 => out.push_str(&format!("    {}: {}\n", paint(c, "2", &run.say("where", &[])), place(&subjects[0]))),
            n => {
                // one per line, `file:line` first: a terminal turns it into a link to the place
                out.push_str(&format!("    {}:\n", paint(c, "2", &run.say("where", &[]))));
                for s in subjects.iter().take(WHERE_SHOWN) {
                    out.push_str(&format!("      {}\n", place(s)));
                }
                if n > WHERE_SHOWN {
                    out.push_str(&format!("      … +{}\n", n - WHERE_SHOWN));
                }
            }
        }
        for fix in f.fixes.iter().filter(|x| !x.ops.is_empty()).take(FIXES_SHOWN) {
            out.push_str(&format!("    {}: {}\n", paint(c, "2", &run.say("fix", &[])), run.fix_label(f, fix)));
        }
        let needs = run.needs(f);
        if f.status == Status::NoData && !needs.is_empty() {
            out.push_str(&format!("    {}: {}\n", paint(c, "2", &run.say("fill_in", &[])), needs.join(", ")));
        }
    }

    if let Some(plan) = run.plan {
        if !plan.items.is_empty() || !plan.manual.is_empty() {
            out.push('\n');
            out.push_str(&paint(c, "1", &run.say("first", &[])));
            out.push('\n');
        }
        for (i, r) in plan.items.iter().enumerate() {
            out.push_str(&format!("  {}. {}  {}\n", i + 1, run.recommendation_label(r), paint(c, "2", &format!("[{}]", r.rule_id))));
            let chips = run.effect_chips(&r.effect);
            if !chips.is_empty() {
                out.push_str(&format!("     {}\n", chips.join(" · ")));
            }
        }
        if let Some(b) = &plan.bundle {
            out.push_str(&format!("  {}: {}\n", run.say("together", &[("n", json!(b.ids.len()))]), run.effect_chips(&b.effect).join(" · ")));
        }
        if !plan.manual.is_empty() {
            out.push_str(&format!("  {}\n", run.say("manual", &[("n", json!(plan.manual.len()))])));
        }
        if plan.truncated {
            out.push_str(&format!("  {}\n", run.say("truncated", &[("n", json!(plan.considered))])));
        }
    }

    if !run.expired.is_empty() {
        out.push('\n');
        out.push_str(&run.say("expired", &[("ids", json!(run.expired.join(", ")))]));
        out.push('\n');
    }

    out.push('\n');
    let v = run.verdict;
    if v.passed {
        out.push_str(&paint(c, "1;32", &run.say("passed", &[])));
    } else {
        out.push_str(&paint(c, "1;31", &run.say("failed", &[])));
    }
    out.push('\n');
    for r in &v.reasons {
        out.push_str(&format!("  - {}\n", run.reason_text(r)));
    }
    if v.known > 0 {
        out.push_str(&format!("  {}\n", run.say("known", &[("n", json!(v.known))])));
    }
    out
}
