//! LLM cost & budget routes (M39): a spend rollup for the Insights dashboard
//! and budget management in Settings.

use crate::app::AppState;
use crate::cost::{
    Budget, BudgetInput, BudgetPeriod, BudgetScope, CostDimension, CostRow, price_rows,
};
use crate::error::{AppError, AppResult};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// Top-N spenders surfaced per rollup dimension.
const TOP_N: usize = 10;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/platform/cost", get(cost_panel))
        .route("/api/jobs/executions/:id/cost", get(execution_cost))
        .route("/api/cost/budgets", get(list_budgets).post(create_budget))
        .route("/api/cost/budgets/:id", delete(delete_budget))
}

/// Priced cost of one job execution (tokens × the per-model price map).
async fn execution_cost(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let rows = state.llm_usage.usage_for_execution(id).await?;
    let input: i64 = rows.iter().map(|r| r.input_tokens).sum();
    let output: i64 = rows.iter().map(|r| r.output_tokens).sum();
    Ok(Json(json!({
        "input_tokens": input,
        "output_tokens": output,
        "cost_usd": state.config.pricing.total(&rows),
    })))
}

/// The Insights cost panel: month/day spend, projection, top spenders per
/// dimension, and each budget's period-to-date status.
async fn cost_panel(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let prices = &state.config.pricing;
    let now = Utc::now();
    let month_start = BudgetPeriod::Monthly.start(now);
    let day_start = BudgetPeriod::Daily.start(now);

    let month_rows = state.llm_usage.usage_since(BudgetScope::Global, None, month_start).await?;
    let day_rows = state.llm_usage.usage_since(BudgetScope::Global, None, day_start).await?;
    let spend_month = prices.total(&month_rows);
    let spend_day = prices.total(&day_rows);

    let by_application = top_rollup(&state, CostDimension::Application, month_start).await?;
    let by_profile = top_rollup(&state, CostDimension::Profile, month_start).await?;
    let by_job_type = top_rollup(&state, CostDimension::JobType, month_start).await?;

    let day = now.day() as f64;
    let projected = if day > 0.0 { spend_month / day * days_in_month(now) as f64 } else { spend_month };

    Ok(Json(json!({
        "spend_this_month": spend_month,
        "spend_today": spend_day,
        "projected_month_end": projected,
        "by_application": by_application,
        "by_profile": by_profile,
        "by_job_type": by_job_type,
        "budgets": budget_status(&state, now).await?,
    })))
}

/// Priced top-N rollup for one dimension since `since`.
async fn top_rollup(state: &AppState, dim: CostDimension, since: DateTime<Utc>) -> AppResult<Vec<CostRow>> {
    let grouped = state.llm_usage.grouped(dim, since).await?;
    let mut rows = price_rows(grouped, &state.config.pricing);
    rows.truncate(TOP_N);
    Ok(rows)
}

/// Number of days in the calendar month containing `now`.
fn days_in_month(now: DateTime<Utc>) -> i64 {
    (BudgetPeriod::Monthly.next_start(now) - BudgetPeriod::Monthly.start(now)).num_days()
}

/// Each budget's period-to-date spend vs. its limit.
async fn budget_status(state: &AppState, now: DateTime<Utc>) -> AppResult<Vec<Value>> {
    let budgets = state.llm_budgets.list().await?;
    let mut out = Vec::with_capacity(budgets.len());
    for b in budgets {
        let rows = state.llm_usage.usage_since(b.scope, b.scope_id, b.period.start(now)).await?;
        let spent = state.config.pricing.total(&rows);
        let mut row = serialize_budget(&b);
        if let Some(obj) = row.as_object_mut() {
            obj.insert("spent_usd".into(), json!(spent));
            obj.insert("over".into(), json!(spent >= b.limit_usd));
        }
        out.push(row);
    }
    Ok(out)
}

fn serialize_budget(b: &Budget) -> Value {
    json!({
        "id": b.id.to_string(),
        "scope": b.scope.as_str(),
        "scope_id": b.scope_id.map(|i| i.to_string()),
        "period": b.period.as_str(),
        "limit_usd": b.limit_usd,
        "hard_stop": b.hard_stop,
    })
}

async fn list_budgets(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let budgets = state.llm_budgets.list().await?;
    let items: Vec<Value> = budgets.iter().map(serialize_budget).collect();
    Ok(Json(json!({ "budgets": items })))
}

/// Request body for creating a budget.
#[derive(Deserialize)]
struct NewBudget {
    scope: String,
    #[serde(default)]
    scope_id: Option<Uuid>,
    period: String,
    limit_usd: f64,
    #[serde(default)]
    hard_stop: bool,
}

async fn create_budget(
    State(state): State<AppState>,
    Json(body): Json<NewBudget>,
) -> AppResult<Json<Value>> {
    let scope = BudgetScope::parse(&body.scope).map_err(AppError::BadRequest)?;
    let period = BudgetPeriod::parse(&body.period).map_err(AppError::BadRequest)?;
    if body.limit_usd <= 0.0 {
        return Err(AppError::BadRequest("limit_usd must be positive".into()));
    }
    if !matches!(scope, BudgetScope::Global) && body.scope_id.is_none() {
        return Err(AppError::BadRequest("scope_id is required for a non-global budget".into()));
    }
    let budget = state
        .llm_budgets
        .create(&BudgetInput {
            scope,
            scope_id: if matches!(scope, BudgetScope::Global) { None } else { body.scope_id },
            period,
            limit_usd: body.limit_usd,
            hard_stop: body.hard_stop,
        })
        .await?;
    Ok(Json(serialize_budget(&budget)))
}

async fn delete_budget(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    state.llm_budgets.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
