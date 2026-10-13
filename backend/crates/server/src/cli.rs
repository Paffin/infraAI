//! `infra-architect check`: the verdict on architectures kept in files, for a pipeline (docs/CLI.md).
//!
//! No database and no network: the rules, the catalog and the engine settings are the ones shipped in the binary, or the ones of an
//! instance when its configuration backup is given (`--config`). What is judged, the thresholds and the report formats are the engine's
//! own (`engine::gate`); this module reads the files, parses the options, writes the reports and picks the exit code.

use crate::api_support::limits;
use crate::config_store::Loaded;
use engine::evaluate::evaluate;
use engine::gate::{self, plain, sarif, say, AcceptedRisk, Baseline, FailOn, Gate, Run};
use engine::model::{validate, Architecture, IssueLevel};
use engine::recommend::{recommend_from, Plan};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{IsTerminal, Read, Write};
use std::sync::RwLock;

/// Every design passed.
pub const EXIT_PASSED: i32 = 0;
/// Some design did not pass the gate.
pub const EXIT_FAILED: i32 = 1;
/// Something could not be checked: a bad option, an unreadable file, a design with errors.
pub const EXIT_INPUT: i32 = 2;

const USAGE_EN: &str = r#"infra-architect check — the verdict on architectures kept in files, for a pipeline

Usage: infra-architect check [options] <architecture.json>...      ("-" reads the standard input)

A file holds an architecture (the "JSON" export of the editor), or an object with an "architecture" field (a template, a project export).
Nothing is sent anywhere and no database is needed: the rules, the catalog and the engine settings are the ones shipped in this program,
or the ones of your instance when you give its configuration backup (--config).

Options:
  --format <text|json|sarif|markdown|github>   what is printed (default: text)
  --report <format>=<path>                     also write a report to a file; repeatable. Markdown is appended to the file,
                                               so $GITHUB_STEP_SUMMARY can be given
  --fail-on <critical|risk|no-data|never>      the level of an open finding that fails the check (default: critical)
  --min-score <0-100>                          fail when the score is lower
  --baseline <file>                            fail only on findings that are new against the JSON output of an earlier run
  --waivers <file>                             accepted risks: a JSON list of {fingerprint, reason, by, at, expires (YYYY-MM-DD)}
  --recommend                                  add "what to improve first": the engine tries each fix on a copy and measures it
  --config <file>                              the configuration backup of an instance (Administration, Backup); default: as shipped
  --lang <ru|en>                               language of the texts (default: the default language of the configuration)
  --today <YYYY-MM-DD>                         the date support dates are judged on (default: today, UTC)
  --color <auto|always|never>                  colours of the text report (default: auto: on a terminal, unless NO_COLOR is set)
  -h, --help                                   this text
  -V, --version                                the version

Exit codes: 0 passed · 1 the check failed · 2 the input could not be checked (a bad option, an unreadable file, a design with errors)
"#;

const USAGE_RU: &str = r#"infra-architect check — вердикт по схемам архитектуры из файлов, для конвейера

Использование: infra-architect check [параметры] <architecture.json>...      («-» — читать стандартный ввод)

В файле — схема (экспорт «JSON» из редактора) или объект с полем "architecture" (шаблон, выгрузка проекта). Никуда ничего не отправляется,
база данных не нужна: правила, каталог и настройки движка — те, что вшиты в программу, или те, что у вашего экземпляра, если дать его
резервную копию конфигурации (--config).

Параметры:
  --format <text|json|sarif|markdown|github>   что печатается (по умолчанию text)
  --report <формат>=<путь>                     дополнительно записать отчёт в файл; можно несколько раз. Markdown дописывается
                                               в конец файла, поэтому подойдёт $GITHUB_STEP_SUMMARY
  --fail-on <critical|risk|no-data|never>      уровень открытой находки, при котором проверка не пройдена (по умолчанию critical)
  --min-score <0-100>                          не пройдено, если оценка ниже
  --baseline <файл>                            не пройдено только из-за находок, которых не было в JSON-выводе прошлого запуска
  --waivers <файл>                             принятые риски: JSON-список {fingerprint, reason, by, at, expires (ГГГГ-ММ-ДД)}
  --recommend                                  добавить «что улучшить в первую очередь»: движок пробует каждое исправление на копии и измеряет
  --config <файл>                              резервная копия конфигурации экземпляра (Администрирование, Бэкап); по умолчанию — как поставляется
  --lang <ru|en>                               язык текстов (по умолчанию — язык конфигурации по умолчанию)
  --today <ГГГГ-ММ-ДД>                         дата, на которую судятся сроки поддержки (по умолчанию сегодня, UTC)
  --color <auto|always|never>                  цвета текстового отчёта (по умолчанию auto: в терминале, если не задан NO_COLOR)
  -h, --help                                   эта справка
  -V, --version                                версия

