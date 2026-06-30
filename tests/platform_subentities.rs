//! SQLite integration test (no container) for application sub-entities:
//! components, use cases, diagrams and observability signals persist and surface
//! in the application detail; re-sync removes stale ones; and `prune_orphans`
//! deletes unused shared entities while leaving users intact.

mod common;
use common::SqliteDb;
use pmp_iq::store;

use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
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
     "diagrams":[{"name":"Flow","kind":"flowchart","description":"d","content":"flowchart TD; A-->B"}]}],
  "dependencies":[{"target_name":"payment-service","kind":"http","description":"charges","component":"UserService"}]
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

    // The code-derived dependency is mapped to its originating component.
    let dep = &detail["dependencies"][0];
    assert_eq!(dep["target_name"], "payment-service");
    assert_eq!(dep["component_name"], "UserService");
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

#[tokio::test]
async fn canonicalizes_dependency_target_against_catalog() {
    use pmp_iq::platform::catalog::resolve_dependencies;

    let db = SqliteDb::start().await;
    let writer = store::platform_writer(&db.database());

    // Seed two known applications the catalog can match against.
    let auth_repo = make_repo(&db, "org/auth-service").await;
    let auth_app_id = writer
        .write(auth_repo, &AnalysisResult::parse(r#"{"application":{"name":"auth-service"}}"#).unwrap())
        .await
        .unwrap();
    let billing_repo = make_repo(&db, "org/billing").await;
    writer
        .write(billing_repo, &AnalysisResult::parse(r#"{"application":{"name":"billing"}}"#).unwrap())
        .await
        .unwrap();

    // Snapshot the catalog and resolve a consumer's fuzzy "auth" target
    // (provider None → exact/normalized/fuzzy matching only, no LLM call).
    let catalog = store::platform_query(&db.database()).catalog().await.unwrap();
    assert!(!catalog.is_empty());
    let mut result = AnalysisResult::parse(
        r#"{"application":{"name":"consumer"},
            "dependencies":[{"target_name":"auth","kind":"http","description":"login"}]}"#,
    )
    .unwrap();
    assert_eq!(resolve_dependencies(&mut result, &catalog, None).await, 1);
    assert_eq!(result.dependencies[0].target_name, "auth-service");

    // Persist the consumer; the canonicalized target now links to the auth app.
    let consumer_repo = make_repo(&db, "org/consumer").await;
    let consumer_app_id = writer.write(consumer_repo, &result).await.unwrap();
    let detail = store::platform_query(&db.database())
        .detail("applications", consumer_app_id)
        .await
        .unwrap();
    let dep = &detail["dependencies"][0];
    assert_eq!(dep["target_name"], "auth-service");
    assert_eq!(dep["target_app_id"], auth_app_id.to_string());
}
