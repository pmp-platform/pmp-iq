//! Pure production-readiness scoring (M43): evaluate configurable checks against
//! an application's model + latest metrics + ownership and compute a weighted
//! score and maturity level. No I/O — unit-tested on fixed inputs.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

/// A configurable check: a built-in `rule` evaluator + its params, weight and
/// severity. `id` is the stable key surfaced in results.
#[derive(Debug, Clone)]
pub struct Check {
    pub id: String,
    pub description: String,
    pub rule: String,
    pub params: Value,
    pub weight: i32,
    pub severity: String, // info | warn | critical
    pub enabled: bool,
}

/// The signals a scorecard is computed from.
pub struct ScorecardInput {
    pub app_detail: Value,
    pub metrics: HashMap<String, f64>,
    pub owner_teams: Vec<String>,
}

/// One evaluated check.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CheckResult {
    pub check_id: String,
    pub passed: bool,
    pub severity: String,
    pub detail: Value,
}

/// The computed scorecard.
#[derive(Debug, Clone, Serialize)]
pub struct Scorecard {
    pub score: f64,
    pub level: String,
    pub results: Vec<CheckResult>,
}

fn metric(input: &ScorecardInput, params: &Value, key: &str) -> Option<f64> {
    let name = params.get(key).and_then(Value::as_str)?;
    input.metrics.get(name).copied()
}

fn threshold(params: &Value) -> f64 {
    params.get("threshold").and_then(Value::as_f64).unwrap_or(0.0)
}

/// Any component carries a non-empty array under `field`.
fn any_component_has(detail: &Value, field: &str) -> bool {
    detail
        .get("components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|c| c.get(field).and_then(Value::as_array).is_some_and(|a| !a.is_empty()))
}

/// Run one rule, returning whether it passed and an observed-vs-expected detail.
/// A missing metric fails a `metric_min` (not demonstrated) but passes a
/// `metric_max` (absence of the bad signal).
fn run_rule(rule: &str, params: &Value, input: &ScorecardInput) -> (bool, Value) {
    match rule {
        "has_owner" => {
            let teams = &input.owner_teams;
            (!teams.is_empty(), json!({ "teams": teams }))
        }
        "metric_min" => {
            let t = threshold(params);
            match metric(input, params, "metric") {
                Some(v) => (v >= t, json!({ "value": v, "threshold": t })),
                None => (false, json!({ "missing": true, "threshold": t })),
            }
        }
        "metric_max" => {
            let t = threshold(params);
            match metric(input, params, "metric") {
                Some(v) => (v <= t, json!({ "value": v, "threshold": t })),
                None => (true, json!({ "missing": true, "threshold": t })),
            }
        }
        "has_observability" => (any_component_has(&input.app_detail, "observability_signals"), json!({})),
        "has_diagrams" => {
            let has = input
                .app_detail
                .get("use_cases")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|u| u.get("diagrams").and_then(Value::as_array).is_some_and(|a| !a.is_empty()));
            (has, json!({}))
        }
        "documented" => {
            let described = input
                .app_detail
                .get("description")
                .and_then(Value::as_str)
                .is_some_and(|d| !d.trim().is_empty());
            (described, json!({ "described": described }))
        }
        // Unknown rule never blocks readiness (fails open as info).
        _ => (true, json!({ "unknown_rule": rule })),
    }
}

/// Evaluate the enabled checks and compute the weighted score + level.
pub fn evaluate(input: &ScorecardInput, checks: &[Check]) -> Scorecard {
    let mut results = Vec::new();
    let mut total_weight = 0i32;
    let mut earned = 0i32;
    let mut critical_failed = false;
    for check in checks.iter().filter(|c| c.enabled) {
        let (passed, detail) = run_rule(&check.rule, &check.params, input);
        let weight = check.weight.max(0);
        total_weight += weight;
        if passed {
            earned += weight;
        } else if check.severity == "critical" {
            critical_failed = true;
        }
        results.push(CheckResult {
            check_id: check.id.clone(),
            passed,
            severity: check.severity.clone(),
            detail,
        });
    }
    let score = if total_weight > 0 { earned as f64 / total_weight as f64 } else { 1.0 };
    Scorecard { score, level: level_for(score, critical_failed).to_string(), results }
}

