//! The gate of a pipeline on the real engine: a design is judged at the level asked, accepted risks and an earlier run are honoured, and the
//! reports say what the rules say, with the line of the element in the file.

use engine::config::EngineConfig;
use engine::defaults;
use engine::evaluate::{evaluate, Report, Status};
use engine::gate::{self, github, md, plain, sarif, AcceptedRisk, Baseline, FailOn, Gate, Reason, Run, Verdict};
use engine::model::Architecture;
use engine::recommend::recommend_from;
use serde_json::{json, Value};

fn cfg(lang: &str) -> EngineConfig {
    defaults::engine_config(lang)
}

fn template(id: &str) -> Architecture {
    defaults::templates().into_iter().find(|t| t.id == id).unwrap().architecture
}

/// Two data centers whose entry points run as one instance each: three critical findings (two single points of failure and an SLO that follows).
fn weak() -> Architecture {
    let mut a = template("two-dc-witness");
    for n in a.nodes.iter_mut().filter(|n| n.id == "gslb" || n.id == "waf") {
        n.props.insert("replicas".into(), json!(1));
    }
    a
}

fn report_of(a: &Architecture) -> Report {
    evaluate(a, &cfg("en"), &[])
}

fn gate_at(fail_on: FailOn) -> Gate {
    Gate { fail_on, ..Gate::default() }
}

#[test]
fn a_sound_design_passes_at_every_level_and_a_weak_one_fails_at_the_level_asked() {
    let sound = report_of(&template("two-dc-witness"));
    for level in [FailOn::Critical, FailOn::Risk] {
        let v = gate::judge(&sound, &gate_at(level));
        assert!(v.passed && v.reasons.is_empty() && v.blocking.is_empty(), "{level:?}: {v:?}");
    }
    let weak = report_of(&weak());
    let v = gate::judge(&weak, &gate_at(FailOn::Critical));
    assert!(!v.passed);
    assert_eq!(v.blocking.len(), 3, "{:?}", v.blocking);
    assert!(v.blocking.iter().any(|id| id == "HA-SPOF-01:gslb") && v.blocking.iter().any(|id| id == "HA-SPOF-01:waf"));
    assert!(matches!(&v.reasons[0], Reason::Findings { fail_on: FailOn::Critical, count: 3, against_baseline: false, .. }));
    // "never" looks at the findings no more
    assert!(gate::judge(&weak, &gate_at(FailOn::Never)).passed);
}

#[test]
fn the_levels_are_nested_critical_inside_risk_inside_no_data() {
    // a design with no requirements has questions the engine cannot answer: "no data" findings, and no critical ones
    let mut a = template("two-dc-witness");
    a.requirements = Default::default();
    let r = report_of(&a);
    let (crit, risk, nodata) = (gate::judge(&r, &gate_at(FailOn::Critical)), gate::judge(&r, &gate_at(FailOn::Risk)), gate::judge(&r, &gate_at(FailOn::NoData)));
    assert!(crit.blocking.len() <= risk.blocking.len() && risk.blocking.len() <= nodata.blocking.len(), "{} {} {}", crit.blocking.len(), risk.blocking.len(), nodata.blocking.len());
    assert!(nodata.blocking.len() > crit.blocking.len(), "the missing requirements are findings of their own: {:?}", nodata.blocking);
    assert!(!nodata.passed);
    assert_eq!(FailOn::parse("No-Data"), Some(FailOn::NoData));
    assert_eq!(FailOn::parse("never"), Some(FailOn::Never));
    assert_eq!(FailOn::parse("sometimes"), None);
}

#[test]
fn a_minimum_score_fails_a_design_that_has_no_finding_at_the_level() {
    let r = report_of(&weak());
    let v = gate::judge(&r, &Gate { fail_on: FailOn::Never, min_score: Some(90.0), baseline: None });
    assert!(!v.passed);
    assert_eq!(v.reasons, vec![Reason::Score { value: r.score.value, min: 90.0 }]);
    assert!(gate::judge(&r, &Gate { fail_on: FailOn::Never, min_score: Some(r.score.value), baseline: None }).passed, "the score may be exactly the minimum");
    // both reasons are said when both hold
    let both = gate::judge(&r, &Gate { fail_on: FailOn::Critical, min_score: Some(99.0), baseline: None });
    assert_eq!(both.reasons.len(), 2);
}

