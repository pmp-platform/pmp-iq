//! DORA metrics routes (M47): generic deployment/incident event ingestion and
//! the derived per-application / fleet DORA reports (recorded as metrics so they
//! trend via M35 and roll up by team via M37).

use crate::app::AppState;
use crate::dora::model::NewDeployment;
use crate::dora::{DoraSummary, compute};
use crate::error::{AppError, AppResult};
use crate::metrics::Metric;
use crate::platform::ListQuery;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const DEFAULT_WINDOW_DAYS: i64 = 90;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/events/deploy", post(record_deploy))
        .route("/api/events/incident", post(open_incident))
        .route("/api/events/incident/:id/resolve", post(resolve_incident))
        .route("/api/platform/applications/:id/dora", get(app_dora))
        .route("/api/platform/dora", get(fleet_dora))
}

#[derive(Deserialize)]
struct WindowQuery {
    days: Option<i64>,
}

fn window(q: &WindowQuery) -> (DateTime<Utc>, i64) {
    let days = q.days.filter(|d| *d > 0).unwrap_or(DEFAULT_WINDOW_DAYS);
    (Utc::now() - Duration::days(days), days)
}

#[derive(Deserialize)]
struct DeployBody {
    application_id: Option<Uuid>,
    repository_full_name: Option<String>,
    environment: Option<String>,
    sha: Option<String>,
    succeeded: Option<bool>,
    first_commit_at: Option<DateTime<Utc>>,
}

/// Resolve a deploy event's application id from an explicit id or a repository name.
async fn resolve_application(state: &AppState, body: &DeployBody) -> AppResult<Uuid> {
    if let Some(id) = body.application_id {
        return Ok(id);
    }
    let Some(full_name) = body.repository_full_name.as_deref() else {
        return Err(AppError::BadRequest("application_id or repository_full_name is required".into()));
    };
    let repos = state.repo_records.list().await?;
    let repo = repos
        .into_iter()
        .find(|r| r.full_name == full_name)
        .ok_or_else(|| AppError::NotFound("repository".into()))?;
    state
        .platform
        .repository_application(repo.id)
        .await?
        .ok_or_else(|| AppError::NotFound("application for repository".into()))
}

async fn record_deploy(State(state): State<AppState>, Json(body): Json<DeployBody>) -> AppResult<Json<Value>> {
    let application_id = resolve_application(&state, &body).await?;
    let deployment = state
        .dora
        .record_deployment(NewDeployment {
            application_id,
            environment: body.environment.unwrap_or_else(|| "production".to_string()),
            sha: body.sha,
            succeeded: body.succeeded.unwrap_or(true),
            first_commit_at: body.first_commit_at,
        })
        .await?;
    Ok(Json(json!({ "id": deployment.id.to_string() })))
}

#[derive(Deserialize)]
struct IncidentBody {
    application_id: Uuid,
    caused_by: Option<Uuid>,
}

async fn open_incident(State(state): State<AppState>, Json(body): Json<IncidentBody>) -> AppResult<Json<Value>> {
    let incident = state.dora.open_incident(body.application_id, body.caused_by).await?;
    Ok(Json(json!({ "id": incident.id.to_string() })))
}

async fn resolve_incident(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    state.dora.resolve_incident(id).await?;
    Ok(Json(json!({ "resolved": true })))
}

/// Record a per-application DORA summary as metrics (best-effort) so it trends.
async fn record_metrics(state: &AppState, app_id: Uuid, s: &DoraSummary) {
    let mut metrics = vec![
        Metric::new("dora_deploy_freq_weekly", s.deploy_frequency_weekly, Some("per_week")),
        Metric::new("dora_change_failure_rate", s.change_failure_rate, Some("ratio")),
    ];
    if let Some(h) = s.lead_time_hours {
        metrics.push(Metric::new("dora_lead_time_hours", h, Some("hours")));
    }
    if let Some(h) = s.mttr_hours {
        metrics.push(Metric::new("dora_mttr_hours", h, Some("hours")));
    }
    let _ = state.metrics.record(app_id, "dora", &metrics).await;
}

async fn app_dora(State(state): State<AppState>, Path(id): Path<Uuid>, Query(q): Query<WindowQuery>) -> AppResult<Json<Value>> {
    let (since, days) = window(&q);
    let deployments = state.dora.deployments_for(id, since).await?;
    let incidents = state.dora.incidents_for(id, since).await?;
    let summary = compute(&deployments, &incidents, days);
    record_metrics(&state, id, &summary).await;
    Ok(Json(json!({ "summary": summary, "window_days": days })))
}

async fn fleet_dora(State(state): State<AppState>, Query(q): Query<WindowQuery>) -> AppResult<Json<Value>> {
    let (since, days) = window(&q);
    let all_deploys = state.dora.all_deployments(since).await?;
    let all_incidents = state.dora.all_incidents(since).await?;
    let fleet = compute(&all_deploys, &all_incidents, days);

    let page = state
        .platform
        .list("applications", &ListQuery::new(None, None, Some(500), Default::default()))
        .await?;
    let mut rows = Vec::new();
    for item in &page.items {
        let Some(id) = item.get("id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) else {
            continue;
        };
        let deploys: Vec<_> = all_deploys.iter().filter(|d| d.application_id == Some(id)).cloned().collect();
        let incidents: Vec<_> = all_incidents.iter().filter(|i| i.application_id == Some(id)).cloned().collect();
        if deploys.is_empty() && incidents.is_empty() {
            continue; // only surface apps with delivery events
        }
        let summary = compute(&deploys, &incidents, days);
        rows.push(json!({
            "id": id.to_string(),
            "name": item.get("name").cloned().unwrap_or(Value::Null),
            "href": format!("/platform/applications/{id}"),
            "summary": summary,
        }));
    }
    Ok(Json(json!({ "fleet": fleet, "applications": rows, "window_days": days })))
}
