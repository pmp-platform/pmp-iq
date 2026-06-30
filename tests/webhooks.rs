//! Integration test for the GitHub webhook endpoint (M25): a default-branch push
//! enqueues a scoped sync; signature verification rejects bad signatures.

mod common;

use axum::body::Body;
use axum::http::Request;
use common::{SqliteDb, build_state_sqlite};
use hmac::{Hmac, Mac};
use http_body_util::BodyExt;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::app::{AppState, build_router};
use pmp_iq::auth::{Argon2Hasher, AuthService, RandomSecretGenerator};
use pmp_iq::config::{Config, MapEnv};
use pmp_iq::db::Database;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use sha2::Sha256;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

const PUSH_BODY: &str = r#"{"ref":"refs/heads/main","repository":{"default_branch":"main","full_name":"org/api"}}"#;

async fn seed_repo(db: &Database) {
    let account = store::accounts(db)
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
    store::repo_records(db)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: "api".into(),
            full_name: "org/api".into(),
            clone_url: "https://example.invalid/org/api.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
}

fn secret_state(db: Database, secret: &str) -> AppState {
    let workspace = std::env::temp_dir()
        .join(format!("pi-wh-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let env = MapEnv::new()
        .with("ADMIN_USER", "admin")
        .with("ADMIN_PASS", "admin")
        .with("WORKSPACE_DIR", &workspace)
        .with("WEBHOOK_GITHUB_SECRET", secret);
    let config = Config::load(&env).unwrap();
    let boot =
        AuthService::from_config(&config.auth, Arc::new(Argon2Hasher), &RandomSecretGenerator, None)
            .unwrap();
    AppState::build(config, db, Arc::new(boot.service), None).unwrap()
}

fn sign(secret: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[tokio::test]
async fn default_branch_push_enqueues_a_sync() {
    let sqlite = SqliteDb::start().await;
    seed_repo(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite)); // no secret → verification skipped

    let resp = app
        .clone()
        .oneshot(
            Request::post("/webhooks/github")
                .header("X-GitHub-Event", "push")
                .header("content-type", "application/json")
                .body(Body::from(PUSH_BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // A sync execution was enqueued by the push.
    let execs = store::job_executions(&sqlite.database()).list(10).await.unwrap();
    assert!(!execs.is_empty(), "push should enqueue a sync execution");
}

#[tokio::test]
async fn signature_is_verified_when_secret_set() {
    let sqlite = SqliteDb::start().await;
    seed_repo(&sqlite.database()).await;
    let app = build_router(secret_state(sqlite.database(), "topsecret"));

    // Wrong signature → 401.
    let bad = app
        .clone()
        .oneshot(
            Request::post("/webhooks/github")
                .header("X-GitHub-Event", "push")
                .header("X-Hub-Signature-256", "sha256=deadbeef")
                .body(Body::from(PUSH_BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), 401);

    // Correct signature → accepted.
    let good = app
        .clone()
        .oneshot(
            Request::post("/webhooks/github")
                .header("X-GitHub-Event", "push")
                .header("X-Hub-Signature-256", sign("topsecret", PUSH_BODY))
                .body(Body::from(PUSH_BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(good.status(), 204);
    let _ = good.into_body().collect().await;
}
