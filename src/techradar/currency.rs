//! Pure version-currency calculation (M45): parse versions, measure how far
//! behind a dependency is, and bucket its end-of-life status. No I/O.

use chrono::NaiveDate;
use serde::Serialize;

/// A dependency to assess: its declared version plus the policy's known-latest
/// version and EOL date (both optional when no policy exists).
#[derive(Debug, Clone)]
pub struct DepInput {
    pub name: String,
    pub ecosystem: String,
    pub version: String,
    pub latest: Option<String>,
    pub eol: Option<NaiveDate>,
}

/// The assessed currency of one dependency.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DepCurrency {
    pub name: String,
    pub ecosystem: String,
    pub version: String,
    pub latest: Option<String>,
    pub major_behind: i64,
    pub eol_status: String, // current | eol_soon | eol | unknown
}

/// Parse a `major.minor.patch` version, tolerating `^`/`~`/`v` prefixes and
/// missing components (returns leading-numeric parts; `None` when unparseable).
pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let cleaned = raw.trim_start_matches(['^', '~', '=', 'v', 'V', ' ']);
    let mut parts = cleaned.split('.');
    let major = leading_number(parts.next()?)?;
    let minor = parts.next().and_then(leading_number).unwrap_or(0);
    let patch = parts.next().and_then(leading_number).unwrap_or(0);
    Some((major, minor, patch))
}

fn leading_number(s: &str) -> Option<u64> {
    let digits: String = s.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// How many major versions `version` is behind `latest` (0 when current or
/// ahead, or when either is unparseable).
pub fn major_behind(version: &str, latest: &str) -> i64 {
    match (parse_semver(version), parse_semver(latest)) {
        (Some((cur, _, _)), Some((lat, _, _))) if lat > cur => (lat - cur) as i64,
        _ => 0,
    }
}

/// EOL bucket from the policy date relative to `today`.
pub fn eol_status(eol: Option<NaiveDate>, today: NaiveDate, soon_days: i64) -> &'static str {
    match eol {
        None => "unknown",
        Some(date) if today > date => "eol",
        Some(date) if today + chrono::Duration::days(soon_days) > date => "eol_soon",
        Some(_) => "current",
    }
}

/// Assess one dependency against its policy.
pub fn assess(dep: &DepInput, today: NaiveDate, soon_days: i64) -> DepCurrency {
    let major = dep.latest.as_deref().map(|l| major_behind(&dep.version, l)).unwrap_or(0);
    DepCurrency {
        name: dep.name.clone(),
        ecosystem: dep.ecosystem.clone(),
        version: dep.version.clone(),
        latest: dep.latest.clone(),
        major_behind: major,
        eol_status: eol_status(dep.eol, today, soon_days).to_string(),
    }
}

/// A dependency is "current" when it is on the latest major and not end-of-life.
pub fn is_current(dep: &DepCurrency) -> bool {
    dep.major_behind == 0 && dep.eol_status != "eol"
}

/// Fraction of dependencies that are current (1.0 when there are none).
pub fn currency_score(deps: &[DepCurrency]) -> f64 {
    if deps.is_empty() {
        return 1.0;
    }
    deps.iter().filter(|d| is_current(d)).count() as f64 / deps.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn parses_versions_tolerantly() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("^2.0"), Some((2, 0, 0)));
        assert_eq!(parse_semver("v3"), Some((3, 0, 0)));
        assert_eq!(parse_semver("1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_semver("not-a-version"), None);
    }

    #[test]
    fn major_lag() {
        assert_eq!(major_behind("1.0.0", "3.2.1"), 2);
        assert_eq!(major_behind("3.0.0", "3.5.0"), 0); // same major
        assert_eq!(major_behind("4.0.0", "3.0.0"), 0); // ahead
        assert_eq!(major_behind("x", "1.0.0"), 0); // unparseable
    }

    #[test]
    fn eol_buckets() {
        let today = day(2026, 6, 1);
        assert_eq!(eol_status(None, today, 90), "unknown");
        assert_eq!(eol_status(Some(day(2026, 1, 1)), today, 90), "eol"); // past
        assert_eq!(eol_status(Some(day(2026, 7, 1)), today, 90), "eol_soon"); // within 90d
        assert_eq!(eol_status(Some(day(2027, 1, 1)), today, 90), "current"); // far off
    }

    #[test]
    fn assess_and_score() {
        let today = day(2026, 6, 1);
        let deps = vec![
            assess(&DepInput { name: "axum".into(), ecosystem: "cargo".into(), version: "0.7.0".into(), latest: Some("0.7.5".into()), eol: None }, today, 90),
            assess(&DepInput { name: "old".into(), ecosystem: "cargo".into(), version: "1.0.0".into(), latest: Some("3.0.0".into()), eol: Some(day(2025, 1, 1)) }, today, 90),
        ];
        // axum same-major + no eol → current; old is 2 behind + eol → not current.
        assert_eq!(deps[0].major_behind, 0);
        assert_eq!(deps[1].major_behind, 2);
        assert_eq!(deps[1].eol_status, "eol");
        assert!((currency_score(&deps) - 0.5).abs() < 1e-9);
        assert_eq!(currency_score(&[]), 1.0);
    }
}
