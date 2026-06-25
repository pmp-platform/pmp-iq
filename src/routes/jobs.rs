//! Jobs: configuration CRUD, manual runs, and execution views.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::AppResult;
use crate::jobs::{Job, JobInput, TriggerType};
use crate::web::{PageContext, render_page};
use axum::Extension;
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::response::Html;
use axum::routing::{get, post};
use minijinja::context;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

const EXECUTIONS_LIMIT: i64 = 100;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/jobs", get(jobs_page))
        .route("/jobs/executions/:id", get(execution_page))
        .route("/api/jobs/types", get(list_job_types))
        .route("/api/jobs", get(list_jobs).post(create_job))
        .route(
            "/api/jobs/:id",
            axum::routing::put(update_job).delete(delete_job),
        )
        .route("/api/jobs/:id/run", post(run_job))
        .route("/api/jobs/executions", get(list_executions))
        .route("/api/jobs/executions/:id", get(get_execution))
        .route("/api/jobs/executions/:id/pause", post(pause_execution))
        .route("/api/jobs/executions/:id/resume", post(resume_execution))
}

#[derive(Deserialize)]
struct JobPayload {
    job_type: String,
    name: String,
    trigger_type: TriggerType,
    #[serde(default)]
    cron_expr: Option<String>,
    #[serde(default = "default_config")]
    config: Value,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_config() -> Value {
    json!({})
}

fn default_true() -> bool {
    true
}

impl From<JobPayload> for JobInput {
    fn from(p: JobPayload) -> Self {
        JobInput {
            job_type: p.job_type,
            name: p.name,
            trigger_type: p.trigger_type,
            cron_expr: p.cron_expr,
            config: p.config,
            enabled: p.enabled,
        }
    }
}

#[derive(Serialize)]
struct JobView {
    id: Uuid,
    job_type: String,
    name: String,
    trigger_type: TriggerType,
    cron_expr: Option<String>,
    config: Value,
    enabled: bool,
}

impl From<&Job> for JobView {
    fn from(j: &Job) -> Self {
        JobView {
            id: j.id,
            job_type: j.job_type.clone(),
            name: j.name.clone(),
            trigger_type: j.trigger_type,
            cron_expr: j.cron_expr.clone(),
            config: j.config.clone(),
            enabled: j.enabled,
        }
    }
}

async fn jobs_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "jobs");
    render_page(&state.engine, "jobs.html", &page, context! {})
}

async fn execution_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "jobs");
    render_page(
        &state.engine,
        "job_detail.html",
        &page,
        context! { execution_id => id.to_string() },
    )
}

/// Available job types and their descriptions (for the create-job form).
async fn list_job_types(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "types": state.job_types.list() })))
}

async fn list_jobs(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let jobs = state.jobs_repo.list().await?;
    let views: Vec<JobView> = jobs.iter().map(JobView::from).collect();
    Ok(Json(json!({ "jobs": views })))
}

async fn create_job(
    State(state): State<AppState>,
    Json(payload): Json<JobPayload>,
) -> AppResult<Json<JobView>> {
    let job = state.jobs_repo.create(JobInput::from(payload)).await?;
    Ok(Json(JobView::from(&job)))
}

async fn update_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<JobPayload>,
) -> AppResult<Json<JobView>> {
    let job = state.jobs_repo.update(id, JobInput::from(payload)).await?;
    Ok(Json(JobView::from(&job)))
}

async fn delete_job(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    state.jobs_repo.delete(id).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn run_job(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let execution_id = state.runner.start(id, "manual").await?;
    Ok(Json(json!({ "execution_id": execution_id })))
}

async fn list_executions(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let executions = state.executions_repo.list(EXECUTIONS_LIMIT).await?;
    Ok(Json(json!({ "executions": executions })))
}

async fn get_execution(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let execution = state.executions_repo.get(id).await?;
    Ok(Json(json!({ "execution": execution })))
}

/// Request a cooperative pause of a running execution (the job pauses at its
/// next checkpoint).
async fn pause_execution(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    state.executions_repo.request_pause(id).await?;
    Ok(Json(json!({ "pause_requested": true })))
}

/// Resume a paused execution from its checkpoint.
async fn resume_execution(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    state.runner.resume(id).await?;
    Ok(Json(json!({ "resumed": true })))
}
