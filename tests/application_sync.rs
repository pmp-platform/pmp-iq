//! Integration test for the per-application Sync button: it schedules a
//! `sync-repositories` execution scoped to the application's repository.

mod common;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::COOKIE;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platiq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platiq::app::build_router;
use platiq::platform::AnalysisResult;
use platiq::repositories::RepoRecordInput;
use platiq::review;
use platiq::store;
use serde_json::Value;
use tower::ServiceExt;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
  "use_cases":[],"users":[],"groups":[],"access":[]
}"#;

#[tokio::test]
async fn sync_schedules_scoped_job() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();

    let account = store::accounts(&db)
        .create(AccountInput {
            name: "gh".into(),
            provider_type: ProviderType::Github,
            auth_type: AuthType::Token,
            base_url: None,
            credentials_enc: None,
            selection_mode: SelectionMode::All,
            selection_value: None,
            enabled: true,
        })
        .await
        .unwrap();
    let repo = store::repo_records(&db)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: "api".into(),
            full_name: "org/api".into(),
            clone_url: "https://example.invalid/org/api.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    let result = AnalysisResult::parse(ANALYSIS).unwrap();
    let app_id = store::platform_writer(&db).write(repo.id, &result).await.unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let resp = app
        .clone()
        .oneshot(
            Request::post(format!("/api/platform/applications/{app_id}/sync"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, 200, "sync should schedule: {body}");
    assert!(body.get("execution_id").is_some(), "carries an execution_id: {body}");

    // A sync-repositories job now exists and the execution is scoped to the repo.
    let jobs = store::jobs(&db).list().await.unwrap();
    let sync_job = jobs.iter().find(|j| j.job_type == review::JOB_TYPE).expect("sync job seeded");
    let execs = store::job_executions(&db).list_for_job(sync_job.id, 10).await.unwrap();
    assert_eq!(execs.len(), 1, "one scoped execution");
    assert_eq!(execs[0].params["repository_id"], repo.id.to_string());
}
