//! The findings as workflow commands of GitHub Actions (`::error file=…,line=…::…`): they appear as annotations on the change.

use super::{open_findings, Run};
use crate::evaluate::Status;

/// A value of a workflow command: `%`, line breaks and, in a property, `:` and `,` are escaped the way the runner reads them.
fn escape(s: &str, property: bool) -> String {
    let mut out = s.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A");
    if property {
        out = out.replace(':', "%3A").replace(',', "%2C");
    }
    out
}

pub fn render(run: &Run) -> String {
    let mut out = String::new();
    for f in open_findings(run.report) {
        let command = match f.status {
            Status::Critical => "error",
            Status::Risk => "warning",
            _ => "notice",
        };
        let line = run.subjects(f).iter().find_map(|s| s.line).unwrap_or(1);
        let title = format!("{} {}", f.rule_id, run.title(&f.rule_id));
        out.push_str(&format!("::{command} file={},line={line},title={}::{}\n", escape(run.file, true), escape(&title, true), escape(&run.message(f), false)));
    }
    out
}
