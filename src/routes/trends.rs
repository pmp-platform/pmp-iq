//! Metric trend & distribution routes (M35): time-series (platform-wide,
//! per-dimension, and per-application) plus a fleet distribution histogram —
//! the read side behind the dashboard charts and per-app sparklines.

use crate::app::AppState;
use crate::error::AppResult;
use crate::metrics::{allowed_dimension, daily_average, histogram};
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/platform/series", get(series))
        .route("/api/platform/distribution", get(distribution))
        .route("/api/platform/portfolio", get(portfolio))
        .route("/api/platform/applications/:id/series", get(app_series))
}

/// Per-application coverage/complexity/LOC for the scatter + treemap charts.
async fn portfolio(State(state): State<AppState>) -> AppResult<Json<Value>> {
    use crate::platform::ListQuery;
    let latest = state.metrics.latest_all().await?;
    let mut by_app: BTreeMap<Uuid, [Option<f64>; 3]> = BTreeMap::new();
    for m in &latest {
        let slot = by_app.entry(m.application_id).or_default();
        match m.metric_key.as_str() {
            "coverage_pct" => slot[0] = Some(m.value),
            "complexity_avg" => slot[1] = Some(m.value),
            "loc" => slot[2] = Some(m.value),
            _ => {}
        }
    }
    let page = state
        .platform
        .list("applications", &ListQuery::new(None, None, Some(500), Default::default()))
        .await?;
    let mut names: BTreeMap<Uuid, String> = BTreeMap::new();
    for item in &page.items {
        if let (Some(id), Some(name)) = (
            item.get("id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok()),
            item.get("name").and_then(Value::as_str),
        ) {
            names.insert(id, name.to_string());
        }
    }
    let apps: Vec<Value> = by_app
        .into_iter()
        .map(|(id, [coverage, complexity, loc])| {
            json!({
                "id": id.to_string(),
                "name": names.get(&id).cloned().unwrap_or_default(),
                "href": format!("/platform/applications/{id}"),
                "coverage_pct": coverage,
                "complexity_avg": complexity,
                "loc": loc,
            })
        })
        .collect();
    Ok(Json(json!({ "apps": apps })))
}

#[derive(Deserialize)]
struct SeriesQuery {
    metric: String,
    #[serde(default)]
    dimension: Option<String>,
    #[serde(default)]
    window: Option<i64>,
}

/// Clamp the window (days) to a sane range; default 90.
fn since(window: Option<i64>) -> chrono::DateTime<Utc> {
    Utc::now() - Duration::days(window.unwrap_or(90).clamp(1, 1825))
}

async fn series(State(state): State<AppState>, Query(q): Query<SeriesQuery>) -> AppResult<Json<Value>> {
    let from = since(q.window);
    match q.dimension {
        Some(dim) if allowed_dimension(&dim) => {
            let rows = state.metrics.history_by_dimension(&q.metric, &dim, from).await?;
            let mut grouped: BTreeMap<String, Vec<crate::metrics::Point>> = BTreeMap::new();
            for (key, point) in rows {
                grouped.entry(key).or_default().push(point);
            }
            let series: BTreeMap<String, _> =
                grouped.into_iter().map(|(k, points)| (k, daily_average(&points))).collect();
            Ok(Json(json!({ "metric": q.metric, "dimension": dim, "series": series })))
        }
        Some(dim) => Err(crate::error::AppError::BadRequest(format!("dimension '{dim}' is not allowed"))),
        None => {
            let points = state.metrics.history(&q.metric, from).await?;
            Ok(Json(json!({ "metric": q.metric, "series": daily_average(&points) })))
        }
    }
}

#[derive(Deserialize)]
struct DistQuery {
    metric: String,
    #[serde(default)]
    buckets: Option<usize>,
}

async fn distribution(State(state): State<AppState>, Query(q): Query<DistQuery>) -> AppResult<Json<Value>> {
    let latest = state.metrics.latest_all().await?;
    let values: Vec<f64> = latest.iter().filter(|m| m.metric_key == q.metric).map(|m| m.value).collect();
    let buckets = histogram(&values, q.buckets.unwrap_or(10).clamp(1, 50));
    Ok(Json(json!({ "metric": q.metric, "count": values.len(), "buckets": buckets })))
}

#[derive(Deserialize)]
struct AppSeriesQuery {
    metric: String,
    #[serde(default)]
    window: Option<i64>,
}

async fn app_series(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<AppSeriesQuery>,
) -> AppResult<Json<Value>> {
    let points = state.metrics.app_history(id, &q.metric, since(q.window)).await?;
    let series = daily_average(&points);
    // Latest value + delta vs the previous collection (for the sparkline label).
    let latest = series.last().map(|p| p.value);
    let delta = (series.len() >= 2)
        .then(|| series[series.len() - 1].value - series[series.len() - 2].value);
    Ok(Json(json!({ "metric": q.metric, "series": series, "latest": latest, "delta": delta })))
}
