//! `infra-architect check` as a pipeline runs it: the real binary, files on disk, exit codes, the reports it writes. No database and no
//! environment: the child process starts with an empty one.

use engine::model::Architecture;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// A directory of its own for the files of one test, removed when the test ends.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("ia-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }

    fn file(&self, name: &str, text: &str) -> String {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().to_string()
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().to_string()
    }

    fn design(&self, name: &str, a: &Architecture) -> String {
        self.file(name, &serde_json::to_string_pretty(a).unwrap())
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn check(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_infra-architect")).arg("check").args(args).env_clear().stdin(Stdio::null()).output().expect("the binary starts")
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("the program exits by itself")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn template(id: &str) -> Architecture {
    engine::defaults::templates().into_iter().find(|t| t.id == id).unwrap().architecture
}

/// Two data centers whose entry points run as one instance each: three critical findings.
fn weak() -> Architecture {
    let mut a = template("two-dc-witness");
    for n in a.nodes.iter_mut().filter(|n| n.id == "gslb" || n.id == "waf") {
        n.props.insert("replicas".into(), json!(1));
    }
    a
}

#[test]
fn a_shipped_reference_design_passes_and_a_weak_one_fails_and_the_exit_code_says_which() {
    let d = Dir::new("verdict");
    let good = d.design("good.json", &template("two-dc-witness"));
    let bad = d.design("bad.json", &weak());

    let o = check(&[&good, "--lang", "en"]);
    assert_eq!(code(&o), 0, "{}{}", out(&o), err(&o));
    assert!(out(&o).contains("PASSED") && out(&o).contains("No open findings."), "{}", out(&o));

    let o = check(&[&bad, "--lang", "en"]);
    assert_eq!(code(&o), 1, "{}{}", out(&o), err(&o));
    let text = out(&o);
    assert!(text.contains("FAILED") && text.contains("HA-SPOF-01") && text.contains("Scale “GSLB + CDN” to 2 instances"), "{text}");
    // the line of the element in the file, so a person can find it
    let line = std::fs::read_to_string(&bad).unwrap().lines().position(|l| l.contains("\"id\": \"gslb\"")).unwrap() + 1;
    assert!(text.contains(&format!("bad.json:{line}")), "line {line} expected in:\n{text}");
    assert!(err(&o).is_empty(), "{}", err(&o));
}

#[test]
fn the_texts_are_in_the_language_of_the_configuration_unless_asked_otherwise() {
    let d = Dir::new("lang");
    let bad = d.design("bad.json", &weak());
    // nothing said: the language the program ships with (the default language of the configuration)
    let o = check(&[&bad]);
    assert!(out(&o).contains("НЕ ПРОЙДЕНО") && out(&o).contains("Единых точек отказа нет"), "{}", out(&o));
    let o = check(&[&bad, "--lang=en"]);
    assert!(out(&o).contains("FAILED") && out(&o).contains("No single point of failure"), "{}", out(&o));
    // and so are the messages about the input
    let o = check(&["--lang", "en", &d.path("nothing.json")]);
    assert!(err(&o).contains("Cannot read"), "{}", err(&o));
    let o = check(&[&d.path("nothing.json")]);
    assert!(err(&o).contains("Не удалось прочитать"), "{}", err(&o));
}

#[test]
fn what_fails_the_check_is_a_threshold_the_caller_sets() {
    let d = Dir::new("thresholds");
    let bad = d.design("bad.json", &weak());
    let good = d.design("good.json", &template("two-dc-witness"));
    assert_eq!(code(&check(&[&bad, "--fail-on", "never"])), 0);
    assert_eq!(code(&check(&[&bad, "--fail-on", "critical"])), 1);
    assert_eq!(code(&check(&[&bad, "--fail-on=risk"])), 1);
    // the score is a threshold of its own, and it can fail a design that has no finding at the level
    assert_eq!(code(&check(&[&bad, "--fail-on", "never", "--min-score", "90"])), 1);
    assert_eq!(code(&check(&[&bad, "--fail-on", "never", "--min-score", "60"])), 0);
    assert_eq!(code(&check(&[&good, "--fail-on", "risk", "--min-score", "95"])), 0);
    let o = check(&[&bad, "--fail-on", "never", "--min-score", "90", "--lang", "en"]);
    assert!(out(&o).contains("Score 70 is below the minimum 90 (--min-score)"), "{}", out(&o));
}

#[test]
fn input_that_cannot_be_checked_is_exit_2_with_the_reason_and_never_a_failed_gate() {
    let d = Dir::new("input");
    let not_json = d.file("not.json", "{ this is not json");
    let not_design = d.file("list.json", "[1, 2, 3]");
    let mut broken = template("two-dc-witness");
    broken.edges[0].target = "nowhere".into();
    let broken = d.design("broken.json", &broken);
    let good = d.design("good.json", &template("two-dc-witness"));

    let o = check(&["--lang", "en", &d.path("missing.json")]);
    assert_eq!(code(&o), 2);
    assert!(out(&o).is_empty() && err(&o).contains("missing.json"), "{}", err(&o));
    let o = check(&["--lang", "en", &not_json]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("not valid JSON") && err(&o).contains("not.json"), "{}", err(&o));
    let o = check(&["--lang", "en", &not_design]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("not an architecture"), "{}", err(&o));
    // a design with errors is not judged: nobody can say what a graph with a link to nowhere survives
    let o = check(&["--lang", "en", &broken]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("cannot be checked") && err(&o).contains("nowhere"), "{}", err(&o));
    assert!(out(&o).is_empty(), "a design that was not judged has no report");
    // the others of the same run are not reported either: one run, one verdict
    assert_eq!(code(&check(&["--lang", "en", &good, &broken])), 2);
    // a number outside its range is such an error too: a design of a million million instances has no verdict, and the reason names the node,
    // the property and the range
    let mut huge = template("two-dc-witness");
    huge.nodes.iter_mut().find(|n| n.id == "app-dc1").unwrap().props.insert("replicas".into(), json!(1_000_000_000_000u64));
    let o = check(&["--lang", "en", &d.design("huge.json", &huge)]);
    assert_eq!(code(&o), 2, "{}", out(&o));
    assert!(err(&o).contains("cannot be checked") && err(&o).contains("app-dc1") && err(&o).contains("replicas") && err(&o).contains("1 to 100000"), "{}", err(&o));
    assert!(out(&o).is_empty());
    let mut slo = template("two-dc-witness");
    slo.requirements.availability_slo = Some(150.0);
    let o = check(&["--lang", "en", &d.design("slo.json", &slo)]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("availabilitySlo"), "{}", err(&o));

    for (args, needle) in [
        (vec!["--lang", "en"], "No architecture file"),
        (vec!["--lang", "en", "--wat", "x.json"], "Unknown option --wat"),
        (vec!["--lang", "en", "x.json", "--format"], "--format needs a value"),
        (vec!["--lang", "en", "x.json", "--format", "yaml"], "“yaml” is not valid"),
        (vec!["--lang", "en", "x.json", "--fail-on", "sometimes"], "“sometimes” is not valid"),
        (vec!["--lang", "en", "x.json", "--min-score", "250"], "not a number from 0 to 100"),
        (vec!["--lang", "en", "x.json", "--today", "tomorrow"], "not a date"),
        (vec!["--lang", "en", "x.json", "--report", "sarif"], "format=path"),
    ] {
        let o = check(&args);
        assert_eq!(code(&o), 2, "{args:?}");
        assert!(err(&o).contains(needle), "{args:?}: {}", err(&o));
    }
}

#[test]
fn the_design_can_come_from_the_standard_input_and_from_a_template_wrapper() {
    let d = Dir::new("stdin");
    let mut child =
        Command::new(env!("CARGO_BIN_EXE_infra-architect")).args(["check", "-", "--lang", "en", "--format", "json"]).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    {
        use std::io::Write;
        child.stdin.take().unwrap().write_all(serde_json::to_string(&weak()).unwrap().as_bytes()).unwrap();
    }
    let o = child.wait_with_output().unwrap();
    assert_eq!(code(&o), 1);
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(doc["results"][0]["file"], "-");
    assert_eq!(doc["results"][0]["passed"], false);

    // an object with an `architecture` field is a template or a project export
    let tpl = engine::defaults::templates().into_iter().find(|t| t.id == "two-dc-witness").unwrap();
    let wrapped = d.file("tpl.json", &serde_json::to_string(&json!({ "id": tpl.id, "name": "x", "architecture": tpl.architecture })).unwrap());
    assert_eq!(code(&check(&[&wrapped])), 0);
}

#[test]
fn the_json_output_is_the_whole_report_and_the_baseline_of_the_next_run() {
    let d = Dir::new("baseline");
    let bad = d.design("bad.json", &weak());
    let o = check(&[&bad, "--format", "json", "--recommend"]);
    assert_eq!(code(&o), 1);
    let doc: Value = serde_json::from_slice(&o.stdout).expect("the output is JSON and nothing else");
    assert_eq!((doc["version"].clone(), doc["passed"].clone()), (json!(1), json!(false)));
    let r = &doc["results"][0];
    assert_eq!(r["verdict"]["blocking"].as_array().unwrap().len(), 3);
    assert!(r["report"]["score"]["value"].as_f64().unwrap() > 0.0 && r["report"]["findings"].as_array().unwrap().len() >= 3);
    assert_eq!(r["plan"]["items"][0]["id"], "set_replicas:gslb:replicas");
    assert!(r.get("expiredAcceptances").is_none());
    let baseline = d.file("baseline.json", &out(&o));

    // the same design against its own baseline: everything is known, the check passes and says how many it let through
    let o = check(&[&bad, "--baseline", &baseline, "--lang", "en"]);
    assert_eq!(code(&o), 0, "{}", out(&o));
    assert!(out(&o).contains("PASSED") && out(&o).contains("Findings the baseline already had, which do not count: 3"), "{}", out(&o));

    // a change that makes it worse fails on what is new only
    let mut worse = weak();
    worse.nodes.iter_mut().filter(|n| n.id == "app-dc1" || n.id == "app-dc2").for_each(|n| {
        n.props.insert("replicas".into(), json!(1));
    });
    let worse = d.design("bad.json", &worse);
    let o = check(&[&worse, "--baseline", &baseline, "--lang", "en"]);
    assert_eq!(code(&o), 1);
    assert!(out(&o).contains("new against the baseline"), "{}", out(&o));
    // a design the baseline has never seen has no history: it has to pass in full
    let other = d.design("other.json", &weak());
    let o = check(&[&other, "--baseline", &baseline, "--lang", "en"]);
    assert_eq!(code(&o), 0, "the baseline names bad.json only, but holds a single result: it is the baseline of the single file");
    let both =
        d.file("two.json", &serde_json::to_string(&json!({ "results": [{ "file": "elsewhere.json", "report": { "findings": [] } }, { "file": "else.json", "report": { "findings": [] } }] })).unwrap());
    assert_eq!(code(&check(&[&other, "--baseline", &both])), 1);
    // a baseline that is not one is refused
    let junk = d.file("junk.json", "{\"hello\": 1}");
    let o = check(&[&bad, "--baseline", &junk, "--lang", "en"]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("Baseline") || err(&o).contains("neither"), "{}", err(&o));
}

#[test]
fn accepted_risks_kept_in_a_file_stop_failing_the_check_until_they_expire() {
    let d = Dir::new("waivers");
    let bad = d.design("bad.json", &weak());
    let accepted = |expires: &str| {
        let list: Vec<Value> =
            ["AV-SLO-01:slo", "HA-SPOF-01:gslb", "HA-SPOF-01:waf"].iter().map(|f| json!({ "fingerprint": f, "reason": "reviewed with the owner", "by": "alice", "expires": expires })).collect();
        d.file("waivers.json", &serde_json::to_string(&list).unwrap())
    };
    let o = check(&[&bad, "--waivers", &accepted("2099-01-01"), "--lang", "en"]);
    assert_eq!(code(&o), 0, "{}", out(&o));
    assert!(out(&o).contains("PASSED") && out(&o).contains("accepted 3"), "{}", out(&o));
    // an acceptance that ended is no acceptance, and the report says which
    let o = check(&[&bad, "--waivers", &accepted("2020-01-01"), "--lang", "en"]);
    assert_eq!(code(&o), 1);
    assert!(out(&o).contains("The acceptance of a risk has expired: AV-SLO-01:slo, HA-SPOF-01:gslb, HA-SPOF-01:waf"), "{}", out(&o));
    // the date they are judged on is the caller's to give: the same file, a day before the end
    let o = check(&[&bad, "--waivers", &accepted("2026-10-13"), "--today", "2026-10-13", "--lang", "en"]);
    assert_eq!(code(&o), 0);
    let o = check(&[&bad, "--waivers", &accepted("2026-10-13"), "--today", "2026-10-14", "--lang", "en"]);
    assert_eq!(code(&o), 1);
    // a typo in a date does not make an acceptance last for ever
    let o = check(&[&bad, "--waivers", &accepted("someday"), "--lang", "en"]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("not a date"), "{}", err(&o));
    // an object with a `waivers` list is read too
    let wrapped = d.file("w2.json", &serde_json::to_string(&json!({ "waivers": [{ "fingerprint": "HA-SPOF-01:gslb", "reason": "r" }] })).unwrap());
    assert_eq!(code(&check(&[&bad, "--waivers", &wrapped, "--fail-on", "never"])), 0);
}

#[test]
fn reports_are_written_to_files_next_to_the_text_and_markdown_is_appended() {
    let d = Dir::new("reports");
    let bad = d.design("bad.json", &weak());
    let (sarif, summary) = (d.path("results.sarif"), d.path("summary.md"));
    std::fs::write(&summary, "# an earlier step wrote this\n").unwrap();
    for _ in 0..2 {
        let o = check(&[&bad, "--lang", "en", "--report", &format!("sarif={sarif}"), "--report", &format!("markdown={summary}"), "--recommend"]);
        assert_eq!(code(&o), 1);
        assert!(out(&o).contains("HA-SPOF-01"), "the text still goes to the output: {}", out(&o));
    }
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&sarif).unwrap()).unwrap();
    assert_eq!(doc["version"], "2.1.0");
    assert_eq!(doc["runs"][0]["results"].as_array().unwrap().len(), 3, "a file that is written is replaced, not added to");
    let md = std::fs::read_to_string(&summary).unwrap();
    assert!(md.starts_with("# an earlier step wrote this\n"), "{md}");
    assert_eq!(md.matches("## ❌").count(), 2, "markdown is appended: two runs, two sections");
    assert!(md.contains("### What to improve first"));
    // a place that cannot be written to is an input problem
    let o = check(&[&bad, "--lang", "en", "--report", &format!("sarif={}", d.path("no/such/dir/x.sarif"))]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("Cannot write"), "{}", err(&o));
}