#[test]
fn an_accepted_risk_stays_visible_and_stops_failing_the_check_until_it_expires() {
    let a = weak();
    let base = report_of(&a);
    let critical: Vec<&str> = base.findings.iter().filter(|f| f.status == Status::Critical).map(|f| f.id.as_str()).collect();
    let list = |expires: Option<&str>| -> Vec<AcceptedRisk> {
        critical.iter().map(|id| AcceptedRisk { fingerprint: id.to_string(), reason: "reviewed".into(), by: Some("alice".into()), at: None, expires: expires.map(String::from) }).collect()
    };
    // in force: the findings are accepted, still in the report, and do not fail the check
    let (live, expired) = gate::waivers_in_force(&list(Some("2027-01-01")), "2026-10-13").unwrap();
    assert!(expired.is_empty() && live.len() == critical.len());
    let r = evaluate(&a, &cfg("en"), &live);
    assert!(r.findings.iter().filter(|f| critical.contains(&f.id.as_str())).all(|f| f.accepted.is_some()));
    let v = gate::judge(&r, &gate_at(FailOn::Critical));
    assert!(v.passed, "{v:?}");
    // an acceptance that ended is no acceptance: the findings are open again, and the report can say which ended
    let (live, expired) = gate::waivers_in_force(&list(Some("2026-10-12")), "2026-10-13").unwrap();
    assert!(live.is_empty() && expired.len() == critical.len());
    let v = gate::judge(&evaluate(&a, &cfg("en"), &live), &gate_at(FailOn::Critical));
    assert!(!v.passed);
    // the last day counts, no date never ends, a typo is refused instead of lasting forever
    assert_eq!(gate::waivers_in_force(&list(Some("2026-10-13")), "2026-10-13").unwrap().1.len(), 0);
    assert_eq!(gate::waivers_in_force(&list(None), "2026-10-13").unwrap().0.len(), critical.len());
    assert!(gate::waivers_in_force(&list(Some("next spring")), "2026-10-13").unwrap_err().contains("not a date"));
}

#[test]
fn a_baseline_lets_the_known_through_and_fails_on_what_is_new_or_got_worse() {
    let a = weak();
    let before = report_of(&a);
    let baseline = Baseline::from_report(&before);
    let with = |b: &Baseline| Gate { fail_on: FailOn::Critical, min_score: None, baseline: Some(b.clone()) };

    // nothing changed: every finding is known, the check passes and says how many it let through
    let v = gate::judge(&before, &with(&baseline));
    assert!(v.passed && v.known == 3 && v.blocking.is_empty(), "{v:?}");

    // a change that adds a single point of failure: only that one fails the check
    let mut worse = a.clone();
    worse.nodes.iter_mut().filter(|n| n.id == "app-dc1" || n.id == "app-dc2").for_each(|n| {
        n.props.insert("replicas".into(), json!(1));
    });
    let after = report_of(&worse);
    let v = gate::judge(&after, &with(&baseline));
    let fresh: Vec<_> = after.findings.iter().filter(|f| f.status == Status::Critical && !baseline.open.contains_key(&f.id)).map(|f| f.id.clone()).collect();
    assert_eq!(v.blocking, fresh, "{v:?}");
    assert!(!v.blocking.is_empty() && !v.passed && v.known >= 1);
    assert!(matches!(&v.reasons[0], Reason::Findings { against_baseline: true, .. }));

    // a finding that was only a risk and is critical now is worse, not known
    let mut milder = baseline.clone();
    milder.open.insert("HA-SPOF-01:gslb".into(), Status::Risk);
    let v = gate::judge(&before, &with(&milder));
    assert_eq!(v.blocking, vec!["HA-SPOF-01:gslb".to_string()]);
    // and one that got better (critical before, risk now) is known
    let mut stricter = baseline.clone();
    stricter.open.insert("HA-SPOF-01:gslb".into(), Status::Critical);
    assert!(gate::judge(&before, &with(&stricter)).passed);
}

