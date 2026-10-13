//! The findings as SARIF 2.1.0, the format that code-scanning tools read (GitHub code scanning, GitLab, Azure DevOps, the viewers of
//! editors). A finding is a result of its rule; it points at the line of the element in the file it is about, so the tool can annotate it.

use super::{Run, Subject};
use crate::config::{Rule, Severity};
use crate::evaluate::{Finding, Status};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";

/// The program that wrote the report.
pub struct Tool<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub information_uri: Option<&'a str>,
}

/// A path as a URI relative to the root of the repository: forward slashes, no `./`, unsafe characters escaped.
pub fn uri_of(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.trim_start_matches("./");
    let mut out = String::with_capacity(p.len());
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn level(s: Status) -> &'static str {
    match s {
        Status::Critical => "error",
        Status::Risk => "warning",
        _ => "note",
    }
}

fn rule_level(r: &Rule) -> &'static str {
    match r.severity {
        Severity::Critical => "error",
        Severity::Risk => "warning",
    }
}

fn listed(f: &Finding) -> bool {
    matches!(f.status, Status::Critical | Status::Risk | Status::NoData)
}

fn location(file_uri: &str, s: Option<&Subject>) -> Value {
    let mut loc = json!({
        "physicalLocation": {
            "artifactLocation": { "uri": file_uri, "uriBaseId": "%SRCROOT%" },
            "region": { "startLine": s.and_then(|s| s.line).unwrap_or(1) },
        },
    });
    if let Some(s) = s {
        loc["logicalLocations"] = json!([{ "name": s.id, "fullyQualifiedName": format!("{}/{}", s.kind, s.id), "kind": "resource" }]);
        loc["message"] = json!({ "text": s.label() });
    }
    loc
}

fn result(run: &Run, f: &Finding, rule_index: usize, file_uri: &str) -> Value {
    let subjects = run.subjects(f);
    let mut r = json!({
        "ruleId": f.rule_id,
        "ruleIndex": rule_index,
        "level": level(f.status),
        "message": { "text": run.message(f) },
        "locations": [location(file_uri, subjects.first())],
        "partialFingerprints": { "infraArchitectFinding/v1": format!("{file_uri}|{}", f.id) },
    });
    if subjects.len() > 1 {
        r["relatedLocations"] = Value::Array(
            subjects.iter().skip(1).enumerate().map(|(i, s)| json!({ "id": i + 1, "physicalLocation": location(file_uri, Some(s))["physicalLocation"], "message": { "text": s.label() } })).collect(),
        );
    }
    let fixes: Vec<String> = f.fixes.iter().filter(|x| !x.ops.is_empty()).map(|x| run.fix_label(f, x)).collect();
    let mut props = Map::new();
    props.insert("status".into(), json!(f.status));
    props.insert("category".into(), json!(f.category));
    if !fixes.is_empty() {
        props.insert("fixes".into(), json!(fixes));
    }
    let needs = run.needs(f);
    if !needs.is_empty() {
        props.insert("needs".into(), json!(needs));
    }
    r["properties"] = Value::Object(props);
    if let Some(a) = &f.accepted {
        r["suppressions"] = json!([{ "kind": "external", "status": "accepted", "justification": a.reason }]);
    }
    r
}

fn rule_entry(r: Option<&Rule>, id: &str, run: &Run) -> Value {
    let Some(r) = r else { return json!({ "id": id, "shortDescription": { "text": id } }) };
    let (title, lang) = (r.title.get(run.lang), run.lang);
    let mut help = String::new();
    if let Some(x) = &r.rationale {
        help.push_str(&format!("{}: {}\n\n", run.say("sarif.rationale", &[]), x.get(lang)));
    }
    if let Some(x) = &r.remediation {
        help.push_str(&format!("{}: {}\n\n", run.say("sarif.remediation", &[]), x.get(lang)));
    }
    if !r.references.is_empty() {
        help.push_str(&r.references.join("; "));
    }
    let help = help.trim().to_string();
    let mut e = json!({
        "id": id,
        "name": id,
        "shortDescription": { "text": title },
        "defaultConfiguration": { "level": rule_level(r) },
        "properties": { "category": r.category, "tags": [r.category.clone(), "architecture"], "precision": "high" },
    });
    if let Some(x) = &r.rationale {
        e["fullDescription"] = json!({ "text": x.get(lang) });
    }
    if !help.is_empty() {
        e["help"] = json!({ "text": help, "markdown": help });
    }
    e
}

/// One SARIF document for every design that was checked.
pub fn document(runs: &[Run], tool: &Tool) -> Value {
    let ids: BTreeSet<String> = runs.iter().flat_map(|r| r.report.findings.iter().filter(|f| listed(f)).map(|f| f.rule_id.clone())).collect();
    let ids: Vec<String> = ids.into_iter().collect();
    let mut results = vec![];
    for run in runs {
        let file_uri = uri_of(run.file);
        let mut findings: Vec<&Finding> = run.report.findings.iter().filter(|f| listed(f)).collect();
        findings.sort_by(|a, b| b.status.cmp(&a.status).then_with(|| a.rule_id.cmp(&b.rule_id)).then_with(|| a.id.cmp(&b.id)));
        for f in findings {
            if let Some(i) = ids.iter().position(|x| x == &f.rule_id) {
                results.push(result(run, f, i, &file_uri));
            }
        }
    }
    let rules: Vec<Value> = match runs.first() {
        Some(first) => ids.iter().map(|id| rule_entry(first.rule(id), id, first)).collect(),
        None => vec![],
    };
    let mut driver = json!({ "name": tool.name, "version": tool.version, "rules": rules });
    if let Some(u) = tool.information_uri {
        driver["informationUri"] = json!(u);
    }
    json!({
        "$schema": SCHEMA,
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": driver },
            "artifacts": runs.iter().map(|r| json!({ "location": { "uri": uri_of(r.file), "uriBaseId": "%SRCROOT%" } })).collect::<Vec<_>>(),
            "results": results,
            "invocations": [{ "executionSuccessful": true }],
            "properties": { "designs": runs.iter().map(|r| json!({ "file": uri_of(r.file), "name": r.name(), "score": r.report.score.value, "passed": r.verdict.passed })).collect::<Vec<_>>() },
        }],
    })
}