#[test]
fn several_designs_are_judged_in_one_run_and_the_worst_decides() {
    let d = Dir::new("many");
    let good = d.design("good.json", &template("two-dc-witness"));
    let bad = d.design("bad.json", &weak());
    let o = check(&[&good, &bad, "--format", "json"]);
    assert_eq!(code(&o), 1);
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    let results = doc["results"].as_array().unwrap();
    assert_eq!((results.len(), results[0]["passed"].clone(), results[1]["passed"].clone()), (2, json!(true), json!(false)));
    assert_eq!(code(&check(&[&good, &good])), 0);
    // one SARIF document, one artifact each
    let o = check(&[&good, &bad, "--format", "sarif"]);
    let sarif: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(sarif["runs"][0]["artifacts"].as_array().unwrap().len(), 2);
    assert!(sarif["runs"][0]["results"].as_array().unwrap().iter().all(|r| r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"].as_str().unwrap().ends_with("bad.json")));
}

#[test]
fn the_annotations_of_a_workflow_and_the_summary_of_a_job_are_formats_like_the_rest() {
    let d = Dir::new("formats");
    let bad = d.design("bad.json", &weak());
    let o = check(&[&bad, "--format", "github", "--lang", "en"]);
    assert_eq!(code(&o), 1);
    let text = out(&o);
    assert_eq!(text.lines().filter(|l| l.starts_with("::error file=")).count(), 3, "{text}");
    assert!(text.contains(",line=") && text.contains("title=HA-SPOF-01 "), "{text}");
    let o = check(&[&bad, "--format", "markdown", "--lang", "en"]);
    assert!(out(&o).starts_with("## ❌") && out(&o).contains("| **CRITICAL** |"), "{}", out(&o));
}

#[test]
fn the_configuration_of_an_instance_decides_what_a_design_is_judged_by() {
    let d = Dir::new("config");
    let bad = d.design("bad.json", &weak());
    // the backup an administrator downloads: only what was changed. The single-point-of-failure rule is switched off.
    let rules: Vec<Value> = serde_json::from_str(engine::defaults::RULES_JSON).unwrap();
    let mut spof = rules.iter().find(|r| r["id"] == "HA-SPOF-01").unwrap().clone();
    spof["enabled"] = json!(false);
    let backup = |items: Value, settings: Value| d.file("config.json", &serde_json::to_string(&json!({ "format": "infra-architect-config/1", "settings": settings, "items": items })).unwrap());

    let o = check(&[&bad, "--lang", "en", "--format", "json", "--config", &backup(json!([{ "kind": "rule", "id": "HA-SPOF-01", "data": spof, "origin": "default" }]), json!({}))]);
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    let ids: Vec<&str> = doc["results"][0]["report"]["findings"].as_array().unwrap().iter().map(|f| f["ruleId"].as_str().unwrap()).collect();
    assert!(!ids.contains(&"HA-SPOF-01") && ids.contains(&"AV-SLO-01"), "{ids:?}");
    // the same design as shipped has them
    let o = check(&[&bad, "--lang", "en", "--format", "json"]);
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(doc["results"][0]["report"]["findings"].as_array().unwrap().iter().any(|f| f["ruleId"] == "HA-SPOF-01"));

    // a setting of the instance is applied: the size limit of a design
    let o = check(&[&bad, "--lang", "en", "--config", &backup(json!([]), json!({ "limits.maxNodes": 10 }))]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("too many nodes"), "{}", err(&o));

    // a backup that does not hold together is refused with the reason, not half applied
    let o = check(&[&bad, "--lang", "en", "--config", &backup(json!([{ "kind": "rule", "id": "X", "data": { "id": "X" } }]), json!({}))]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("rule:X"), "{}", err(&o));
    let o = check(&[&bad, "--lang", "en", "--config", &backup(json!([]), json!({ "no.such.setting": 1 }))]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("unknown setting"), "{}", err(&o));
    let o = check(&[&bad, "--lang", "en", "--config", &d.file("other.json", "{\"format\": \"something/9\"}")]);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("unknown configuration format"), "{}", err(&o));
}

#[test]
fn help_and_version_need_no_file_and_the_help_follows_the_language() {
    let o = check(&["--help"]);
    assert_eq!(code(&o), 0);
    assert!(out(&o).contains("Использование: infra-architect check") && out(&o).contains("Коды выхода"), "{}", out(&o));
    let o = check(&["-h", "--lang", "en"]);
    assert!(out(&o).contains("Usage: infra-architect check") && out(&o).contains("Exit codes"), "{}", out(&o));
    // every option the program reads is in the help (a new option cannot be forgotten)
    for opt in ["--format", "--report", "--fail-on", "--min-score", "--baseline", "--waivers", "--recommend", "--config", "--lang", "--today", "--color"] {
        assert!(out(&o).contains(opt), "{opt}");
    }
    let o = check(&["--version"]);
    assert_eq!(code(&o), 0);
    assert!(out(&o).starts_with("infra-architect "), "{}", out(&o));
}

#[test]
fn the_shipped_reference_designs_pass_their_own_gate() {
    // what a pipeline of this repository does with the designs it ships: if one of them stops passing, the engine and the data disagree
    let d = Dir::new("shipped");
    let mut failed = vec![];
    for t in engine::defaults::templates() {
        let file = d.design(&format!("{}.json", t.id), &t.architecture);
        let o = check(&[&file, "--lang", "en", "--fail-on", "critical", "--today", "2026-10-13"]);
        if code(&o) != 0 {
            failed.push(format!("{}: exit {}\n{}{}", t.id, code(&o), out(&o), err(&o)));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let _ = Path::new("");
}
