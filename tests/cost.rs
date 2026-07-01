//! Integration tests for LLM cost & budgeting (M39): usage aggregation, the
//! cost panel API, and budget CRUD — all on SQLite (no Docker).

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::Request;
use chrono::Utc;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::app::build_router;
use pmp_iq::cost::{
    BudgetInput, BudgetPeriod, BudgetScope, CostDimension, LlmUsageInput,
};
use pmp_iq::store;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

/// Seed one usage row.
fn usage(model: &str, app: Option<Uuid>, profile: Option<Uuid>, input: i64, output: i64) -> LlmUsageInput {
    LlmUsageInput {
        job_execution_id: Uuid::new_v4(),
        application_id: app,
        ai_profile_id: profile,
        model: model.into(),
        input_tokens: input,
        output_tokens: output,
    }
}

#[tokio::test]
async fn usage_aggregates_by_scope_and_dimension() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let repo = store::llm_usage(&db);
    let app_a = Uuid::new_v4();
    let app_b = Uuid::new_v4();

    repo.record(&usage("claude-opus-4", Some(app_a), None, 1_000_000, 0)).await.unwrap();
    repo.record(&usage("claude-sonnet-4", Some(app_a), None, 2_000_000, 0)).await.unwrap();
    repo.record(&usage("claude-opus-4", Some(app_b), None, 1_000_000, 0)).await.unwrap();

    let epoch = chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap();

    // Global, by model.
    let global = repo.usage_since(BudgetScope::Global, None, epoch).await.unwrap();
    let opus = global.iter().find(|m| m.model == "claude-opus-4").unwrap();
    assert_eq!(opus.input_tokens, 2_000_000);

    // Application-scoped.
    let a = repo.usage_since(BudgetScope::Application, Some(app_a), epoch).await.unwrap();
    let total_a: i64 = a.iter().map(|m| m.input_tokens).sum();
    assert_eq!(total_a, 3_000_000);

    // Grouped by application → two keys, each priced separately downstream.
    let grouped = repo.grouped(CostDimension::Application, epoch).await.unwrap();
    let keys: std::collections::HashSet<&str> = grouped.iter().map(|g| g.key.as_str()).collect();
    assert!(keys.contains(app_a.to_string().as_str()));
    assert!(keys.contains(app_b.to_string().as_str()));

    // Per-execution rollup sums a single execution's calls by model.
    let exec = Uuid::new_v4();
    let mut row = usage("claude-opus-4", None, None, 5, 6);
    row.job_execution_id = exec;
    repo.record(&row).await.unwrap();
    let mut row2 = usage("claude-opus-4", None, None, 1, 2);
    row2.job_execution_id = exec;
    repo.record(&row2).await.unwrap();
    let per_exec = repo.usage_for_execution(exec).await.unwrap();
    assert_eq!(per_exec.len(), 1);
    assert_eq!(per_exec[0].input_tokens, 6);
    assert_eq!(per_exec[0].output_tokens, 8);
}

async fn post_json(app: &Router, cookies: &[String], uri: &str, body: Value) -> (u16, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::post(uri)
                .header(COOKIE, cookie_header(cookies))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

#[tokio::test]
async fn cost_panel_reports_spend_and_budget_status() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    // 1M opus output tokens @ $75/Mtok = $75 spent this month.
    store::llm_usage(&db)
        .record(&usage("claude-opus-4", None, None, 0, 1_000_000))
        .await
        .unwrap();
    // A global monthly budget of $50, hard-stop → should report over-budget.
    store::llm_budgets(&db)
        .create(&BudgetInput {
            scope: BudgetScope::Global,
            scope_id: None,
            period: BudgetPeriod::Monthly,
            limit_usd: 50.0,
            hard_stop: true,
        })
        .await
        .unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let resp = app
        .clone()
        .oneshot(
            Request::get("/api/platform/cost")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!((body["spend_this_month"].as_f64().unwrap() - 75.0).abs() < 1e-6, "{body}");
    let budgets = body["budgets"].as_array().unwrap();
    assert_eq!(budgets.len(), 1);
    assert_eq!(budgets[0]["over"], json!(true));
    assert!((budgets[0]["spent_usd"].as_f64().unwrap() - 75.0).abs() < 1e-6);
}

#[tokio::test]
async fn execution_cost_endpoint_prices_a_single_execution() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let exec = Uuid::new_v4();
    let mut row = usage("claude-opus-4", None, None, 0, 1_000_000);
    row.job_execution_id = exec;
    store::llm_usage(&db).record(&row).await.unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let body = common_get(&app, &cookies, &format!("/api/jobs/executions/{exec}/cost")).await;
    assert_eq!(body["output_tokens"], 1_000_000);
    assert!((body["cost_usd"].as_f64().unwrap() - 75.0).abs() < 1e-6);
}

#[tokio::test]
async fn budget_crud_via_api() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Create.
    let (status, created) = post_json(
        &app,
        &cookies,
        "/api/cost/budgets",
        json!({ "scope": "global", "period": "monthly", "limit_usd": 100.0, "hard_stop": false }),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let id = created["id"].as_str().unwrap().to_string();

    // List shows it.
    let list = common_get(&app, &cookies, "/api/cost/budgets").await;
    assert_eq!(list["budgets"].as_array().unwrap().len(), 1);

    // Invalid scope is rejected.
    let (bad, _) = post_json(
        &app,
        &cookies,
        "/api/cost/budgets",
        json!({ "scope": "nope", "period": "monthly", "limit_usd": 10.0 }),
    )
    .await;
    assert_eq!(bad, 400);

    // A non-global budget without a scope_id is rejected.
    let (missing_scope, _) = post_json(
        &app,
        &cookies,
        "/api/cost/budgets",
        json!({ "scope": "application", "period": "daily", "limit_usd": 10.0 }),
    )
    .await;
    assert_eq!(missing_scope, 400);

    // Delete.
    let resp = app
        .clone()
        .oneshot(
            Request::delete(format!("/api/cost/budgets/{id}"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let after = common_get(&app, &cookies, "/api/cost/budgets").await;
    assert!(after["budgets"].as_array().unwrap().is_empty());
}

async fn common_get(app: &Router, cookies: &[String], uri: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(
            Request::get(uri)
                .header(COOKIE, cookie_header(cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
