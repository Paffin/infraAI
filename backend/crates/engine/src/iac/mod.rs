//! Infrastructure-as-code generation from the architecture model.
//!
//! The pipeline is: model → [`plan::build_plan`] (which cluster runs what, which prerequisites,
//! what could *not* be generated) → writers ([`emit`]) for Kubernetes manifests, Helm and
//! Terraform/OpenTofu. Everything that decides *what* a component becomes lives in
//! admin-editable templates (see [`dsl`]); the Rust code only knows how to place workloads and
//! how to write files.

pub mod dsl;
mod emit;
mod plan;
pub mod yaml;

use crate::config::IacTemplate;
use crate::text::LText;
use serde_json::Value;

pub use emit::{helm, kubernetes, terraform, Bundle};
pub use plan::{build_plan, ClusterInfo, Generated, Note, Plan, Prerequisite, Report, SecretNeed, Skipped};

/// Static check of a template: every expression must compile, object ids must be unique. Runs when an
/// administrator saves a template, so a typo can never reach an export.
pub fn validate_template(t: &IacTemplate) -> Vec<String> {
    let dsl = dsl::Dsl::new();
    let mut errs: Vec<dsl::DslError> = vec![];
    let text = |l: &LText, path: &str, errs: &mut Vec<dsl::DslError>| match l {
        LText::Plain(s) => dsl.check(&Value::String(s.clone()), path, errs),
        LText::Map(m) => {
            for (lang, s) in m {
                dsl.check(&Value::String(s.clone()), &format!("{path}.{lang}"), errs);
            }
        }
    };
    let when = |w: &Option<String>, path: &str, errs: &mut Vec<dsl::DslError>| {
        if let Some(e) = w {
            dsl.check_expr(e, path, errs);
        }
    };
    for (i, v) in t.vars.iter().enumerate() {
        dsl.check_expr(&v.expr, &format!("vars[{i}].{}", v.name), &mut errs);
    }
    for (i, r) in t.requires.iter().enumerate() {
        when(&r.when().map(|w| w.to_string()), &format!("requires[{i}].when"), &mut errs);
    }
    for (i, r) in t.unsupported.iter().enumerate() {
        when(&r.when, &format!("unsupported[{i}].when"), &mut errs);
        text(&r.text, &format!("unsupported[{i}].text"), &mut errs);
    }
    for (i, n) in t.needs.iter().enumerate() {
        if let Some(h) = &n.hint {
            text(h, &format!("needs[{i}].hint"), &mut errs);
        }
    }
    let mut ids = std::collections::HashSet::new();
    for (i, o) in t.objects.iter().enumerate() {
        if o.id.trim().is_empty() || !ids.insert(o.id.clone()) {
            errs.push(dsl::DslError { path: format!("objects[{i}].id"), message: "must be unique and not empty".into() });
        }
        when(&o.when, &format!("objects[{i}].when"), &mut errs);
        dsl.check(&o.body, &format!("objects[{i}].body"), &mut errs);
    }
    for (i, r) in t.releases.iter().enumerate() {
        when(&r.when, &format!("releases[{i}].when"), &mut errs);
        for (f, v) in [("name", Some(&r.name)), ("namespace", Some(&r.namespace)), ("chart", Some(&r.chart)), ("repo", Some(&r.repo)), ("version", r.version.as_ref())] {
            if let Some(v) = v {
                dsl.check(&Value::String(v.clone()), &format!("releases[{i}].{f}"), &mut errs);
            }
        }
        dsl.check(&r.values, &format!("releases[{i}].values"), &mut errs);
    }
    for (i, s) in t.secrets.iter().enumerate() {
        when(&s.when, &format!("secrets[{i}].when"), &mut errs);
        dsl.check(&Value::String(s.name.clone()), &format!("secrets[{i}].name"), &mut errs);
        if let Some(h) = &s.hint {
            text(h, &format!("secrets[{i}].hint"), &mut errs);
        }
    }
    for (i, n) in t.notes.iter().enumerate() {
        when(&n.when, &format!("notes[{i}].when"), &mut errs);
        text(&n.text, &format!("notes[{i}].text"), &mut errs);
    }
    errs.into_iter().map(|e| e.to_string()).collect()
}
