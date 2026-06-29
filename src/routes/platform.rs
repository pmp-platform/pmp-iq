//! Platform section: overview, entity tables, detail pages, and read API.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::files::{FileBrowser, FileError};
use crate::platform::{GraphScope, ListQuery, filter_fields, is_entity};
use crate::web::{PageContext, render_page};
use axum::Extension;
use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::{Html, Redirect};
use axum::routing::{get, post};
use minijinja::context;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/platform", get(overview_page))
        .route("/platform/graph", get(graph_page))
        .route("/api/platform/graph", get(graph_api))
        .route("/platform/:entity", get(list_page))
        .route("/platform/:entity/:id", get(detail_page))
        .route("/api/platform/applications/:id/sync", post(sync_application))
        .route("/api/platform/applications/:id/ask", get(ask_history).post(ask_application))
        .route("/api/platform/applications/:id/ask/:execution_id", get(ask_result))
        .route(
            "/api/platform/applications/:id/agent-tasks",
            get(list_agent_tasks).post(create_agent_task),
        )
        .route("/api/platform/applications/:id/agent-tasks/:task_id", get(get_agent_task))
        .route(
            "/api/platform/applications/:id/agent-tasks/:task_id/messages",
            post(post_agent_message),
        )
        .route(
            "/api/platform/applications/:id/hints",
            get(list_hints).put(put_hint).delete(delete_hint),
        )
        .route("/api/platform/applications/:id/files", get(browse_files))
        .route("/api/platform/applications/:id/files/content", get(file_content))
        .route("/api/platform/:entity", get(list_api))
        .route("/api/platform/:entity/facets", get(facets_api))
        .route("/api/platform/:entity/:id", get(detail_api))
}

/// A question to ask the LLM about an application.
#[derive(Deserialize)]
struct AskPayload {
    question: String,
}

/// Schedule a `sync-repositories` run scoped to this application's repository.
async fn sync_application(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let repository = state
        .platform
        .application_repository(app_id)
        .await?
        .ok_or_else(|| AppError::BadRequest("application has no linked repository".into()))?;
    let profile = default_profile(&state).await.ok();
    let job_id = crate::review::ensure_sync_job(state.jobs_repo.as_ref(), profile).await?;
    let params = json!({ "repository_id": repository });
    let execution_id = state.runner.start_with_params(job_id, "manual", params).await?;
    Ok(Json(json!({ "execution_id": execution_id })))
}

/// Pick the AI profile to answer with: the first enabled, else the first
/// configured. Errors when none exist.
async fn default_profile(state: &AppState) -> AppResult<Uuid> {
    let profiles = state.ai.list().await?;
    profiles
        .iter()
        .find(|p| p.enabled)
        .or_else(|| profiles.first())
        .map(|p| p.id)
        .ok_or_else(|| AppError::BadRequest("no AI agent profile configured".into()))
}

/// Queue an `llm-repository-request` execution for an application's repository
/// with the user's question; returns the execution id to poll.
async fn ask_application(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    Json(payload): Json<AskPayload>,
) -> AppResult<Json<Value>> {
    let question = payload.question.trim().to_string();
    if question.is_empty() {
        return Err(AppError::BadRequest("question is required".into()));
    }
    let repository = state
        .platform
        .application_repository(app_id)
        .await?
        .ok_or_else(|| AppError::BadRequest("application has no linked repository".into()))?;
    let profile_id = default_profile(&state).await?;
    let job_id = crate::llm_request::ensure_job(state.jobs_repo.as_ref()).await?;
    let params = json!({ "repository": repository, "input": question, "ai_profile_id": profile_id });
    let execution_id = state.runner.start_with_params(job_id, "ask", params).await?;
    Ok(Json(json!({ "execution_id": execution_id })))
}

/// Body to create a new agent task.
#[derive(Deserialize)]
struct NewTaskPayload {
    title: String,
    message: String,
}

/// Body to add a follow-up message to a task.
#[derive(Deserialize)]
struct MessagePayload {
    message: String,
}

/// Enqueue an `application-agent-task` turn for a task with the given message.
async fn enqueue_turn(
    state: &AppState,
    task: &crate::agent_tasks::AgentTask,
    message: &str,
) -> AppResult<Uuid> {
    let profile_id = default_profile(state).await?;
    let job_id =
        crate::agent_tasks::ensure_job(state.jobs_repo.as_ref(), state.config.agent_max_concurrency)
            .await?;
    let params = json!({ "task_id": task.id, "message": message, "ai_profile_id": profile_id });
    state.runner.start_with_params(job_id, "agent", params).await
}

/// List the AI Agent tasks for an application (newest first).
async fn list_agent_tasks(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let tasks = state.agent_tasks.list_for_application(app_id).await?;
    Ok(Json(json!({ "tasks": tasks })))
}

