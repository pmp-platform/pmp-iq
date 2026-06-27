//! SQLite integration test (no container) for application sub-entities:
//! components, use cases, diagrams and observability signals persist and surface
//! in the application detail; re-sync removes stale ones; and `prune_orphans`
//! deletes unused shared entities while leaving users intact.

mod common;
use common::SqliteDb;
use platform_inspector::store;

use platform_inspector::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platform_inspector::platform::AnalysisResult;
use platform_inspector::repositories::RepoRecordInput;
use uuid::Uuid;

const WITH_SUB: &str = r#"{
  "application": {"name":"api"},
  "libraries":[{"name":"axum","ecosystem":"cargo","version":"0.7"}],
  "components":[
    {"name":"UserController","kind":"controller","description":"HTTP entry",
     "observability_signals":[{"name":"req_count","kind":"metric","description":"requests"}]},
    {"name":"UserService","kind":"service","description":"logic"}],
  "use_cases":[
    {"name":"Register","description":"sign up","components":["UserController","UserService"],
     "diagrams":[{"name":"Flow","kind":"flowchart","description":"d","content":"flowchart TD; A-->B"}]}]
}"#;

async fn make_repo(db: &SqliteDb, full_name: &str) -> Uuid {
    let database = db.database();
    let account = store::accounts(&database)
        .create(AccountInput {
            name: format!("acc-{full_name}"),
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
    store::repo_records(&database)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: full_name.into(),
            full_name: full_name.into(),
            clone_url: format!("/repos/{full_name}"),
            default_branch: None,
        })
        .await
        .unwrap()
        .id
}

async fn count(db: &SqliteDb, table: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&db.pool)
        .await
        .unwrap();
    n
}

#[tokio::test]
async fn persists_subentities_and_surfaces_in_detail() {
    let db = SqliteDb::start().await;
    let repo_id = make_repo(&db, "org/api").await;
    let result = AnalysisResult::parse(WITH_SUB).unwrap();
    let app_id = store::platform_writer(&db.database()).write(repo_id, &result).await.unwrap();

    assert_eq!(count(&db, "components").await, 2);
    assert_eq!(count(&db, "use_cases").await, 1);
    assert_eq!(count(&db, "observability_signals").await, 1);
    assert_eq!(count(&db, "diagrams").await, 1);
    assert_eq!(count(&db, "use_case_components").await, 2);

    let detail = store::platform_query(&db.database())
        .detail("applications", app_id)
        .await
        .unwrap();
    assert_eq!(detail["components"].as_array().unwrap().len(), 2);
    let uc = &detail["use_cases"][0];
    assert_eq!(uc["name"], "Register");
    assert_eq!(uc["components"].as_array().unwrap().len(), 2);
    assert_eq!(uc["diagrams"][0]["content"], "flowchart TD; A-->B");
    let controller = detail["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "UserController")
        .unwrap();
    assert_eq!(controller["observability_signals"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn resync_removes_stale_subentities() {
    let db = SqliteDb::start().await;
    let repo_id = make_repo(&db, "org/api").await;
    let writer = store::platform_writer(&db.database());
    writer.write(repo_id, &AnalysisResult::parse(WITH_SUB).unwrap()).await.unwrap();
    assert_eq!(count(&db, "components").await, 2);
    assert_eq!(count(&db, "observability_signals").await, 1);

    // Re-sync with no components/use cases → all removed via CASCADE.
    writer
        .write(repo_id, &AnalysisResult::parse(r#"{"application":{"name":"api"}}"#).unwrap())
        .await
        .unwrap();
    assert_eq!(count(&db, "components").await, 0);
    assert_eq!(count(&db, "use_cases").await, 0);
    assert_eq!(count(&db, "observability_signals").await, 0);
    assert_eq!(count(&db, "diagrams").await, 0);
}

#[tokio::test]
async fn prune_orphans_removes_unused_shared_entities_but_keeps_users() {
    let db = SqliteDb::start().await;
    let repo_id = make_repo(&db, "org/api").await;
    let writer = store::platform_writer(&db.database());
    writer.write(repo_id, &AnalysisResult::parse(WITH_SUB).unwrap()).await.unwrap();
    assert_eq!(count(&db, "libraries").await, 1);

    // Re-sync without the library (orphaning it) but with a codeowner user.
    writer
        .write(
            repo_id,
            &AnalysisResult::parse(
                r#"{"application":{"name":"api"},"users":[{"username":"alice"}],
                    "access":[{"principal_type":"user","principal_name":"alice","access_level":"owner"}]}"#,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(count(&db, "users").await, 1);

    writer.prune_orphans().await.unwrap();
    assert_eq!(count(&db, "libraries").await, 0, "orphaned library pruned");
    assert_eq!(count(&db, "library_versions").await, 0, "orphaned version pruned");
    assert_eq!(count(&db, "users").await, 1, "users are never pruned");
}
