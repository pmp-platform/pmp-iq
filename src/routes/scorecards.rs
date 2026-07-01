//! Production-readiness scorecard routes (M43): per-application and fleet.

use crate::app::AppState;
use crate::error::AppResult;
use crate::platform::ListQuery;
use crate::scorecards::{ScorecardInput, default_checks, engine::Check, evaluate};
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/platform/applications/:id/scorecard", get(app_scorecard))
        .route("/api/platform/scorecards", get(fleet_scorecards))
}

/// The active checks: stored definitions, or the code defaults when unseeded.
pub(crate) async fn active_checks(state: &AppState) -> AppResult<Vec<Check>> {
    let stored = state.scorecards.list_checks().await?;
    Ok(if stored.is_empty() { default_checks() } else { stored })
}

/// Latest metric values for an application as a `key → value` map.
async fn metrics_map(state: &AppState, app_id: Uuid) -> AppResult<HashMap<String, f64>> {
    let latest = state.metrics.latest_for_application(app_id).await?;
    Ok(latest.into_iter().map(|m| (m.metric_key, m.value)).collect())
}

/// Build the scorecard input for one application.
async fn input_for(state: &AppState, app_id: Uuid) -> AppResult<ScorecardInput> {
    Ok(ScorecardInput {
        app_detail: state.platform.detail("applications", app_id).await?,
        metrics: metrics_map(state, app_id).await?,
        owner_teams: state.rbac.owner_team_names(app_id).await.unwrap_or_default(),
    })
}

async fn app_scorecard(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let checks = active_checks(&state).await?;
    let card = evaluate(&input_for(&state, id).await?, &checks);
    // Record the latest results for history (best-effort).
    let _ = state.scorecards.record(id, &card.results).await;
    Ok(Json(json!(card)))
}

async fn fleet_scorecards(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let checks = active_checks(&state).await?;
    let page = state
        .platform
        .list("applications", &ListQuery::new(None, None, Some(500), Default::default()))
        .await?;
    let mut rows = Vec::new();
    for item in &page.items {
        let Some(id) = item.get("id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) else {
            continue;
        };
        let card = evaluate(&input_for(&state, id).await?, &checks);
        rows.push(json!({
            "id": id.to_string(),
            "name": item.get("name").cloned().unwrap_or(Value::Null),
            "href": format!("/platform/applications/{id}"),
            "score": card.score,
            "level": card.level,
        }));
    }
    rows.sort_by(|a, b| {
        b["score"].as_f64().unwrap_or(0.0).partial_cmp(&a["score"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(Json(json!({ "scorecards": rows })))
}
