//! Auto-remediation routes (M46): rule CRUD, the evaluator, the remediation queue
//! and the approval gate (which opens an agent task).

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::platform::ListQuery;
use crate::remediation::repository::RuleInput;
use crate::remediation::{AppSignals, RemediationService};
use crate::scorecards::{ScorecardInput, evaluate as score};
use crate::techradar::{DepInput, assess};
use crate::web::{PageContext, render_page};
use axum::extract::{Path, Query, State};
use axum::response::Html;
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use chrono::{NaiveDate, Utc};
use minijinja::context;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/platform/remediation", get(remediation_page))
        .route("/api/platform/remediation/rules", get(list_rules).post(create_rule))
        .route("/api/platform/remediation/rules/:id", delete(delete_rule))
        .route("/api/platform/remediation/evaluate", post(run_evaluate))
        .route("/api/platform/remediations", get(list_remediations))
        .route("/api/platform/remediations/:id/approve", post(approve))
        .route("/api/platform/remediations/:id/dismiss", post(dismiss))
}

async fn remediation_page(State(state): State<AppState>, Extension(user): Extension<Principal>) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(&state.engine, "remediation.html", &page, context! { active_tab => "remediation" })
}

fn service(state: &AppState) -> RemediationService {
    RemediationService::new(state.remediation.clone())
}

/// `(ecosystem, name) → (latest version, EOL date)` for currency lookups.
type PolicyMap = HashMap<(String, String), (Option<String>, Option<NaiveDate>)>;

/// Assemble an application's remediation signals (metrics, failed scorecard
/// checks, end-of-life dependency count).
async fn app_signals(
    state: &AppState,
    app_id: Uuid,
    checks: &[crate::scorecards::engine::Check],
    policies: &PolicyMap,
    today: NaiveDate,
) -> AppResult<AppSignals> {
    let detail = state.platform.detail("applications", app_id).await?;
    let metrics: HashMap<String, f64> = state
        .metrics
        .latest_for_application(app_id)
        .await?
        .into_iter()
        .map(|m| (m.metric_key, m.value))
        .collect();
    let owner_teams = state.rbac.owner_team_names(app_id).await.unwrap_or_default();
    let card = score(&ScorecardInput { app_detail: detail.clone(), metrics: metrics.clone(), owner_teams }, checks);
    let failed_checks: HashSet<String> =
        card.results.iter().filter(|r| !r.passed).map(|r| r.check_id.clone()).collect();
    let eol_count = detail
        .get("libraries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|lib| {
            let name = lib.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let ecosystem = lib.get("ecosystem").and_then(Value::as_str).unwrap_or("").to_string();
            let version = lib.get("version").and_then(Value::as_str).unwrap_or("").to_string();
            let (latest, eol) = policies.get(&(ecosystem.clone(), name.clone())).cloned().unwrap_or((None, None));
            assess(&DepInput { name, ecosystem, version, latest, eol }, today, 90).eol_status == "eol"
        })
        .count() as i64;
    Ok(AppSignals { metrics, failed_checks, eol_count })
}

async fn policy_map(state: &AppState) -> AppResult<PolicyMap> {
    Ok(state
        .techradar
        .list_policies()
        .await?
        .into_iter()
        .map(|p| {
            let eol = p.eol_date.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            ((p.ecosystem, p.name), (p.latest, eol))
        })
        .collect())
}

async fn run_evaluate(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let checks = crate::routes::scorecards::active_checks(&state).await?;
    let policies = policy_map(&state).await?;
    let today = Utc::now().date_naive();
    let page = state
        .platform
        .list("applications", &ListQuery::new(None, None, Some(500), Default::default()))
        .await?;
    let mut apps = Vec::new();
    for item in &page.items {
        let Some(id) = item.get("id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) else {
            continue;
        };
        apps.push((id, app_signals(&state, id, &checks, &policies, today).await?));
    }
    let proposed = service(&state).evaluate(&apps).await?;
    Ok(Json(json!({ "proposed": proposed })))
}

async fn list_rules(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "rules": state.remediation.list_rules().await? })))
}

#[derive(Deserialize)]
struct RuleBody {
    name: String,
    trigger_kind: String,
    #[serde(default)]
    params: Value,
    action: String,
    prompt: String,
    #[serde(default)]
    scope: Value,
    #[serde(default)]
    auto_approve: bool,
}

async fn create_rule(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Json(body): Json<RuleBody>,
) -> AppResult<Json<Value>> {
    if !user.has_role("admin") {
        return Err(AppError::Unauthorized);
    }
    if body.name.trim().is_empty() || body.prompt.trim().is_empty() {
        return Err(AppError::BadRequest("name and prompt are required".into()));
    }
    let rule = state
        .remediation
        .create_rule(RuleInput {
            name: body.name,
            trigger_kind: body.trigger_kind,
            params: body.params,
            action: body.action,
            prompt: body.prompt,
            scope: body.scope,
            auto_approve: body.auto_approve,
        })
        .await?;
    Ok(Json(json!(rule)))
}

async fn delete_rule(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    if !user.has_role("admin") {
        return Err(AppError::Unauthorized);
    }
    state.remediation.delete_rule(id).await?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
struct StatusQuery {
    status: Option<String>,
}

async fn list_remediations(State(state): State<AppState>, Query(q): Query<StatusQuery>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "remediations": state.remediation.list_remediations(q.status).await? })))
}

/// Approve a proposed remediation: open an agent task for its application and
/// mark it running. Maintainers may only approve apps their team owns.
async fn approve(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let rem = state.remediation.get_remediation(id).await?.ok_or_else(|| AppError::NotFound("remediation".into()))?;
    if rem.status != "proposed" {
        return Err(AppError::BadRequest("remediation is not pending approval".into()));
    }
    let app_id = rem.application_id.ok_or_else(|| AppError::BadRequest("remediation has no application".into()))?;
    if !state.rbac.can_mutate_app(&user, app_id).await? {
        return Err(AppError::Unauthorized);
    }
    let prompt = state
        .remediation
        .list_rules()
        .await?
        .into_iter()
        .find(|r| Some(r.id) == rem.rule_id)
        .map(|r| r.prompt)
        .unwrap_or_else(|| "Apply the remediation.".to_string());
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
            title: "Auto-remediation".to_string(),
        })
        .await?;
    crate::routes::platform::record_user_message(&state, task.id, &prompt).await?;
    crate::routes::platform::add_target_and_enqueue(&state, &task, repository, &prompt).await?;
    service(&state).mark_running(id, task.id).await?;
    state.audit.record(&user.username, "remediation.approve", Some(&id.to_string()), json!({})).await;
    Ok(Json(json!({ "task_id": task.id.to_string() })))
}

async fn dismiss(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    state.remediation.set_status(id, "dismissed", None).await?;
    Ok(Json(json!({ "dismissed": true })))
}
