//! Product lifecycle: release tables (data of `catalog.versions`, synced from endoflife.date or edited by hand) and
//! what they say about the version a node runs. The engine never reads a clock: the date is an argument.

use serde::{Deserialize, Serialize};

/// A date (`YYYY-MM-DD`) or a flag, the way endoflife.date reports end of support: a date, `true` (over, no date known)
/// or `false` (not over / unknown).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DateOrFlag {
    Date(String),
    Flag(bool),
}

impl DateOrFlag {
    /// Has the date passed `today`? `None` = no date and no flag: unknown.
    pub fn reached(&self, today: i64) -> Option<bool> {
        match self {
            DateOrFlag::Date(d) => parse_date(d).map(|d| d <= today),
            DateOrFlag::Flag(b) => Some(*b),
        }
    }
    pub fn date(&self) -> Option<&str> {
        match self {
            DateOrFlag::Date(d) => Some(d),
            DateOrFlag::Flag(_) => None,
        }
    }
}

/// One release line of a product (`16`, `1.31`, `8.0`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Cycle {
    pub cycle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    /// End of active (feature and bug-fix) support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support: Option<DateOrFlag>,
    /// End of security fixes: after it the line is not maintained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eol: Option<DateOrFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_release_date: Option<String>,
    #[serde(default)]
    pub lts: bool,
}

/// The release table of one product (`catalog.versions`, id = product id, e.g. `postgresql`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VersionSet {
    pub id: String,
    /// How the product is called (`Ubuntu`, `Node.js`); the id when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Where the data comes from (`https://endoflife.date/api/postgresql.json`, "manual").
    #[serde(default)]
    pub source: String,
    /// When it was taken, `YYYY-MM-DD`.
    #[serde(default)]
    pub as_of: String,
    #[serde(default)]
    pub cycles: Vec<Cycle>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Active support.
    Supported,
    /// Active support is over, security fixes continue.
    Maintenance,
    /// No security fixes any more.
    Eol,
    /// The table does not know the dates.
    Unknown,
}