Коды выхода: 0 — пройдено · 1 — проверка не пройдена · 2 — проверить не удалось (неверный параметр, нечитаемый файл, ошибки в схеме)
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
    Sarif,
    Markdown,
    Github,
}

impl Format {
    const NAMES: &'static str = "text, json, sarif, markdown, github";

    fn parse(s: &str) -> Option<Format> {
        match s.trim().to_ascii_lowercase().as_str() {
            "text" => Some(Format::Text),
            "json" => Some(Format::Json),
            "sarif" => Some(Format::Sarif),
            "markdown" | "md" => Some(Format::Markdown),
            "github" => Some(Format::Github),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Auto,
    Always,
    Never,
}

#[derive(Debug)]
struct Options {
    files: Vec<String>,
    format: Format,
    reports: Vec<(Format, String)>,
    fail_on: FailOn,
    min_score: Option<f64>,
    baseline: Option<String>,
    waivers: Option<String>,
    recommend: bool,
    config: Option<String>,
    lang: Option<String>,
    today: Option<String>,
    color: Color,
}

#[derive(Debug)]
enum Parsed {
    Run(Box<Options>),
    Help,
    Version,
}

/// The language the messages before the configuration is read are in: `--lang`, else the language the program ships with.
fn lang_hint(args: &[String]) -> String {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix("--lang=") {
            return v.to_string();
        }
        if a == "--lang" {
            if let Some(v) = it.next() {
                return v.clone();
            }
        }
    }
    "ru".to_string()
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let lang = lang_hint(args);
    let m = |key: &str, vars: &[(&str, Value)]| say(&lang, key, vars);
    let mut o = Options {
        files: vec![],
        format: Format::Text,
        reports: vec![],
        fail_on: FailOn::Critical,
        min_score: None,
        baseline: None,
        waivers: None,
        recommend: false,
        config: None,
        lang: None,
        today: None,
        color: Color::Auto,
    };
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        i += 1;
        if raw == "-" || !raw.starts_with('-') {
            o.files.push(raw.clone());
            continue;
        }
        // `--name=value` and `--name value`
        let (name, inline) = match raw.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (raw.clone(), None),
        };
        match name.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "--recommend" => {
                o.recommend = true;
                continue;
            }
            "--format" | "--report" | "--fail-on" | "--min-score" | "--baseline" | "--waivers" | "--config" | "--lang" | "--today" | "--color" => {}
            _ => return Err(m("cli.err.unknown_option", &[("name", json!(raw))])),
        }
        let value = match inline {
            Some(v) => v,
            None => {
                let v = args.get(i).ok_or_else(|| m("cli.err.needs_value", &[("name", json!(name))]))?.clone();
                i += 1;
                v
            }
        };
        let bad = |allowed: &str| m("cli.err.bad_value", &[("name", json!(name)), ("value", json!(value)), ("allowed", json!(allowed))]);
        match name.as_str() {
            "--format" => o.format = Format::parse(&value).ok_or_else(|| bad(Format::NAMES))?,
            "--report" => {
                let (f, path) = value.split_once('=').ok_or_else(|| m("cli.err.report_spec", &[]))?;
                o.reports.push((Format::parse(f).ok_or_else(|| bad(Format::NAMES))?, path.to_string()));
            }
            "--fail-on" => o.fail_on = FailOn::parse(&value).ok_or_else(|| bad("critical, risk, no-data, never"))?,
            "--min-score" => {
                let n: f64 = value.trim().parse().map_err(|_| m("cli.err.min_score", &[("value", json!(value))]))?;
                if !(0.0..=100.0).contains(&n) {
                    return Err(m("cli.err.min_score", &[("value", json!(value))]));
                }
                o.min_score = Some(n);
            }
            "--baseline" => o.baseline = Some(value),
            "--waivers" => o.waivers = Some(value),
            "--config" => o.config = Some(value),
            "--lang" => o.lang = Some(value),
            "--today" => {
                if engine::lifecycle::parse_date(&value).is_none() {
                    return Err(m("cli.err.today", &[("value", json!(value))]));
                }
                o.today = Some(value);
            }
            "--color" => {
                o.color = match value.as_str() {
                    "auto" => Color::Auto,
                    "always" => Color::Always,
                    "never" => Color::Never,
                    _ => return Err(bad("auto, always, never")),
                }
            }
            _ => unreachable!("every option that takes a value is listed above"),
        }
    }
    if o.files.is_empty() {
        return Err(m("cli.err.no_files", &[]));
    }
    Ok(Parsed::Run(Box::new(o)))
}