#[test]
fn a_baseline_is_read_from_the_json_of_an_earlier_run_or_a_bare_report() {
    let r = report_of(&weak());
    let value = serde_json::to_value(&r).unwrap();
    let want = Baseline::from_report(&r);
    // a bare report
    assert_eq!(Baseline::from_json(&value, "a.json").unwrap(), want);
    // the document `check --format json` writes: the entry of the file, or the only one
    let doc = json!({ "version": 1, "results": [{ "file": "a.json", "report": value }, { "file": "b.json", "report": { "findings": [] } }] });
    assert_eq!(Baseline::from_json(&doc, "a.json").unwrap(), want);
    assert!(Baseline::from_json(&doc, "b.json").unwrap().open.is_empty());
    let only = json!({ "results": [{ "file": "elsewhere.json", "report": value }] });
    assert_eq!(Baseline::from_json(&only, "a.json").unwrap(), want, "a single result is the baseline of the single file");
    // a design the baseline does not know is new as a whole
    assert!(Baseline::from_json(&doc, "c.json").unwrap().open.is_empty());
    // accepted findings were not open then
    let mut accepted = value.clone();
    accepted["findings"][0]["accepted"] = json!({ "reason": "reviewed" });
    assert_eq!(Baseline::from_json(&accepted, "a.json").unwrap().open.len(), want.open.len() - 1);
    // something else is refused, with the reason
    assert!(Baseline::from_json(&json!({ "hello": 1 }), "a.json").is_err());
    assert!(Baseline::from_json(&json!({ "findings": 3 }), "a.json").is_err());
}

struct Fixture {
    arch: Architecture,
    cfg: EngineConfig,
    report: Report,
    plan: engine::recommend::Plan,
    verdict: Verdict,
}

fn fixture(lang: &str) -> Fixture {
    let arch = weak();
    let cfg = cfg(lang);
    let report = evaluate(&arch, &cfg, &[]);
    let plan = recommend_from(&arch, &report, &cfg, &[], &Default::default());
    let verdict = gate::judge(&report, &gate_at(FailOn::Critical));
    Fixture { arch, cfg, report, plan, verdict }
}

fn run<'a>(f: &'a Fixture, lang: &'a str, with_plan: bool, line: &'a dyn Fn(&str) -> Option<u32>) -> Run<'a> {
    Run { file: "designs/shop.json", arch: &f.arch, cfg: &f.cfg, report: &f.report, plan: with_plan.then_some(&f.plan), verdict: &f.verdict, lang, expired: &[], line }
}

fn lines(id: &str) -> Option<u32> {
    match id {
        "gslb" => Some(41),
        "waf" => Some(57),
        _ => None,
    }
}

#[test]
fn the_text_report_says_what_is_wrong_where_and_how_to_fix_it_in_the_language_asked() {
    for lang in ["en", "ru"] {
        let f = fixture(lang);
        let r = run(&f, lang, true, &lines);
        let text = plain::render(&r, plain::Style { color: false });
        // the figures, the findings with the words of the rules, the element and its line, the fix, the plan, the verdict
        assert!(text.contains("HA-SPOF-01") && text.contains("AV-SLO-01"), "{text}");
        assert!(text.contains("designs/shop.json:41") && text.contains("designs/shop.json:57"), "{text}");
        assert!(text.contains(&r.say("status.critical", &[])), "{text}");
        let fix = r.fix_label(f.report.findings.iter().find(|x| x.id == "HA-SPOF-01:gslb").unwrap(), &f.report.findings.iter().find(|x| x.id == "HA-SPOF-01:gslb").unwrap().fixes[0]);
        assert!(text.contains(&fix) && fix.contains("GSLB"), "{fix}\n{text}");
        assert!(text.contains(&r.say("first", &[])) && text.contains("→"), "{text}");
        // ("НЕ ПРОЙДЕНО" contains "ПРОЙДЕНО": the verdict is a line of its own)
        assert!(text.lines().any(|l| l == r.say("failed", &[])) && !text.lines().any(|l| l == r.say("passed", &[])), "{text}");
        // the number of the findings listed is the number in the headline, not the number of rules
        assert!(text.contains(&format!("{} 3", r.say("critical", &[]))), "{text}");
        assert!(!text.contains('\u{1b}'), "no colour was asked for");
        // nothing was left unfilled
        assert!(!text.contains("{") && !text.contains("cli.") && !text.contains("chip."), "{text}");
        let coloured = plain::render(&r, plain::Style { color: true });
        assert!(coloured.contains("\u{1b}[1;31m"), "critical is red");
    }
    // the plan is added only when it was asked for
    let f = fixture("en");
    assert!(!plain::render(&run(&f, "en", false, &lines), plain::Style::default()).contains("What to improve first"));
}

