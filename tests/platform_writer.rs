//! Integration test for the platform writer against PostgreSQL: a full analysis
//! result populates every model table, and re-analysis is idempotent.

mod common;
use platform_inspector::store;

use common::TestDb;
use platform_inspector::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platform_inspector::platform::AnalysisResult;
use platform_inspector::repositories::RepoRecordInput;
use sqlx::PgPool;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{"k":"v"}},
  "languages":[{"name":"Rust","percentage":90.0},{"name":"SQL","percentage":10.0}],
  "libraries":[{"name":"axum","ecosystem":"cargo","version":"0.7","scope":"runtime"},
               {"name":"serde","ecosystem":"cargo","version":"1.0","scope":"runtime"}],
  "infrastructure":[{"name":"PostgreSQL","kind":"database","version":"16","usage":"primary"}],
  "dependencies":[{"target_name":"auth-service","kind":"http","description":"calls auth"}],
  "users":[{"username":"alice","email":"alice@x.com","groups":["devs"]}],
  "groups":[{"name":"devs"}],
  "access":[{"principal_type":"group","principal_name":"devs","access_level":"write"},
            {"principal_type":"user","principal_name":"alice","access_level":"read"}]
}"#;

async fn count(pool: &PgPool, table: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .unwrap();
    n
}

async fn assert_full_model(pool: &PgPool) {
    assert_eq!(count(pool, "applications").await, 1, "applications");
    assert_eq!(count(pool, "languages").await, 2, "languages");
    assert_eq!(count(pool, "application_languages").await, 2, "application_languages");
    assert_eq!(count(pool, "libraries").await, 2, "libraries");
    assert_eq!(count(pool, "library_versions").await, 2, "library_versions");
    assert_eq!(count(pool, "application_libraries").await, 2, "application_libraries");
    assert_eq!(count(pool, "infrastructure").await, 1, "infrastructure");
    assert_eq!(count(pool, "application_infrastructure").await, 1, "application_infrastructure");
    assert_eq!(count(pool, "application_dependencies").await, 1, "application_dependencies");
    assert_eq!(count(pool, "users").await, 1, "users");
    assert_eq!(count(pool, "groups").await, 1, "groups");
    assert_eq!(count(pool, "group_memberships").await, 1, "group_memberships");
    assert_eq!(count(pool, "access_grants").await, 2, "access_grants");
}

#[tokio::test]
async fn writes_full_model_and_is_idempotent() {
    let db = TestDb::start().await;
    let database = db.database();

    // A repository to attach the application to.
    let account = store::accounts(&database)
        .create(AccountInput {
            name: "acc".into(),
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
    let record = store::repo_records(&database)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: "api".into(),
            full_name: "org/api".into(),
            clone_url: "/repos/api".into(),
            default_branch: None,
        })
        .await
        .unwrap();

    let result = AnalysisResult::parse(ANALYSIS).unwrap();
    let writer = store::platform_writer(&database);

    let app_id = writer.write(record.id, &result).await.unwrap();
    assert_full_model(&db.pool).await;

    // Re-analysis: same application id, no duplicated rows.
    let app_id2 = writer.write(record.id, &result).await.unwrap();
    assert_eq!(app_id, app_id2);
    assert_full_model(&db.pool).await;
}
