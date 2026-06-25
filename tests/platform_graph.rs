//! Integration test for the platform connection-graph API.

mod common;
use platform_inspector::store;

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Method, Request};
use axum::response::Response;
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platform_inspector::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platform_inspector::app::build_router;
use platform_inspector::platform::AnalysisResult;
use platform_inspector::repositories::RepoRecordInput;
use serde_json::Value;
use tower::ServiceExt;

async fn body_json(resp: Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn get(path: &str, cookies: &[String]) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(COOKIE, cookie_header(cookies))
        .body(Body::empty())
        .unwrap()
}

async fn seed(db: &TestDb, name: &str, json: &str) {
    let account = store::accounts(&db.database())
        .create(AccountInput {
            name: format!("acc-{name}"),
            provider_type: ProviderType::Local,
            auth_type: AuthType::None,
            base_url: Some("/r".into()),
            credentials_enc: None,
            selection_mode: SelectionMode::All,
            selection_value: None,
            enabled: true,
        })
        .await
        .unwrap();
    let record = store::repo_records(&db.database())
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: name.into(),
            full_name: format!("org/{name}"),
            clone_url: format!("/r/{name}"),
            default_branch: None,
        })
        .await
        .unwrap();
    let result = AnalysisResult::parse(json).unwrap();
    store::platform_writer(&db.database())
        .write(record.id, &result)
        .await
        .unwrap();
}

#[tokio::test]
async fn graph_has_app_infra_and_app_app_edges() {
    let db = TestDb::start().await;
    // billing -> postgres (infra) and billing -> shipping (app dependency).
    seed(
        &db,
        "billing",
        r#"{"application":{"name":"billing"},
            "infrastructure":[{"name":"PostgreSQL","kind":"database"}],
            "dependencies":[{"target_name":"shipping","kind":"http"}]}"#,
    )
    .await;
    seed(&db, "shipping", r#"{"application":{"name":"shipping"}}"#).await;

    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let resp = app
        .clone()
        .oneshot(get("/api/platform/graph", &cookies))
        .await
        .unwrap();
    let graph = body_json(resp).await;
    let nodes = graph["nodes"].as_array().unwrap();
    let edges = graph["edges"].as_array().unwrap();

    // 2 application nodes + 1 infrastructure node.
    let app_nodes = nodes.iter().filter(|n| n["data"]["kind"] == "application").count();
    let infra_nodes = nodes.iter().filter(|n| n["data"]["kind"] == "infrastructure").count();
    assert_eq!(app_nodes, 2);
    assert_eq!(infra_nodes, 1);

    // An app->app edge and an app->infra edge exist.
    let has_http = edges.iter().any(|e| e["data"]["kind"] == "http");
    let has_db = edges.iter().any(|e| e["data"]["kind"] == "database");
    assert!(has_http, "expected app->app http edge");
    assert!(has_db, "expected app->infra database edge");
    assert_eq!(graph["truncated"], false);
}
