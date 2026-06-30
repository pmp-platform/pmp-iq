//! Platform metrics & insights dashboard (M32): aggregates the M31 quality
//! metrics with the catalog into rollups, group-by breakdowns, and leaderboards.
//! The aggregation is a pure function over the application list + latest metrics
//! so it is unit-testable; the route fetches the inputs and calls it.

use crate::metrics::ApplicationMetric;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

/// One application's fields + its latest metrics, flattened for aggregation.
struct AppRow {
    name: String,
    app_type: String,
    language: String,
    metrics: HashMap<String, f64>,
}

fn str_field<'a>(app: &'a Value, key: &str, default: &'a str) -> &'a str {
    app.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(default)
}

/// Build the dashboard JSON from the applications list + latest metrics.
pub fn build(apps: &[Value], metrics: &[ApplicationMetric]) -> Value {
    let mut by_app: HashMap<String, HashMap<String, f64>> = HashMap::new();
    for m in metrics {
        by_app
            .entry(m.application_id.to_string())
            .or_default()
            .insert(m.metric_key.clone(), m.value);
    }
    let rows: Vec<AppRow> = apps
        .iter()
        .map(|a| AppRow {
            name: str_field(a, "name", "unknown").to_string(),
            app_type: str_field(a, "app_type", "unknown").to_string(),
            language: str_field(a, "primary_language", "unknown").to_string(),
            metrics: a
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| by_app.get(id).cloned())
                .unwrap_or_default(),
        })
        .collect();

    json!({
        "rollup": rollup(&rows),
        "leaderboards": {
            "top_coverage": leaderboard(&rows, "coverage_pct", false, 5),
            "needs_coverage": leaderboard(&rows, "coverage_pct", true, 5),
            "lowest_complexity": leaderboard(&rows, "complexity_avg", true, 5),
            "highest_complexity": leaderboard(&rows, "complexity_avg", false, 5),
        },
        "groups": {
            "coverage_by_type": group_avg(&rows, |r| &r.app_type, "coverage_pct"),
            "coverage_by_language": group_avg(&rows, |r| &r.language, "coverage_pct"),
        },
    })
}

fn avg(rows: &[AppRow], key: &str) -> Option<f64> {
    let vals: Vec<f64> = rows.iter().filter_map(|r| r.metrics.get(key).copied()).collect();
    (!vals.is_empty()).then(|| vals.iter().sum::<f64>() / vals.len() as f64)
}

fn rollup(rows: &[AppRow]) -> Value {
    let total = rows.len();
    let with_ci = rows
        .iter()
        .filter(|r| r.metrics.get("has_ci").copied().unwrap_or(0.0) >= 1.0)
        .count();
    let with_metrics = rows.iter().filter(|r| !r.metrics.is_empty()).count();
    json!({
        "applications": total,
        "with_metrics": with_metrics,
        "with_ci": with_ci,
        "avg_coverage": avg(rows, "coverage_pct"),
        "avg_complexity": avg(rows, "complexity_avg"),
    })
}

fn leaderboard(rows: &[AppRow], key: &str, ascending: bool, n: usize) -> Vec<Value> {
    let mut v: Vec<(String, f64)> = rows
        .iter()
        .filter_map(|r| r.metrics.get(key).map(|&val| (r.name.clone(), val)))
        .collect();
    v.sort_by(|a, b| {
        let ord = a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal);
        if ascending { ord } else { ord.reverse() }
    });
    v.into_iter().take(n).map(|(name, value)| json!({ "name": name, "value": value })).collect()
}

fn group_avg(rows: &[AppRow], field: impl Fn(&AppRow) -> &str, metric: &str) -> Vec<Value> {
    let mut groups: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for r in rows {
        if let Some(&v) = r.metrics.get(metric) {
            groups.entry(field(r).to_string()).or_default().push(v);
        }
    }
    groups
        .into_iter()
        .map(|(group, vals)| {
            let avg = vals.iter().sum::<f64>() / vals.len() as f64;
            json!({ "group": group, "avg": avg, "count": vals.len() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn metric(app: Uuid, key: &str, value: f64) -> ApplicationMetric {
        ApplicationMetric {
            id: Uuid::new_v4(),
            application_id: app,
            metric_key: key.into(),
            value,
            unit: None,
            source: "llm".into(),
            category: crate::metrics::category_for(key).as_str().into(),
            collected_at: Utc::now(),
        }
    }

    #[test]
    fn aggregates_rollup_leaderboard_and_groups() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let apps = vec![
            json!({ "id": a.to_string(), "name": "api", "app_type": "service", "primary_language": "Rust" }),
            json!({ "id": b.to_string(), "name": "web", "app_type": "frontend", "primary_language": "TypeScript" }),
        ];
        let metrics = vec![
            metric(a, "coverage_pct", 90.0),
            metric(a, "has_ci", 1.0),
            metric(b, "coverage_pct", 50.0),
        ];
        let d = build(&apps, &metrics);

        assert_eq!(d["rollup"]["applications"], 2);
        assert_eq!(d["rollup"]["with_ci"], 1);
        assert_eq!(d["rollup"]["avg_coverage"], 70.0);
        // Top coverage = api (90).
        assert_eq!(d["leaderboards"]["top_coverage"][0]["name"], "api");
        assert_eq!(d["leaderboards"]["top_coverage"][0]["value"], 90.0);
        // Needs coverage = web (50) first.
        assert_eq!(d["leaderboards"]["needs_coverage"][0]["name"], "web");
        // Grouped by language, two groups.
        assert_eq!(d["groups"]["coverage_by_language"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn empty_inputs_are_safe() {
        let d = build(&[], &[]);
        assert_eq!(d["rollup"]["applications"], 0);
        assert!(d["rollup"]["avg_coverage"].is_null());
        assert!(d["leaderboards"]["top_coverage"].as_array().unwrap().is_empty());
    }
}