/// The date the engine is told it is (it reads no clock of its own; a function pointer cannot capture the date).
static TODAY: RwLock<String> = RwLock::new(String::new());

fn clock() -> String {
    TODAY.read().map(|d| d.clone()).unwrap_or_default()
}

fn read_text(path: &str) -> std::io::Result<String> {
    if path == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        std::fs::read_to_string(path)
    }
}

fn read_json(path: &str, lang: &str) -> Result<(String, Value), String> {
    let text = read_text(path).map_err(|e| say(lang, "cli.err.read", &[("file", json!(path)), ("error", json!(e.to_string()))]))?;
    let v = serde_json::from_str(&text).map_err(|e| say(lang, "cli.err.json", &[("file", json!(path)), ("error", json!(e.to_string()))]))?;
    Ok((text, v))
}

/// Where each element of the design lies in the file: the line of the first `"id": "…"` that names it (every line of a minified file is the first).
fn element_lines(text: &str) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    for (n, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find("\"id\"") {
            rest = &rest[at + 4..];
            let after = rest.trim_start();
            let Some(after) = after.strip_prefix(':') else { continue };
            let Some(after) = after.trim_start().strip_prefix('"') else { continue };
            if let Some(end) = after.find('"') {
                out.entry(after[..end].to_string()).or_insert(n as u32 + 1);
            }
        }
    }
    out
}

/// The architecture in a file: the file itself, or its `architecture` field.
fn architecture_of(v: &Value) -> Result<Architecture, String> {
    let inner = match v.get("architecture") {
        Some(a) if a.is_object() => a,
        _ => v,
    };
    let mut a: Architecture = serde_json::from_value(inner.clone()).map_err(|e| e.to_string())?;
    // `proposed` marks the nodes of a preview of an AI proposal; it is never part of a design that is kept
    for n in a.nodes.iter_mut() {
        n.proposed = false;
    }
    Ok(a)
}

/// One design, checked.
struct Checked {
    file: String,
    arch: Architecture,
    report: engine::evaluate::Report,
    plan: Option<Plan>,
    verdict: gate::Verdict,
    lines: HashMap<String, u32>,
}

fn accepted_risks(path: &str, lang: &str) -> Result<Vec<AcceptedRisk>, String> {
    let (_, v) = read_json(path, lang)?;
    let list = match &v {
        Value::Array(_) => v.clone(),
        other => other.get("waivers").or_else(|| other.get("accepted")).cloned().unwrap_or(Value::Null),
    };
    serde_json::from_value(list).map_err(|e| say(lang, "cli.err.waivers", &[("file", json!(path)), ("error", json!(e.to_string()))]))
}

