//! Integration test for per-entity LLM hints on SQLite: hints are keyed by the
//! entity's natural name, survive the per-sync delete-and-recreate of
//! sub-entities, and support upsert/delete.

mod common;

use common::SqliteDb;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::hints::EntityHintInput;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],
  "components":[{"name":"Svc","kind":"service","description":"does things"}],
  "use_cases":[{"name":"Checkout","description":"buy","components":["Svc"],"diagrams":[]}],
  "users":[],"groups":[],"access":[]
}"#;

#[tokio::test]
async fn hints_persist_across_resync_and_support_crud() {
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

    let hints = store::entity_hints(&db);
    hints
        .upsert(EntityHintInput {
            application_id: app_id,
            entity_type: "use_case".into(),
            entity_key: "Checkout".into(),
            hint: "include the refund path".into(),
        })
        .await
        .unwrap();

    // Re-sync deletes and recreates sub-entities but keeps the application.
    let app_id2 = store::platform_writer(&db).write(repo.id, &result).await.unwrap();
    assert_eq!(app_id2, app_id, "same application id across re-sync");

    let listed = hints.list_for_application(app_id).await.unwrap();
    assert_eq!(listed.len(), 1, "hint survives re-sync");
    assert_eq!(listed[0].hint, "include the refund path");

    // Upsert replaces the existing hint (unique on app+type+key).
    hints
        .upsert(EntityHintInput {
            application_id: app_id,
            entity_type: "use_case".into(),
            entity_key: "Checkout".into(),
            hint: "and chargebacks".into(),
        })
        .await
        .unwrap();
    let listed = hints.list_for_application(app_id).await.unwrap();
    assert_eq!(listed.len(), 1, "upsert replaces, not duplicates");
    assert_eq!(listed[0].hint, "and chargebacks");

    // Delete removes it.
    hints.delete(app_id, "use_case", "Checkout").await.unwrap();
    assert!(hints.list_for_application(app_id).await.unwrap().is_empty());
}