#[test]
fn a_design_that_passes_says_so_and_lists_nothing() {
    let arch = template("two-dc-witness");
    let c = cfg("en");
    let report = evaluate(&arch, &c, &[]);
    let verdict = gate::judge(&report, &gate_at(FailOn::Critical));
    let none = |_: &str| None;
    let r = Run { file: "ok.json", arch: &arch, cfg: &c, report: &report, plan: None, verdict: &verdict, lang: "en", expired: &[], line: &none };
    let text = plain::render(&r, plain::Style::default());
    assert!(text.contains("PASSED") && text.contains("No open findings."), "{text}");
    let md = md::render(&r);
    assert!(md.starts_with("## ✅") && md.contains("No open findings."), "{md}");
    assert!(github::render(&r).is_empty());
}

#[test]
fn the_markdown_is_a_table_a_pipeline_can_show_and_the_cells_cannot_break_it() {
    let f = fixture("en");
    let m = md::render(&run(&f, "en", true, &lines));
    assert!(m.starts_with("## ❌") && m.contains("FAILED") && m.contains("| **CRITICAL** | `HA-SPOF-01` |"), "{m}");
    assert!(m.contains("### What to improve first") && m.contains("**All 2 together**"), "{m}");
    // every table row has as many cells as its header
    let cells = |l: &str| l.matches('|').count() - l.matches("\\|").count();
    for table in m.split("\n\n").filter(|b| b.starts_with('|')) {
        let rows: Vec<&str> = table.lines().collect();
        assert!(rows.iter().all(|r| cells(r) == cells(rows[0])), "{table}");
    }
}

#[test]
fn the_annotations_of_a_workflow_carry_the_level_the_file_and_the_line_and_are_escaped() {
    let f = fixture("en");
    let out = github::render(&run(&f, "en", false, &lines));
    let first = out.lines().find(|l| l.contains("HA-SPOF-01 ") && l.contains("line=41")).expect(&out);
    assert!(first.starts_with("::error file=designs/shop.json,line=41,title=HA-SPOF-01 "), "{first}");
    assert_eq!(out.lines().count(), 3);
    // an element without a line is the top of the file; percent signs and line breaks cannot end the command early
    assert!(out.lines().any(|l| l.contains("line=1,") || l.contains("line=57,")));
    assert!(out.contains("99.8%25"), "{out}");
    assert!(!out.trim_end().contains('\n') || out.lines().all(|l| l.starts_with("::")));
}

#[test]
fn the_sarif_has_what_code_scanning_needs_and_points_at_the_line_of_the_element() {
    let f = fixture("en");
    let r = run(&f, "en", false, &lines);
    let tool = sarif::Tool { name: "infra-architect", version: "9.9.9", information_uri: None };
    let doc = sarif::document(&[r], &tool);
    assert_eq!(doc["version"], "2.1.0");
    assert_eq!(doc["$schema"], sarif::SCHEMA);
    let run0 = &doc["runs"][0];
    assert_eq!(run0["tool"]["driver"]["name"], "infra-architect");
    assert_eq!(run0["tool"]["driver"]["version"], "9.9.9");
    let rules = run0["tool"]["driver"]["rules"].as_array().unwrap();
    let ids: Vec<&str> = rules.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["AV-SLO-01", "HA-SPOF-01"], "the rules that have results, once each, in order");
    assert!(rules.iter().all(|r| r["shortDescription"]["text"].as_str().is_some_and(|t| !t.is_empty()) && r["defaultConfiguration"]["level"] == "error"));
    let results = run0["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    let mut prints = std::collections::BTreeSet::new();
    for res in results {
        let i = res["ruleIndex"].as_u64().unwrap() as usize;
        assert_eq!(res["ruleId"], rules[i]["id"], "ruleIndex points at the rule of the result");
        assert_eq!(res["level"], "error");
        assert!(res["message"]["text"].as_str().is_some_and(|t| t.len() > 10));
        let loc = &res["locations"][0]["physicalLocation"];
        assert_eq!(loc["artifactLocation"]["uri"], "designs/shop.json");
        assert!(loc["region"]["startLine"].as_u64().unwrap() >= 1);
        assert!(prints.insert(res["partialFingerprints"]["infraArchitectFinding/v1"].as_str().unwrap().to_string()), "fingerprints tell results apart");
    }
    let gslb = results.iter().find(|x| x["ruleId"] == "HA-SPOF-01" && x["locations"][0]["logicalLocations"][0]["name"] == "gslb").unwrap();
    assert_eq!(gslb["locations"][0]["physicalLocation"]["region"]["startLine"], 41);
    assert!(gslb["properties"]["fixes"][0].as_str().unwrap().contains("GSLB"));
    assert_eq!(run0["artifacts"][0]["location"]["uri"], "designs/shop.json");
}