fn run(o: &Options) -> Result<i32, String> {
    let boot_lang = o.lang.clone().unwrap_or_else(|| "ru".to_string());
    let loaded = match &o.config {
        Some(path) => {
            let (_, export) = read_json(path, &boot_lang)?;
            Loaded::from_export(&export).map_err(|e| say(&boot_lang, "cli.err.config", &[("file", json!(path)), ("error", json!(e))]))?
        }
        None => Loaded::embedded_defaults(),
    };
    let lang = o.lang.clone().unwrap_or_else(|| loaded.settings.str("app.defaultLanguage"));
    let today = o.today.clone().unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
    if let Ok(mut t) = TODAY.write() {
        *t = today.clone();
    }
    let mut ec = (*loaded.engine_config(&lang)).clone();
    ec.today = clock;
    let lim = limits(&loaded);

    let (waivers, expired) = match &o.waivers {
        Some(path) => gate::waivers_in_force(&accepted_risks(path, &lang)?, &today).map_err(|e| say(&lang, "cli.err.waivers", &[("file", json!(path)), ("error", json!(e))]))?,
        None => (vec![], vec![]),
    };
    let baseline_doc = match &o.baseline {
        Some(path) => Some((path.clone(), read_json(path, &lang)?.1)),
        None => None,
    };

    let mut checked: Vec<Checked> = vec![];
    let mut invalid = vec![];
    for file in &o.files {
        let (text, doc) = read_json(file, &lang)?;
        let arch = architecture_of(&doc).map_err(|e| say(&lang, "cli.err.architecture", &[("file", json!(file)), ("error", json!(e))]))?;
        let errors: Vec<_> = validate(&arch, &ec.catalog, &lim).into_iter().filter(|i| i.level == IssueLevel::Error).collect();
        if !errors.is_empty() {
            let mut msg = say(&lang, "cli.err.invalid", &[("file", json!(file))]);
            for e in errors.iter().take(20) {
                msg.push_str(&format!("\n  - {}", e.message));
            }
            if errors.len() > 20 {
                msg.push_str(&format!("\n  … +{}", errors.len() - 20));
            }
            invalid.push(msg);
            continue;
        }
        let report = evaluate(&arch, &ec, &waivers);
        let plan = o.recommend.then(|| recommend_from(&arch, &report, &ec, &waivers, &lim));
        let baseline = match &baseline_doc {
            Some((path, v)) => Some(Baseline::from_json(v, file).map_err(|e| say(&lang, "cli.err.baseline", &[("file", json!(path)), ("error", json!(e))]))?),
            None => None,
        };
        let verdict = gate::judge(&report, &Gate { fail_on: o.fail_on, min_score: o.min_score, baseline });
        checked.push(Checked { file: file.clone(), arch, report, plan, verdict, lines: element_lines(&text) });
    }
    if !invalid.is_empty() {
        return Err(invalid.join("\n"));
    }

    let line_of: Vec<Box<dyn Fn(&str) -> Option<u32> + '_>> = checked.iter().map(|c| Box::new(move |id: &str| c.lines.get(id).copied()) as Box<dyn Fn(&str) -> Option<u32> + '_>).collect();
    let runs: Vec<Run> = checked
        .iter()
        .zip(line_of.iter())
        .map(|(c, line)| Run { file: &c.file, arch: &c.arch, cfg: &ec, report: &c.report, plan: c.plan.as_ref(), verdict: &c.verdict, lang: &lang, expired: &expired, line: line.as_ref() })
        .collect();

    let color = match o.color {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
    };
    for (format, path) in &o.reports {
        let doc = render(*format, &runs, false);
        write_report(*format, path, &doc).map_err(|e| say(&lang, "cli.err.write", &[("file", json!(path)), ("error", json!(e.to_string()))]))?;
    }
    let out = render(o.format, &runs, color);
    // a closed pipe (`| head`) is not an error of the check
    let _ = std::io::stdout().lock().write_all(out.as_bytes());

    Ok(if checked.iter().all(|c| c.verdict.passed) { EXIT_PASSED } else { EXIT_FAILED })
}

fn write_report(format: Format, path: &str, doc: &str) -> std::io::Result<()> {
    if format == Format::Markdown {
        // a job summary is a file that other steps write to as well
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        f.write_all(doc.as_bytes())
    } else {
        std::fs::write(path, doc)
    }
}

