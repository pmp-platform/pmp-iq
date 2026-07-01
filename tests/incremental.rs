//! Integration test for incremental analysis (M41) on SQLite: a partial write
//! upserts only the supplied components/use cases and leaves untouched siblings
//! intact, and `mark_analyzed` advances the repository's last-analyzed commit.

mod common;

use common::SqliteDb;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use uuid::Uuid;

fn full() -> &'static str {
    r#"{"application":{"name":"svc","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
    "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],
    "platforms":[],"external":[],"dependencies":[],
    "components":[{"name":"A","kind":"controller","description":"old A"},{"name":"B","kind":"model","description":"B"}],
    "use_cases":[],"users":[],"groups":[],"access":[]}"#
}

// A partial result re-extracting only component A (changed) — B is omitted.
fn partial() -> &'static str {
    r#"{"application":{"name":"svc","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
    "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],
    "platforms":[],"external":[],"dependencies":[],
    "components":[{"name":"A","kind":"controller","description":"new A"}],
    "use_cases":[],"users":[],"groups":[],"access":[]}"#
}

#[tokio::test]
async fn partial_write_merges_and_preserves_untouched() {
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
            name: "svc".into(),
            full_name: "org/svc".into(),
            clone_url: "https://x.invalid/svc.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    let writer = store::platform_writer(&db);

    // Full write: components A (old A) + B.
    let app_id = writer.write(repo.id, &AnalysisResult::parse(full()).unwrap()).await.unwrap();

    // Partial write: only A (new A). B must survive untouched.
    writer.write_partial(app_id, &AnalysisResult::parse(partial()).unwrap()).await.unwrap();

    let detail = store::platform_query(&db).detail("applications", app_id).await.unwrap();
    let components = detail["components"].as_array().unwrap();
    let by_name = |name: &str| components.iter().find(|c| c["name"] == name).cloned();
    // A was updated in place; B is preserved.
    assert_eq!(by_name("A").unwrap()["description"], "new A");
    let b = by_name("B").expect("untouched component B preserved");
    assert_eq!(b["description"], "B");
    assert_eq!(components.len(), 2);

    // mark_analyzed advances the repo's last-analyzed commit.
    store::repo_records(&db).mark_analyzed(repo.id, "abc123").await.unwrap();
    let reloaded = store::repo_records(&db).get(repo.id).await.unwrap();
    assert_eq!(reloaded.last_analyzed_sha.as_deref(), Some("abc123"));
}

#[tokio::test]
async fn first_analysis_records_no_prior_sha() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let account = store::accounts(&db)
        .create(AccountInput {
            name: "a".into(),
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
            name: "new".into(),
            full_name: "org/new".into(),
            clone_url: "https://x.invalid/new.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    // A freshly-upserted repository has no prior analyzed commit.
    assert_eq!(repo.last_analyzed_sha, None);
    let _ = Uuid::new_v4();
}
