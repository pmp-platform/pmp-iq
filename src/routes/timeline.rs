//! Timeline, diff and audit routes (M36): the platform change feed (per app +
//! global), a two-point net diff, and the admin audit log.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::platform::changes::summarize;
use crate::web::{PageContext, render_page};
use axum::extract::{Path, Query, State};
use axum::response::Html;
use axum::routing::get;
use axum::{Extension, Json, Router};
use chrono::{DateTime, Utc};
use minijinja::context;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const TIMELINE_LIMIT: i64 = 200;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/platform/audit", get(audit_page))
        .route("/api/platform/timeline", get(global_timeline))
        .route("/api/platform/applications/:id/timeline", get(app_timeline))
        .route("/api/platform/diff", get(diff))
        .route("/api/audit", get(audit_list))
}

async fn global_timeline(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let rows = state.changes.timeline(None, TIMELINE_LIMIT).await?;
    Ok(Json(json!({ "changes": rows })))
}

async fn app_timeline(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let rows = state.changes.timeline(Some(id), TIMELINE_LIMIT).await?;
    Ok(Json(json!({ "changes": rows })))
}

/// `?from=&to=&application=` — net created/updated/removed per entity type
/// between two timestamps (RFC3339), optionally scoped to one application.
#[derive(Deserialize)]
struct DiffQuery {
    from: Option<String>,
    to: Option<String>,
    application: Option<Uuid>,
}

fn parse_time(raw: Option<&str>, default: DateTime<Utc>) -> Result<DateTime<Utc>, AppError> {
    match raw {
        None => Ok(default),
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| AppError::BadRequest(format!("invalid timestamp '{s}': {e}"))),
    }
}

async fn diff(State(state): State<AppState>, Query(q): Query<DiffQuery>) -> AppResult<Json<Value>> {
    let now = Utc::now();
    let from = parse_time(q.from.as_deref(), now - chrono::Duration::days(30))?;
    let to = parse_time(q.to.as_deref(), now)?;
    let rows = state.changes.between(from, to, q.application).await?;
    Ok(Json(json!({
        "from": from.to_rfc3339(),
        "to": to.to_rfc3339(),
        "summary": summarize(&rows),
        "changes": rows,
    })))
}

async fn audit_list(State(state): State<AppState>, Extension(user): Extension<Principal>) -> AppResult<Json<Value>> {
    if !user.has_role("admin") {
        return Err(AppError::Unauthorized);
    }
    let events = state.audit.list(TIMELINE_LIMIT).await?;
    Ok(Json(json!({ "events": events })))
}

async fn audit_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Html<String>> {
    if !user.has_role("admin") {
        return Err(AppError::Unauthorized);
    }
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(&state.engine, "audit.html", &page, context! { active_tab => "audit" })
}
