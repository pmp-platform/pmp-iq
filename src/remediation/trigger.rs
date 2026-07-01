//! Pure remediation-trigger evaluation (M46): decide whether a finding applies
//! to an application given its assembled signals. No I/O.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The signals a trigger is evaluated against for one application.
#[derive(Debug, Clone, Default)]
pub struct AppSignals {
    pub metrics: HashMap<String, f64>,
    pub failed_checks: HashSet<String>,
    pub eol_count: i64,
}

fn metric(params: &Value, signals: &AppSignals) -> Option<f64> {
    let name = params.get("metric").and_then(Value::as_str)?;
    signals.metrics.get(name).copied()
}

fn threshold(params: &Value) -> f64 {
    params.get("threshold").and_then(Value::as_f64).unwrap_or(0.0)
}

/// Does a rule's trigger match an application's signals? A missing metric never
/// matches (a finding requires positive evidence).
pub fn trigger_matches(trigger: &str, params: &Value, signals: &AppSignals) -> bool {
    match trigger {
        "metric_below" => metric(params, signals).is_some_and(|v| v < threshold(params)),
        "metric_above" => metric(params, signals).is_some_and(|v| v > threshold(params)),
        "scorecard_failed" => params
            .get("check_id")
            .and_then(Value::as_str)
            .is_some_and(|c| signals.failed_checks.contains(c)),
        "dep_eol" => signals.eol_count > 0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn signals() -> AppSignals {
        AppSignals {
            metrics: HashMap::from([("coverage_pct".to_string(), 40.0), ("vuln_critical".to_string(), 2.0)]),
            failed_checks: HashSet::from(["has_owner".to_string()]),
            eol_count: 1,
        }
    }

    #[test]
    fn metric_triggers() {
        let s = signals();
        assert!(trigger_matches("metric_below", &json!({ "metric": "coverage_pct", "threshold": 70 }), &s));
        assert!(!trigger_matches("metric_below", &json!({ "metric": "coverage_pct", "threshold": 30 }), &s));
        assert!(trigger_matches("metric_above", &json!({ "metric": "vuln_critical", "threshold": 0 }), &s));
        // Missing metric never matches.
        assert!(!trigger_matches("metric_below", &json!({ "metric": "absent", "threshold": 99 }), &s));
    }

    #[test]
    fn scorecard_and_eol_triggers() {
        let s = signals();
        assert!(trigger_matches("scorecard_failed", &json!({ "check_id": "has_owner" }), &s));
        assert!(!trigger_matches("scorecard_failed", &json!({ "check_id": "coverage_min" }), &s));
        assert!(trigger_matches("dep_eol", &json!({}), &s));
        assert!(!trigger_matches("unknown", &json!({}), &s));
    }
}