fn render(format: Format, runs: &[Run], color: bool) -> String {
    match format {
        Format::Text => runs.iter().map(|r| plain::render(r, plain::Style { color })).collect::<Vec<_>>().join("\n"),
        Format::Markdown => runs.iter().map(gate::md::render).collect::<Vec<_>>().join("\n"),
        Format::Github => runs.iter().map(gate::github::render).collect::<String>(),
        Format::Sarif => {
            let tool = sarif::Tool { name: "infra-architect", version: env!("CARGO_PKG_VERSION"), information_uri: None };
            let mut s = serde_json::to_string_pretty(&sarif::document(runs, &tool)).unwrap_or_default();
            s.push('\n');
            s
        }
        Format::Json => {
            let results: Vec<Value> = runs
                .iter()
                .map(|r| {
                    let mut v = json!({ "file": r.file, "name": r.name(), "passed": r.verdict.passed, "verdict": r.verdict, "report": r.report });
                    if let Some(p) = r.plan {
                        v["plan"] = json!(p);
                    }
                    if !r.expired.is_empty() {
                        v["expiredAcceptances"] = json!(r.expired);
                    }
                    v
                })
                .collect();
            let doc = json!({ "version": 1, "tool": { "name": "infra-architect", "version": env!("CARGO_PKG_VERSION") }, "passed": runs.iter().all(|r| r.verdict.passed), "results": results });
            let mut s = serde_json::to_string_pretty(&doc).unwrap_or_default();
            s.push('\n');
            s
        }
    }
}

/// `infra-architect check …` (the arguments after `check`): prints the report and returns the exit code.
pub fn check(args: &[String]) -> i32 {
    let lang = lang_hint(args);
    match parse(args) {
        Ok(Parsed::Help) => {
            print!("{}", if lang == "ru" { USAGE_RU } else { USAGE_EN });
            EXIT_PASSED
        }
        Ok(Parsed::Version) => {
            println!("infra-architect {}", env!("CARGO_PKG_VERSION"));
            EXIT_PASSED
        }
        Ok(Parsed::Run(o)) => match run(&o) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("infra-architect check: {e}");
                EXIT_INPUT
            }
        },
        Err(e) => {
            eprintln!("infra-architect check: {e}");
            EXIT_INPUT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn options_are_read_in_both_forms_and_checked() {
        let Ok(Parsed::Run(o)) = parse(&args(&["a.json", "--fail-on=risk", "--min-score", "70", "--format", "sarif", "--report", "markdown=out.md", "--recommend", "-"])) else { panic!("parsed") };
        assert_eq!(o.files, ["a.json", "-"]);
        assert_eq!((o.fail_on, o.min_score, o.format, o.recommend), (FailOn::Risk, Some(70.0), Format::Sarif, true));
        assert_eq!(o.reports, [(Format::Markdown, "out.md".to_string())]);
        for bad in [
            &["a.json", "--nope"][..],
            &["a.json", "--fail-on", "sometimes"],
            &["a.json", "--min-score", "101"],
            &["a.json", "--min-score"],
            &["a.json", "--today", "tomorrow"],
            &["--format", "text"],
            &["a.json", "--report", "sarif"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
        assert!(matches!(parse(&args(&["-h"])), Ok(Parsed::Help)) && matches!(parse(&args(&["--version"])), Ok(Parsed::Version)));
    }

    #[test]
    fn messages_follow_the_language() {
        assert!(parse(&args(&["--lang", "en", "a.json", "--nope"])).unwrap_err().starts_with("Unknown option --nope"));
        assert!(parse(&args(&["a.json", "--nope"])).unwrap_err().starts_with("Неизвестный параметр --nope"));
    }

    #[test]
    fn the_line_of_an_element_is_where_its_id_is_first_written() {
        let text = "{\n \"nodes\": [\n  {\"id\": \"gslb\", \"name\": \"G\"},\n  {\n   \"id\":\"app\"\n  }\n ],\n \"edges\": [{ \"id\" : \"e1\" }]\n}";
        let lines = element_lines(text);
        assert_eq!((lines["gslb"], lines["app"], lines["e1"]), (3, 5, 8));
        // a minified file has one line
        assert_eq!(element_lines("{\"nodes\":[{\"id\":\"a\"},{\"id\":\"b\"}]}")["b"], 1);
    }
}