/// What the table says about one version at one date.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lifecycle {
    pub cycle: String,
    pub phase: Phase,
    pub eol: Option<String>,
    /// Days until the end of security fixes (negative: already over); `None` without a date.
    pub days_to_eol: Option<i64>,
    pub latest: Option<String>,
    pub lts: bool,
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date (proleptic Gregorian); `None` if it does not parse.
pub fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.trim().splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.get(..2).unwrap_or("").parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days_from_civil (Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// `YYYY-MM-DD` of a day count since 1970-01-01 (the inverse of [`parse_date`]).
pub fn format_date(days: i64) -> String {
    // civil_from_days (Howard Hinnant)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// What endoflife.date reports for an end date: a date, a flag, or nothing.
fn date_or_flag(v: Option<&serde_json::Value>) -> Option<DateOrFlag> {
    match v? {
        serde_json::Value::Bool(b) => Some(DateOrFlag::Flag(*b)),
        serde_json::Value::String(s) if s.len() >= 10 && parse_date(s).is_some() => Some(DateOrFlag::Date(s[..10].to_string())),
        _ => None,
    }
}

/// The release lines of a product from the answer of the endoflife.date API (`/api/<product>.json`): the ones that are
/// still maintained or ended less than `max_age_years` ago, newest first. Anything that is not a list of objects with
/// a `cycle` yields nothing – the caller keeps the old table instead of emptying it.
pub fn from_endoflife(rows: &serde_json::Value, today: i64, max_age_years: f64) -> Vec<Cycle> {
    let Some(rows) = rows.as_array() else { return vec![] };
    let cutoff = today - (365.25 * max_age_years) as i64;
    let mut out: Vec<Cycle> = vec![];
    for r in rows {
        let Some(cycle) = r.get("cycle").and_then(|c| c.as_str().map(str::to_string).or_else(|| c.as_f64().map(|n| n.to_string()))).map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) else {
            continue;
        };
        let eol = date_or_flag(r.get("eol"));
        let release = r.get("releaseDate").and_then(|v| v.as_str()).filter(|d| d.len() >= 10 && parse_date(d).is_some()).map(|d| d[..10].to_string());
        let over = eol.as_ref().and_then(|e| e.reached(today)).unwrap_or(false);
        if over && release.as_deref().and_then(parse_date).map(|d| d < cutoff).unwrap_or(false) {
            continue;
        }
        out.push(Cycle {
            cycle,
            release_date: release,
            support: date_or_flag(r.get("support")),
            eol,
            latest: r.get("latest").and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(str::to_string),
            latest_release_date: r.get("latestReleaseDate").and_then(|v| v.as_str()).filter(|d| d.len() >= 10 && parse_date(d).is_some()).map(|d| d[..10].to_string()),
            lts: r.get("lts").map(|v| !matches!(v, serde_json::Value::Bool(false) | serde_json::Value::Null)).unwrap_or(false),
        });
    }
    out.sort_by(|a, b| b.release_date.cmp(&a.release_date).then(b.cycle.cmp(&a.cycle)));
    out
}

impl VersionSet {
    /// The release line a version string belongs to: the longest cycle that is a prefix of it on dot boundaries
    /// (`16.3` → `16`, `1.31.2` → `1.31`, `8.0.36` → `8.0`, `v1.31` → `1.31`).
    pub fn cycle_of(&self, version: &str) -> Option<&Cycle> {
        let v = version.trim().trim_start_matches(['v', 'V']);
        if v.is_empty() {
            return None;
        }
        self.cycles.iter().filter(|c| v == c.cycle || (v.starts_with(&c.cycle) && v[c.cycle.len()..].starts_with(['.', '-', '_', ' ']))).max_by_key(|c| c.cycle.len())
    }

    pub fn lifecycle(&self, version: &str, today: i64) -> Option<Lifecycle> {
        let c = self.cycle_of(version)?;
        let eol = c.eol.as_ref().and_then(|e| e.reached(today));
        let support = c.support.as_ref().and_then(|e| e.reached(today));
        let phase = match (eol, support) {
            (Some(true), _) => Phase::Eol,
            (Some(false), Some(true)) => Phase::Maintenance,
            (Some(false), _) => Phase::Supported,
            (None, Some(true)) => Phase::Maintenance,
            (None, Some(false)) => Phase::Supported,
            (None, None) => Phase::Unknown,
        };
        let days = c.eol.as_ref().and_then(|e| e.date()).and_then(parse_date).map(|d| d - today);
        Some(Lifecycle { cycle: c.cycle.clone(), phase, eol: c.eol.as_ref().and_then(|e| e.date()).map(String::from), days_to_eol: days, latest: c.latest.clone(), lts: c.lts })
    }

    /// How many days old the table is at `today` (`None` when it does not say when it was taken).
    pub fn age_days(&self, today: i64) -> Option<i64> {
        parse_date(&self.as_of).map(|d| today - d)
    }

    /// The release line to move to: the newest one that still gets security fixes and has been out for `mature_days`
    /// (a brand-new `x.0` is not a recommendation), the long-term-support ones first when `prefer_lts`. When nothing is
    /// that old yet, the newest maintained line. Cycles are listed newest first by the sync tool, but the order is not
    /// relied upon.
    pub fn recommended(&self, today: i64, prefer_lts: bool, mature_days: i64) -> Option<&Cycle> {
        let alive = |c: &&Cycle| c.eol.as_ref().and_then(|e| e.reached(today)) != Some(true);
        let mut cands: Vec<&Cycle> = self.cycles.iter().filter(alive).collect();
        cands.sort_by(|a, b| b.release_date.cmp(&a.release_date).then(b.cycle.cmp(&a.cycle)));
        let mature = |c: &&Cycle| c.release_date.as_deref().and_then(parse_date).map(|d| today - d >= mature_days).unwrap_or(true);
        let pool: Vec<&Cycle> = if cands.iter().any(mature) { cands.iter().copied().filter(mature).collect() } else { cands };
        if prefer_lts {
            if let Some(c) = pool.iter().find(|c| c.lts) {
                return Some(c);
            }
        }
        pool.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cycle(c: &str, release: &str, eol: Option<DateOrFlag>, lts: bool) -> Cycle {
        Cycle { cycle: c.into(), release_date: Some(release.into()), eol, latest: Some(format!("{c}.9")), lts, ..Default::default() }
    }

    #[test]
    fn dates_are_days_since_the_epoch() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("2000-03-01"), Some(11017));
        assert_eq!(parse_date("2026-10-13").unwrap() - parse_date("2026-10-12").unwrap(), 1);
        assert_eq!(parse_date("2024-03-01").unwrap() - parse_date("2024-02-28").unwrap(), 2); // leap year
        assert_eq!(parse_date("nonsense"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("2026-10-13T00:00:00Z"), parse_date("2026-10-13"));
    }

    #[test]
    fn versions_belong_to_the_longest_matching_cycle() {
        let set =
            VersionSet { id: "x".into(), cycles: vec![cycle("1", "2015-01-01", None, false), cycle("1.31", "2024-01-01", None, false), cycle("16", "2023-01-01", None, false)], ..Default::default() };
        assert_eq!(set.cycle_of("1.31.2").unwrap().cycle, "1.31");
        assert_eq!(set.cycle_of("v1.31").unwrap().cycle, "1.31");
        assert_eq!(set.cycle_of("1.4").unwrap().cycle, "1");
        assert_eq!(set.cycle_of("16.3").unwrap().cycle, "16");
        assert!(set.cycle_of("160").is_none(), "16 is not a prefix of 160 on a dot boundary");
        assert!(set.cycle_of("").is_none());
    }

    #[test]
    fn phases_follow_the_dates_and_flags() {
        let today = parse_date("2026-10-13").unwrap();
        let set = VersionSet {
            id: "x".into(),
            cycles: vec![
                Cycle { cycle: "18".into(), support: Some(DateOrFlag::Date("2028-01-01".into())), eol: Some(DateOrFlag::Date("2030-11-14".into())), ..Default::default() },
                Cycle { cycle: "14".into(), support: Some(DateOrFlag::Date("2025-01-01".into())), eol: Some(DateOrFlag::Date("2026-11-12".into())), ..Default::default() },
                Cycle { cycle: "12".into(), eol: Some(DateOrFlag::Date("2024-11-14".into())), ..Default::default() },
                Cycle { cycle: "9".into(), eol: Some(DateOrFlag::Flag(true)), ..Default::default() },
                Cycle { cycle: "99".into(), eol: Some(DateOrFlag::Flag(false)), ..Default::default() },
                Cycle { cycle: "x".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let phase = |v: &str| set.lifecycle(v, today).unwrap().phase;
        assert_eq!(phase("18.1"), Phase::Supported);
        assert_eq!(phase("14"), Phase::Maintenance);
        assert_eq!(set.lifecycle("14", today).unwrap().days_to_eol, Some(30));
        assert_eq!(phase("12.3"), Phase::Eol);
        assert!(set.lifecycle("12", today).unwrap().days_to_eol.unwrap() < 0);
        assert_eq!(phase("9"), Phase::Eol);
        assert_eq!(phase("99"), Phase::Supported);
        assert_eq!(phase("x"), Phase::Unknown);
        assert!(set.lifecycle("7", today).is_none());
    }

    #[test]
    fn dates_roundtrip_through_the_day_count() {
        for d in ["1970-01-01", "1999-12-31", "2000-02-29", "2024-03-01", "2026-10-13", "2100-03-01"] {
            assert_eq!(format_date(parse_date(d).unwrap()), d);
        }
        assert_eq!(format_date(-1), "1969-12-31");
    }

    #[test]
    fn the_endoflife_answer_becomes_cycles_newest_first() {
        let today = parse_date("2026-10-13").unwrap();
        let rows = serde_json::json!([
            {"cycle": "12", "releaseDate": "2018-10-18", "eol": "2024-11-14", "latest": "12.22", "lts": false},
            {"cycle": "16", "releaseDate": "2023-09-14", "support": "2026-11-12", "eol": "2028-11-09", "latest": "16.11", "latestReleaseDate": "2026-08-13"},
            {"cycle": "18", "releaseDate": "2025-09-25", "support": false, "eol": false, "lts": "Yes"},
            {"cycle": "9.6", "releaseDate": "2016-09-29", "eol": true},
            {"cycle": 17.0, "releaseDate": "2024-09-26", "eol": "2029-11-08"},
            {"releaseDate": "2020-01-01"},
            "garbage"
        ]);
        let c = from_endoflife(&rows, today, 8.0);
        let ids: Vec<&str> = c.iter().map(|c| c.cycle.as_str()).collect();
        // 9.6 ended long ago and came out before the eight-year window: dropped; 12 ended in 2024: kept
        assert_eq!(ids, ["18", "17", "16", "12"], "{ids:?}");
        assert_eq!(c[0].eol, Some(DateOrFlag::Flag(false)));
        assert_eq!(c[0].support, Some(DateOrFlag::Flag(false)));
        assert!(c[0].lts, "any truthy `lts` marks a long-term-support line");
        assert_eq!(c[2].support, Some(DateOrFlag::Date("2026-11-12".into())));
        assert_eq!(c[2].latest_release_date.as_deref(), Some("2026-08-13"));
        assert!(from_endoflife(&serde_json::json!({"error": "not found"}), today, 8.0).is_empty());
        // a tighter window drops the old line
        assert!(!from_endoflife(&rows, today, 1.0).iter().any(|c| c.cycle == "12"));
    }

    #[test]
    fn the_age_of_a_table_is_measured_in_days() {
        let set = VersionSet { id: "x".into(), as_of: "2026-10-01".into(), ..Default::default() };
        assert_eq!(set.age_days(parse_date("2026-10-13").unwrap()), Some(12));
        assert_eq!(VersionSet::default().age_days(0), None);
    }

    #[test]
    fn the_recommended_line_is_the_newest_that_is_still_maintained() {
        let today = parse_date("2026-10-13").unwrap();
        let set = VersionSet {
            id: "x".into(),
            cycles: vec![
                cycle("19", "2026-09-01", None, false),
                cycle("18", "2025-09-01", Some(DateOrFlag::Date("2030-01-01".into())), true),
                cycle("12", "2019-01-01", Some(DateOrFlag::Date("2024-01-01".into())), true),
            ],
            ..Default::default()
        };
        assert_eq!(set.recommended(today, false, 0).unwrap().cycle, "19");
        assert_eq!(set.recommended(today, true, 0).unwrap().cycle, "18");
        // 19 came out six weeks ago: with a 90-day maturity it is not a recommendation yet
        assert_eq!(set.recommended(today, false, 90).unwrap().cycle, "18");
        // nothing is mature: the newest maintained line is still better than no answer
        assert_eq!(set.recommended(today, false, 4000).unwrap().cycle, "19");
    }
}