/// The shipped default checks (seeded into `scorecard_checks`; editable).
pub fn default_checks() -> Vec<Check> {
    let c = |id: &str, description: &str, rule: &str, params: Value, weight: i32, severity: &str| Check {
        id: id.into(),
        description: description.into(),
        rule: rule.into(),
        params,
        weight,
        severity: severity.into(),
        enabled: true,
    };
    vec![
        c("has_owner", "Has an owning team", "has_owner", json!({}), 1, "warn"),
        c("coverage_min", "Test coverage ≥ 70%", "metric_min", json!({ "metric": "coverage_pct", "threshold": 70 }), 2, "warn"),
        c("complexity_max", "Average complexity ≤ 15", "metric_max", json!({ "metric": "complexity_avg", "threshold": 15 }), 1, "warn"),
        c("has_ci", "Has CI configured", "metric_min", json!({ "metric": "has_ci", "threshold": 1 }), 1, "warn"),
        c("has_tests", "Has automated tests", "metric_min", json!({ "metric": "tests_total", "threshold": 1 }), 1, "warn"),
        c("no_critical_vulns", "No critical vulnerabilities", "metric_max", json!({ "metric": "vuln_critical", "threshold": 0 }), 2, "critical"),
        c("has_observability", "Emits observability signals", "has_observability", json!({}), 1, "warn"),
        c("documented", "Has a description", "documented", json!({}), 1, "info"),
    ]
}

/// Maturity level from the score, capped to `at_risk` if any critical check fails.
pub fn level_for(score: f64, critical_failed: bool) -> &'static str {
    if critical_failed {
        "at_risk"
    } else if score >= 0.85 {
        "gold"
    } else if score >= 0.5 {
        "silver"
    } else {
        "bronze"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(id: &str, rule: &str, params: Value, weight: i32, severity: &str) -> Check {
        Check { id: id.into(), description: id.into(), rule: rule.into(), params, weight, severity: severity.into(), enabled: true }
    }

    fn input() -> ScorecardInput {
        ScorecardInput {
            app_detail: json!({
                "description": "a documented service",
                "components": [{ "name": "Api", "observability_signals": [{ "name": "m" }] }],
                "use_cases": [{ "name": "Pay", "diagrams": [{ "name": "seq" }] }]
            }),
            metrics: HashMap::from([("coverage_pct".to_string(), 80.0), ("complexity_avg".to_string(), 6.0)]),
            owner_teams: vec!["platform".into()],
        }
    }

    #[test]
    fn rules_evaluate_against_signals() {
        let inp = input();
        assert!(run_rule("has_owner", &json!({}), &inp).0);
        assert!(run_rule("metric_min", &json!({ "metric": "coverage_pct", "threshold": 70 }), &inp).0);
        assert!(!run_rule("metric_min", &json!({ "metric": "coverage_pct", "threshold": 90 }), &inp).0);
        assert!(run_rule("metric_max", &json!({ "metric": "complexity_avg", "threshold": 15 }), &inp).0);
        assert!(run_rule("has_observability", &json!({}), &inp).0);
        assert!(run_rule("has_diagrams", &json!({}), &inp).0);
        assert!(run_rule("documented", &json!({}), &inp).0);
    }

    #[test]
    fn missing_metric_fails_min_passes_max() {
        let inp = input();
        assert!(!run_rule("metric_min", &json!({ "metric": "absent", "threshold": 1 }), &inp).0);
        assert!(run_rule("metric_max", &json!({ "metric": "absent", "threshold": 0 }), &inp).0);
    }

    #[test]
    fn weighted_score_and_level() {
        let inp = input();
        let checks = vec![
            check("owner", "has_owner", json!({}), 1, "warn"),
            check("cov", "metric_min", json!({ "metric": "coverage_pct", "threshold": 70 }), 2, "warn"),
            check("hard", "metric_min", json!({ "metric": "coverage_pct", "threshold": 99 }), 1, "warn"),
        ];
        let card = evaluate(&inp, &checks);
        // passed weight 1+2=3 of 4 → 0.75 → silver.
        assert!((card.score - 0.75).abs() < 1e-9);
        assert_eq!(card.level, "silver");
        assert_eq!(card.results.len(), 3);
    }

    #[test]
    fn failed_critical_caps_at_risk() {
        let inp = input();
        let checks = vec![
            check("owner", "has_owner", json!({}), 1, "warn"),
            check("vuln", "metric_min", json!({ "metric": "scanned", "threshold": 1 }), 1, "critical"),
        ];
        let card = evaluate(&inp, &checks);
        assert_eq!(card.level, "at_risk"); // critical failed regardless of score
    }

    #[test]
    fn level_bands() {
        assert_eq!(level_for(0.9, false), "gold");
        assert_eq!(level_for(0.6, false), "silver");
        assert_eq!(level_for(0.2, false), "bronze");
        assert_eq!(level_for(1.0, true), "at_risk");
    }

    #[test]
    fn disabled_checks_excluded_and_empty_is_perfect() {
        let inp = input();
        let mut c = check("x", "has_owner", json!({}), 1, "warn");
        c.enabled = false;
        let card = evaluate(&inp, &[c]);
        assert_eq!(card.score, 1.0);
        assert!(card.results.is_empty());
    }
}