/// Create a task (title + first instruction) and enqueue its first turn.
async fn create_agent_task(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    Json(payload): Json<NewTaskPayload>,
) -> AppResult<Json<Value>> {
    let title = payload.title.trim().to_string();
    let message = payload.message.trim().to_string();
    if title.is_empty() || message.is_empty() {
        return Err(AppError::BadRequest("title and message are required".into()));
    }
    let repository = state
        .platform
        .application_repository(app_id)
        .await?
        .ok_or_else(|| AppError::BadRequest("application has no linked repository".into()))?;
    let task = state
        .agent_tasks
        .create(crate::agent_tasks::NewAgentTask {
            application_id: app_id,
            repository_id: repository,
            title,
        })
        .await?;
    record_user_message(&state, task.id, &message).await?;
    let execution_id = enqueue_turn(&state, &task, &message).await?;
    Ok(Json(json!({ "task": task, "execution_id": execution_id })))
}

/// A task with its full message transcript.
async fn get_agent_task(
    State(state): State<AppState>,
    Path((_app_id, task_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let task = state.agent_tasks.get(task_id).await?;
    let messages = state.agent_tasks.messages(task_id).await?;
    Ok(Json(json!({ "task": task, "messages": messages })))
}

/// Append a follow-up instruction to a task and enqueue another turn.
async fn post_agent_message(
    State(state): State<AppState>,
    Path((_app_id, task_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<MessagePayload>,
) -> AppResult<Json<Value>> {
    let message = payload.message.trim().to_string();
    if message.is_empty() {
        return Err(AppError::BadRequest("message is required".into()));
    }
    let task = state.agent_tasks.get(task_id).await?;
    record_user_message(&state, task.id, &message).await?;
    let execution_id = enqueue_turn(&state, &task, &message).await?;
    Ok(Json(json!({ "execution_id": execution_id })))
}

/// Persist a user message on a task's transcript.
async fn record_user_message(state: &AppState, task_id: Uuid, message: &str) -> AppResult<()> {
    state
        .agent_tasks
        .add_message(crate::agent_tasks::NewMessage {
            task_id,
            role: "user".to_string(),
            content: message.to_string(),
            execution_id: None,
        })
        .await?;
    Ok(())
}

/// All hints configured for an application.
async fn list_hints(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let hints = state.hints.list_for_application(app_id).await?;
    Ok(Json(json!({ "hints": hints })))
}

/// Body for creating/updating a hint.
#[derive(Deserialize)]
struct HintPayload {
    entity_type: String,
    #[serde(default)]
    entity_key: String,
    hint: String,
}

/// Create or replace a hint for an application entity. An empty `hint` clears it.
async fn put_hint(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    Json(payload): Json<HintPayload>,
) -> AppResult<Json<Value>> {
    let entity_type = payload.entity_type.trim().to_string();
    if entity_type.is_empty() {
        return Err(AppError::BadRequest("entity_type is required".into()));
    }
    let entity_key = payload.entity_key.trim().to_string();
    if payload.hint.trim().is_empty() {
        state.hints.delete(app_id, &entity_type, &entity_key).await?;
        return Ok(Json(json!({ "deleted": true })));
    }
    let hint = state
        .hints
        .upsert(crate::hints::EntityHintInput {
            application_id: app_id,
            entity_type,
            entity_key,
            hint: payload.hint.trim().to_string(),
        })
        .await?;
    Ok(Json(json!({ "hint": hint })))
}

/// Query for addressing a specific hint to delete.
#[derive(Deserialize)]
struct HintTarget {
    entity_type: String,
    #[serde(default)]
    entity_key: String,
}

async fn delete_hint(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    Query(target): Query<HintTarget>,
) -> AppResult<Json<Value>> {
    state.hints.delete(app_id, &target.entity_type, &target.entity_key).await?;
    Ok(Json(json!({ "deleted": true })))
}

/// Resolve an application's local checkout root (its cloned repository path).
async fn checkout_root(state: &AppState, app_id: Uuid) -> AppResult<String> {
    let repo_id = state
        .platform
        .application_repository(app_id)
        .await?
        .ok_or_else(|| AppError::BadRequest("application has no linked repository".into()))?;
    let record = state.repo_records.get(repo_id).await?;
    record
        .local_path
        .ok_or_else(|| AppError::BadRequest("repository has not been cloned yet".into()))
}

fn file_error(err: FileError) -> AppError {
    match err {
        FileError::Forbidden => AppError::BadRequest("invalid path".into()),
        FileError::NotFound => AppError::NotFound("file not found".into()),
        FileError::TooLarge => AppError::BadRequest("file is too large to display".into()),
        FileError::Binary => AppError::BadRequest("file is not text".into()),
        FileError::Io(message) => AppError::internal(message),
    }
}

/// A relative path within a checkout.
#[derive(Deserialize)]
struct FilePathQuery {
    #[serde(default)]
    path: String,
}

/// One directory level of the application's cloned checkout (lazy tree).
async fn browse_files(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    Query(query): Query<FilePathQuery>,
) -> AppResult<Json<Value>> {
    let root = checkout_root(&state, app_id).await?;
    let browser = FileBrowser::new(state.deps.fs.clone());
    let entries = browser.list(&root, &query.path).map_err(file_error)?;
    Ok(Json(json!({ "path": query.path, "entries": entries })))
}

/// The text content of a file within the application's checkout.
async fn file_content(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    Query(query): Query<FilePathQuery>,
) -> AppResult<Json<Value>> {
    let root = checkout_root(&state, app_id).await?;
    let browser = FileBrowser::new(state.deps.fs.clone());
    let file = browser.read(&root, &query.path).map_err(file_error)?;
    Ok(Json(json!({ "path": file.path, "content": file.content })))
}

/// History of questions asked about this application (newest first).
async fn ask_history(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let Some(repository) = state.platform.application_repository(app_id).await? else {
        return Ok(Json(json!({ "questions": [] })));
    };
    let jobs = state.jobs_repo.list().await?;
    let Some(job) = jobs.iter().find(|j| j.job_type == crate::llm_request::JOB_TYPE) else {
        return Ok(Json(json!({ "questions": [] })));
    };
    let repo = repository.to_string();
    let executions = state.executions_repo.list_for_job(job.id, 50).await?;
    let questions: Vec<Value> = executions
        .iter()
        .filter(|e| e.params.get("repository").and_then(|v| v.as_str()) == Some(repo.as_str()))
        .map(|e| {
            json!({
                "execution_id": e.id,
                "question": e.params.get("input"),
                "status": e.status.as_str(),
                "answer": e.metadata.get("answer"),
                "started_at": e.started_at,
                "finished_at": e.finished_at,
            })
        })
        .collect();
    Ok(Json(json!({ "questions": questions })))
}

/// Poll an ask execution: status, raw output, metadata, and the answer.
async fn ask_result(
    State(state): State<AppState>,
    Path((_app_id, execution_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let exec = state.executions_repo.get(execution_id).await?;
    Ok(Json(json!({
        "status": exec.status.as_str(),
        "output": exec.logs,
        "metadata": exec.metadata,
        "answer": exec.metadata.get("answer"),
        "error": exec.error,
    })))
}

/// Build a list query from raw query params: `search`/`page`/`page_size` plus
/// any allowlisted equality filters for the entity.
fn build_list_query(entity: &str, params: &HashMap<String, String>) -> ListQuery {
    let search = params.get("search").cloned();
    let page = params.get("page").and_then(|v| v.parse().ok());
    let page_size = params.get("page_size").and_then(|v| v.parse().ok());
    let mut filters = BTreeMap::new();
    for &field in filter_fields(entity) {
        if let Some(value) = params.get(field).filter(|v| !v.is_empty()) {
            filters.insert(field.to_string(), value.clone());
        }
    }
    ListQuery::new(search, page, page_size, filters)
}

fn validate_entity(entity: &str) -> AppResult<()> {
    if is_entity(entity) {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("unknown entity '{entity}'")))
    }
}

/// Entering the Platform section lands on the Graph tab by default.
async fn overview_page() -> Redirect {
    Redirect::to("/platform/graph")
}

#[derive(Deserialize)]
struct GraphParams {
    #[serde(default)]
    center: Option<Uuid>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn graph_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(&state.engine, "platform_graph.html", &page, context! { active_tab => "graph" })
}

async fn graph_api(
    State(state): State<AppState>,
    Query(params): Query<GraphParams>,
) -> AppResult<Json<Value>> {
    let scope = GraphScope::new(params.center, params.limit);
    let graph = state.graph.build(&scope).await?;
    Ok(Json(graph))
}

async fn list_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(entity): Path<String>,
) -> AppResult<Html<String>> {
    validate_entity(&entity)?;
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(
        &state.engine,
        "platform_list.html",
        &page,
        context! { entity => entity, active_tab => entity },
    )
}

async fn detail_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path((entity, id)): Path<(String, Uuid)>,
) -> AppResult<Html<String>> {
    validate_entity(&entity)?;
    let page = PageContext::new(Some(user.display_name), "platform");
    let template = if entity == "applications" {
        "platform_app_detail.html"
    } else {
        "platform_detail.html"
    };
    render_page(
        &state.engine,
        template,
        &page,
        context! { entity => entity, entity_id => id.to_string(), active_tab => entity },
    )
}

async fn list_api(
    State(state): State<AppState>,
    Path(entity): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<Json<Value>> {
    validate_entity(&entity)?;
    let query = build_list_query(&entity, &params);
    let page = state.platform.list(&entity, &query).await?;
    Ok(Json(json!({
        "items": page.items,
        "total": page.total,
        "page": page.page,
        "page_size": page.page_size,
    })))
}

/// Distinct values for each filterable field of an entity (filter dropdowns).
async fn facets_api(
    State(state): State<AppState>,
    Path(entity): Path<String>,
) -> AppResult<Json<Value>> {
    validate_entity(&entity)?;
    let facets = state.platform.facets(&entity).await?;
    Ok(Json(facets))
}

async fn detail_api(
    State(state): State<AppState>,
    Path((entity, id)): Path<(String, Uuid)>,
) -> AppResult<Json<Value>> {
    validate_entity(&entity)?;
    let detail = state.platform.detail(&entity, id).await?;
    Ok(Json(json!({ "detail": detail })))
}
