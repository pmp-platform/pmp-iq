//! Pure aggregation helpers for metric trends & charts (M35): daily time-series
//! bucketing and fleet distribution histograms. The SQL-facing reads live in the
//! repository; these turn raw points into chart-ready series deterministically.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

/// Dimensions a series may be grouped by (allowlisted to safe columns).
pub const ALLOWED_DIMENSIONS: &[&str] = &["app_type", "primary_language"];

/// Is `dimension` an allowlisted grouping column?
pub fn allowed_dimension(dimension: &str) -> bool {
    ALLOWED_DIMENSIONS.contains(&dimension)
}

/// One timestamped metric reading.
#[derive(Debug, Clone)]
pub struct Point {
    pub at: DateTime<Utc>,
    pub value: f64,
}

/// One day's average for a series.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesPoint {
    pub day: String,
    pub value: f64,
}

/// Average the points per UTC day, ascending by day (chart-ready trend line).
pub fn daily_average(points: &[Point]) -> Vec<SeriesPoint> {
    let mut acc: BTreeMap<String, (f64, u32)> = BTreeMap::new();
    for p in points {
        let day = p.at.format("%Y-%m-%d").to_string();
        let entry = acc.entry(day).or_insert((0.0, 0));
        entry.0 += p.value;
        entry.1 += 1;
    }
    acc.into_iter()
        .map(|(day, (sum, n))| SeriesPoint { day, value: if n > 0 { sum / n as f64 } else { 0.0 } })
        .collect()
}

/// One histogram bucket `[lo, hi)` (the last bucket is inclusive of `hi`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bucket {
    pub lo: f64,
    pub hi: f64,
    pub count: i64,
}

/// Equal-width histogram of `values` into `buckets` bins over `[min, max]`.
/// Empty input → empty; a single distinct value → one bucket holding all.
pub fn histogram(values: &[f64], buckets: usize) -> Vec<Bucket> {
    let buckets = buckets.max(1);
    if values.is_empty() {
        return vec![];
    }
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if (max - min).abs() < f64::EPSILON {
        return vec![Bucket { lo: min, hi: max, count: values.len() as i64 }];
    }
    let width = (max - min) / buckets as f64;
    let mut out: Vec<Bucket> = (0..buckets)
        .map(|i| Bucket { lo: min + width * i as f64, hi: min + width * (i + 1) as f64, count: 0 })
        .collect();
    for &v in values {
        let mut idx = ((v - min) / width).floor() as usize;
        if idx >= buckets {
            idx = buckets - 1; // the max value lands in the final bucket
        }
        out[idx].count += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn pt(y: i32, m: u32, d: u32, value: f64) -> Point {
        Point { at: Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap(), value }
    }

    #[test]
    fn daily_average_groups_and_orders() {
        let points = vec![
            pt(2026, 6, 2, 10.0),
            pt(2026, 6, 1, 20.0),
            pt(2026, 6, 1, 40.0), // same day → averaged with the 20
        ];
        let series = daily_average(&points);
        assert_eq!(series.len(), 2);
        assert_eq!(series[0], SeriesPoint { day: "2026-06-01".into(), value: 30.0 });
        assert_eq!(series[1], SeriesPoint { day: "2026-06-02".into(), value: 10.0 });
    }

    #[test]
    fn daily_average_empty_is_safe() {
        assert!(daily_average(&[]).is_empty());
    }

    #[test]
    fn histogram_buckets_values() {
        // 0..10 into 2 buckets → [0,5):4 (0,1,2,3,4), [5,10]:6 (5..10).
        let values: Vec<f64> = (0..=10).map(|n| n as f64).collect();
        let h = histogram(&values, 2);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].count + h[1].count, 11);
        assert_eq!(h[1].count, 6); // 5,6,7,8,9,10
    }

    #[test]
    fn histogram_edge_cases() {
        assert!(histogram(&[], 4).is_empty());
        let single = histogram(&[7.0, 7.0, 7.0], 4);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].count, 3);
    }

    #[test]
    fn dimension_allowlist() {
        assert!(allowed_dimension("app_type"));
        assert!(allowed_dimension("primary_language"));
        assert!(!allowed_dimension("evil; DROP"));
    }
}
