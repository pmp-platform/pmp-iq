//! Integration test for the platform read API: seed the model via the writer,
//! then exercise list (search + pagination) and detail endpoints.

mod common;
use platform_inspector::store;

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platform_inspector::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platform_inspector::app::build_router;
use platform_inspector::platform::AnalysisResult;
use platform_inspector::repositories::RepoRecordInput;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

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

/// Seed an application named `app_name` with one library + infrastructure.
async fn seed_app(db: &TestDb, app_name: &str) -> Uuid {
    let account = store::accounts(&db.database())
        .create(AccountInput {
            name: format!("acc-{app_name}"),
            provider_type: ProviderType::Local,
            auth_type: AuthType::None,
            base_url: Some("/repos".into()),
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
            name: app_name.into(),
            full_name: format!("org/{app_name}"),
            clone_url: format!("/repos/{app_name}"),
            default_branch: None,
        })
        .await
        .unwrap();
    let json = format!(
        r#"{{"application":{{"name":"{app_name}","app_type":"api"}},
            "libraries":[{{"name":"axum","ecosystem":"cargo","version":"0.7"}}],
            "infrastructure":[{{"name":"PostgreSQL","kind":"database","version":"16"}}],
            "access":[{{"principal_type":"group","principal_name":"devs","access_level":"write"}}]}}"#
    );
    let result = AnalysisResult::parse(&json).unwrap();
    store::platform_writer(&db.database())
        .write(record.id, &result)
        .await
        .unwrap()
}

#[tokio::test]
async fn lists_filters_and_details() {
    let db = TestDb::start().await;
    let app_id = seed_app(&db, "billing").await;
    seed_app(&db, "shipping").await;

    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // List applications — both present.
    let resp = app
        .clone()
        .oneshot(get("/api/platform/applications", &cookies))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);

    // Search narrows to one.
    let resp = app
        .clone()
        .oneshot(get("/api/platform/applications?search=bill", &cookies))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["name"], "billing");

    // Pagination: page_size 1.
    let resp = app
        .clone()
        .oneshot(get("/api/platform/applications?page=2&page_size=1", &cookies))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["page"], 2);

    // Application detail includes relations.
    let resp = app
        .clone()
        .oneshot(get(&format!("/api/platform/applications/{app_id}"), &cookies))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let detail = body_json(resp).await;
    assert_eq!(detail["detail"]["name"], "billing");
    assert_eq!(detail["detail"]["libraries"][0]["name"], "axum");
    assert_eq!(detail["detail"]["infrastructure"][0]["name"], "PostgreSQL");

    // Infrastructure and groups lists are populated.
    let resp = app
        .clone()
        .oneshot(get("/api/platform/infrastructure", &cookies))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["total"], 1);

    let resp = app
        .clone()
        .oneshot(get("/api/platform/groups", &cookies))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["total"], 1);

    // Unknown entity is 404.
    let resp = app
        .oneshot(get("/api/platform/widgets", &cookies))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
