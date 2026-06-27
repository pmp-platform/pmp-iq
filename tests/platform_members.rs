//! Integration test for git-provider member reconciliation (SQLite, no
//! container): members are recorded with permissions, departures become
//! ex-members, returns are revived, and member status outranks codeowners.

mod common;
use common::SqliteDb;
use platform_inspector::store;

use platform_inspector::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platform_inspector::platform::{AnalysisResult, MemberInfo};
use platform_inspector::repositories::RepoRecordInput;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

/// Create an account + repo record + application from `analysis`, returning the
/// application id.
async fn setup_app(db: &SqliteDb, full_name: &str, analysis: &str) -> Uuid {
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
    let record = store::repo_records(&database)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: full_name.into(),
            full_name: full_name.into(),
            clone_url: format!("/repos/{full_name}"),
            default_branch: None,
        })
        .await
        .unwrap();
    let result = AnalysisResult::parse(analysis).unwrap();
    store::platform_writer(&database).write(record.id, &result).await.unwrap()
}

fn member(username: &str, role: &str) -> MemberInfo {
    MemberInfo {
        username: username.into(),
        email: None,
        role: Some(role.into()),
        permissions: json!({ "push": true, "admin": role == "admin" }),
        metadata: json!({}),
    }
}

/// The association_type recorded for `username` (None if no grant).
async fn assoc(pool: &SqlitePool, username: &str) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT ag.association_type FROM access_grants ag JOIN users u ON u.id=ag.principal_id \
         WHERE ag.principal_type='user' AND u.username=?1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    .unwrap();
    row.map(|(s,)| s)
}

#[tokio::test]
async fn reconciles_members_and_tracks_ex_members() {
    let db = SqliteDb::start().await;
    let app_id = setup_app(&db, "org/api", r#"{"application":{"name":"api"}}"#).await;
    let writer = store::platform_writer(&db.database());

    // First sync: alice + bob are members.
    writer
        .reconcile_members(app_id, &[member("alice", "admin"), member("bob", "write")])
        .await
        .unwrap();
    assert_eq!(assoc(&db.pool, "alice").await.as_deref(), Some("member"));
    assert_eq!(assoc(&db.pool, "bob").await.as_deref(), Some("member"));

    // Permissions + role are persisted on the grant.
    let (level, perms): (Option<String>, Value) = sqlx::query_as(
        "SELECT ag.access_level, ag.permissions FROM access_grants ag \
         JOIN users u ON u.id=ag.principal_id WHERE u.username='bob'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(level.as_deref(), Some("write"));
    assert_eq!(perms["push"], json!(true));

    // Second sync: bob is gone -> ex_member; alice stays a member.
    writer.reconcile_members(app_id, &[member("alice", "admin")]).await.unwrap();
    assert_eq!(assoc(&db.pool, "alice").await.as_deref(), Some("member"));
    assert_eq!(assoc(&db.pool, "bob").await.as_deref(), Some("ex_member"));

    // Third sync: bob returns -> member again.
    writer
        .reconcile_members(app_id, &[member("alice", "admin"), member("bob", "read")])
        .await
        .unwrap();
    assert_eq!(assoc(&db.pool, "bob").await.as_deref(), Some("member"));

    // Empty sync: everyone becomes an ex-member.
    writer.reconcile_members(app_id, &[]).await.unwrap();
    assert_eq!(assoc(&db.pool, "alice").await.as_deref(), Some("ex_member"));
    assert_eq!(assoc(&db.pool, "bob").await.as_deref(), Some("ex_member"));
}

#[tokio::test]
async fn members_outrank_codeowners_and_survive_reanalysis() {
    let db = SqliteDb::start().await;
    let analysis = r#"{"application":{"name":"api"},
        "users":[{"username":"alice"}],
        "access":[{"principal_type":"user","principal_name":"alice","access_level":"owner"}]}"#;
    let app_id = setup_app(&db, "org/api", analysis).await;
    let writer = store::platform_writer(&db.database());

    // AI-extracted CODEOWNERS access is recorded as a codeowner.
    assert_eq!(assoc(&db.pool, "alice").await.as_deref(), Some("codeowner"));

    // A real provider member upgrades the same principal to member.
    writer.reconcile_members(app_id, &[member("alice", "admin")]).await.unwrap();
    assert_eq!(assoc(&db.pool, "alice").await.as_deref(), Some("member"));

    // Re-running analysis refreshes codeowner grants but must not clobber the
    // member status, and must not create a duplicate grant.
    let result = AnalysisResult::parse(analysis).unwrap();
    rewrite_same_repo(&db, app_id, &result).await;
    assert_eq!(assoc(&db.pool, "alice").await.as_deref(), Some("member"));
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM access_grants ag JOIN users u ON u.id=ag.principal_id WHERE u.username='alice'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "no duplicate grant for alice");
}

/// Re-run the writer for the application's existing repository.
async fn rewrite_same_repo(db: &SqliteDb, app_id: Uuid, result: &AnalysisResult) {
    let (repo_id,): (Uuid,) =
        sqlx::query_as("SELECT repository_id FROM applications WHERE id=?1")
            .bind(app_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    store::platform_writer(&db.database()).write(repo_id, result).await.unwrap();
}
