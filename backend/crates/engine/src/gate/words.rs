//! The few words a pipeline report says in its own voice. Everything about the design itself – rule titles, what is wrong, how to fix it –
//! is the text of the rules and is rendered from them; these are the headings and sentences around it. A language that has no column
//! here is read in English, like every other text of the engine.

use serde_json::Value;

/// `(key, ru, en)`.
const TEXTS: &[(&str, &str, &str)] = &[
    ("score", "Оценка", "Score"),
    ("confidence", "достоверность", "confidence"),
    ("critical", "критично", "critical"),
    ("risk", "риск", "risk"),
    ("no_data", "нет данных", "no data"),
    ("accepted", "принято", "accepted"),
    ("availability", "Доступность", "Availability"),
    ("target", "цель", "target"),
    ("cost", "Стоимость", "Cost"),
    ("per_month", "в месяц", "a month"),
    ("status.critical", "КРИТИЧНО", "CRITICAL"),
    ("status.risk", "РИСК", "RISK"),
    ("status.no_data", "НЕТ ДАННЫХ", "NO DATA"),
    ("level.critical", "критично", "critical"),
    ("level.risk", "риск", "risk"),
    ("level.no_data", "нет данных", "no data"),
    ("where", "Где", "Where"),
    ("fix", "Исправление", "Fix"),
    ("fill_in", "Чтобы проверить, заполните", "To verify, fill in"),
    ("first", "Что улучшить в первую очередь", "What to improve first"),
    ("together", "Все {n} вместе", "All {n} together"),
    ("manual", "Для человека: {n} {n|plural:находка,находки,находок} — движок не умеет исправлять их сам.", "For a person: {n} {n|plural:finding,findings} the engine cannot fix by itself."),
    ("truncated", "Проверено исправлений: {n} — предел настройки рекомендаций; остальные не пробовались.", "Fixes tried: {n} — the limit of the recommendations setting; the rest were not tried."),
    ("passed", "ПРОЙДЕНО", "PASSED"),
    ("failed", "НЕ ПРОЙДЕНО", "FAILED"),
    ("reason.findings", "Открытых находок уровня «{level}» и тяжелее: {n} (--fail-on {fail_on})", "Open findings of level “{level}” or worse: {n} (--fail-on {fail_on})"),
    (
        "reason.findings.new",
        "Новых относительно базы находок уровня «{level}» и тяжелее: {n} (--fail-on {fail_on})",
        "Findings of level “{level}” or worse that are new against the baseline: {n} (--fail-on {fail_on})",
    ),
    ("reason.score", "Оценка {value} ниже минимума {min} (--min-score)", "Score {value} is below the minimum {min} (--min-score)"),
    ("known", "Находок, которые уже были в базе и не мешают: {n}", "Findings the baseline already had, which do not count: {n}"),
    ("expired", "Срок принятия риска истёк: {ids}", "The acceptance of a risk has expired: {ids}"),
    ("issues", "Замечания к схеме", "Notes on the design"),
    ("none", "Открытых находок нет.", "No open findings."),
    ("chip.critical", "критичных {from} → {to}", "critical {from} → {to}"),
    ("chip.risk", "рисков {from} → {to}", "risks {from} → {to}"),
    ("chip.score", "оценка {from} → {to}", "score {from} → {to}"),
    ("chip.availability", "доступность {from} → {to}", "availability {from} → {to}"),
    ("chip.cost", "{v} {unit}", "{v} {unit}"),
    ("chip.introduced", "новых находок: {n}", "new findings: {n}"),
    ("md.severity", "Уровень", "Level"),
    ("md.rule", "Правило", "Rule"),
    ("md.what", "Что не так", "What is wrong"),
    ("md.where", "Где", "Where"),
    ("md.fix", "Исправление", "Fix"),
    ("md.step", "Шаг", "Step"),
    ("md.effect", "Эффект", "Effect"),
    ("md.findings", "Находки", "Findings"),
    ("cli.err.unknown_option", "Неизвестный параметр {name}. Справка: infra-architect check --help", "Unknown option {name}. See: infra-architect check --help"),
    ("cli.err.needs_value", "Параметру {name} нужно значение", "{name} needs a value"),
    ("cli.err.bad_value", "Параметр {name}: «{value}» не подходит (допустимо: {allowed})", "{name}: “{value}” is not valid (allowed: {allowed})"),
    ("cli.err.no_files", "Не указан файл со схемой. Справка: infra-architect check --help", "No architecture file given. See: infra-architect check --help"),
    ("cli.err.read", "Не удалось прочитать {file}: {error}", "Cannot read {file}: {error}"),
    ("cli.err.write", "Не удалось записать {file}: {error}", "Cannot write {file}: {error}"),
    ("cli.err.json", "{file}: это не JSON ({error})", "{file}: not valid JSON ({error})"),
    ("cli.err.architecture", "{file}: это не схема архитектуры ({error})", "{file}: not an architecture ({error})"),
    ("cli.err.invalid", "{file}: в схеме есть ошибки, проверять нечего:", "{file}: the design has errors and cannot be checked:"),
    ("cli.err.config", "Конфигурация {file}: {error}", "Configuration {file}: {error}"),
    ("cli.err.baseline", "База сравнения {file}: {error}", "Baseline {file}: {error}"),
    ("cli.err.waivers", "Принятые риски {file}: {error}", "Accepted risks {file}: {error}"),
    ("cli.err.today", "--today: «{value}» — не дата (ГГГГ-ММ-ДД)", "--today: “{value}” is not a date (YYYY-MM-DD)"),
    ("cli.err.report_spec", "--report ждёт формат=путь, например sarif=results.sarif", "--report expects format=path, for example sarif=results.sarif"),
    ("cli.err.min_score", "--min-score: «{value}» — не число от 0 до 100", "--min-score: “{value}” is not a number from 0 to 100"),
    ("sarif.rationale", "Почему это важно", "Why it matters"),
    ("sarif.remediation", "Что делать", "What to do"),
];

