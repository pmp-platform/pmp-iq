//! Integration test for the repository-account data layer against PostgreSQL.

mod common;
use platiq::store;

use common::TestDb;
use platiq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};

fn input(name: &str) -> AccountInput {
    AccountInput {
        name: name.into(),
        provider_type: ProviderType::Github,
        auth_type: AuthType::Token,
        base_url: None,
        credentials_enc: Some(vec![1, 2, 3, 4]),
        selection_mode: SelectionMode::Regex,
        selection_value: Some("^org/".into()),
        enabled: true,
    }
}

#[tokio::test]
async fn crud_round_trip() {
    let db = TestDb::start().await;
    let repo = store::accounts(&db.database());

    let created = repo.create(input("gh")).await.unwrap();
    assert_eq!(created.name, "gh");
    assert_eq!(created.provider_type, ProviderType::Github);
    assert_eq!(created.credentials_enc, Some(vec![1, 2, 3, 4]));

    let fetched = repo.get(created.id).await.unwrap();
    assert_eq!(fetched.id, created.id);

    let mut upd = input("gh-renamed");
    upd.enabled = false;
    let updated = repo.update(created.id, upd).await.unwrap();
    assert_eq!(updated.name, "gh-renamed");
    assert!(!updated.enabled);

    // list_enabled excludes the disabled account.
    repo.create(input("enabled-one")).await.unwrap();
    let enabled = repo.list_enabled().await.unwrap();
    assert!(enabled.iter().all(|a| a.enabled));
    assert!(enabled.iter().any(|a| a.name == "enabled-one"));

    repo.delete(created.id).await.unwrap();
    assert!(repo.get(created.id).await.is_err());
}

#[tokio::test]
async fn delete_missing_is_not_found() {
    let db = TestDb::start().await;
    let repo = store::accounts(&db.database());
    let result = repo.delete(uuid::Uuid::new_v4()).await;
    assert!(result.is_err());
}