#[test]
fn sarif_marks_the_levels_and_the_accepted_and_says_so_when_nothing_is_found() {
    let a = weak();
    let c = cfg("en");
    // one of the criticals accepted, a requirement taken away so there are findings of other levels too
    let base = evaluate(&a, &c, &[]);
    let waiver = engine::config::Waiver { fingerprint: "HA-SPOF-01:waf".into(), reason: "the CDN fronts it".into(), by: None, at: None };
    let report = evaluate(&a, &c, std::slice::from_ref(&waiver));
    assert!(base.findings.len() == report.findings.len());
    let verdict = gate::judge(&report, &gate_at(FailOn::Critical));
    let none = |_: &str| None;
    let r = Run { file: ".\\designs\\my design.json", arch: &a, cfg: &c, report: &report, plan: None, verdict: &verdict, lang: "en", expired: &[], line: &none };
    let doc = sarif::document(&[r], &sarif::Tool { name: "t", version: "1", information_uri: Some("https://example.org/ia") });
    assert_eq!(doc["runs"][0]["tool"]["driver"]["informationUri"], "https://example.org/ia");
    let results = doc["runs"][0]["results"].as_array().unwrap();
    let waf = results.iter().find(|x| x["partialFingerprints"]["infraArchitectFinding/v1"].as_str().unwrap().ends_with("HA-SPOF-01:waf")).unwrap();
    assert_eq!(waf["suppressions"][0]["kind"], "external");
    assert_eq!(waf["suppressions"][0]["justification"], "the CDN fronts it");
    assert!(results.iter().filter(|x| x.get("suppressions").is_some()).count() == 1);
    // the path is a URI relative to the root: forward slashes, no leading dot, spaces escaped; an element without a line is the top of the file
    assert_eq!(sarif::uri_of(".\\designs\\my design.json"), "designs/my%20design.json");
    assert_eq!(results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"], "designs/my%20design.json");
    assert_eq!(results[0]["locations"][0]["physicalLocation"]["region"]["startLine"], 1);
    // no runs, no results, and still a document a viewer opens
    let empty = sarif::document(&[], &sarif::Tool { name: "t", version: "1", information_uri: None });
    assert_eq!(empty["runs"][0]["results"], json!([]));
}

#[test]
fn the_figures_count_the_findings_that_are_listed_and_name_the_target() {
    let f = fixture("en");
    let r = run(&f, "en", false, &lines);
    let figures = r.figures().join(" | ");
    assert!(figures.contains("Score 70/100") && figures.contains("critical 3") && figures.contains("risk 0"), "{figures}");
    assert!(figures.contains("Availability ≈ 99.8% (target 99.95%)"), "{figures}");
    assert!(figures.contains("727,900 ₽ a month"), "{figures}");
    assert_eq!(gate::group_thousands(727_900.0, "en"), "727,900");
    assert_eq!(gate::group_thousands(727_900.0, "ru"), "727\u{a0}900");
    assert_eq!(gate::group_thousands(-1_800.0, "en"), "−1,800");
    assert_eq!(gate::group_thousands(12.0, "en"), "12");
    // the name of a design is its own, or its file's
    assert_eq!(r.name(), f.arch.meta.name.clone().unwrap());
    let mut unnamed = f.arch.clone();
    unnamed.meta.name = None;
    let none = |_: &str| None;
    let r2 = Run { file: "designs/shop-prod.json", arch: &unnamed, cfg: &f.cfg, report: &f.report, plan: None, verdict: &f.verdict, lang: "en", expired: &[], line: &none };
    assert_eq!(r2.name(), "shop-prod");
}

#[test]
fn a_verdict_is_a_document_a_tool_can_read() {
    let f = fixture("en");
    let v: Value = serde_json::to_value(&f.verdict).unwrap();
    assert_eq!(v["passed"], false);
    assert_eq!(v["failOn"], "critical");
    assert_eq!(v["reasons"][0]["kind"], "findings");
    assert_eq!(v["reasons"][0]["count"], 3);
    assert_eq!(v["blocking"].as_array().unwrap().len(), 3);
    let s = serde_json::to_value(gate::judge(&f.report, &Gate { fail_on: FailOn::Never, min_score: Some(99.0), baseline: None })).unwrap();
    assert_eq!(s["reasons"][0], json!({ "kind": "score", "value": f.report.score.value, "min": 99.0 }));
}