fn column(lang: &str) -> fn(&(&'static str, &'static str, &'static str)) -> &'static str {
    if lang == "ru" {
        |t| t.1
    } else {
        |t| t.2
    }
}

/// The text for `key` in `lang` with `{placeholders}` filled in; a key that does not exist is shown as itself.
pub fn say(lang: &str, key: &str, vars: &[(&str, Value)]) -> String {
    match TEXTS.iter().find(|t| t.0 == key) {
        Some(t) => super::fill(column(lang)(t), vars, lang),
        None => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_there_once_and_has_both_columns_with_the_same_placeholders() {
        let names = |s: &str| -> Vec<String> {
            let mut v: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split('}').next()).map(|p| p.split('|').next().unwrap_or("").to_string()).collect();
            v.sort();
            v.dedup();
            v
        };
        let mut seen = std::collections::BTreeSet::new();
        for (key, ru, en) in TEXTS {
            assert!(seen.insert(*key), "{key} is listed twice");
            assert!(!ru.is_empty() && !en.is_empty(), "{key} lacks a language");
            assert_eq!(names(ru), names(en), "{key}: the languages use different placeholders");
        }
    }

    #[test]
    fn plural_forms_follow_the_language_and_unknown_keys_stay_visible() {
        let n = |n: i64| [("n", serde_json::json!(n))];
        assert!(say("ru", "manual", &n(1)).contains("1 находка"));
        assert!(say("ru", "manual", &n(2)).contains("2 находки"));
        assert!(say("ru", "manual", &n(5)).contains("5 находок"));
        assert!(say("en", "manual", &n(1)).contains("1 finding the"));
        assert!(say("en", "manual", &n(3)).contains("3 findings"));
        assert!(say("de", "passed", &[]) == "PASSED", "a language without a column reads English");
        assert_eq!(say("ru", "no.such.key", &[]), "no.such.key");
    }
}
