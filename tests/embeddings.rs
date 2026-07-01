//! Integration tests for semantic search & duplicate detection (M40) on SQLite:
//! the embedding repository (store + nearest), and the search/similar/
//! duplicates routes (with a configured model + directly-inserted vectors).

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::header::COOKIE;
use common::{
    SqliteDb, build_state_sqlite, build_state_sqlite_with_embeddings, cookie_header, login_cookies,
};
use http_body_util::BodyExt;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::app::build_router;
use pmp_iq::embeddings::EntityEmbedding;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

const MODEL: &str = "test-embed";

fn analysis(name: &str) -> String {
    format!(
        r#"{{"application":{{"name":"{name}","app_type":"api","description":"d","primary_language":"Rust","metadata":{{}}}},
        "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
        "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
        "use_cases":[],"users":[],"groups":[],"access":[]}}"#
    )
}

/// Seed an analysed application and return its id.
async fn seed_app(db: &pmp_iq::db::Database, name: &str) -> Uuid {
    let account = store::accounts(db)
        .create(AccountInput {
            name: format!("acc-{name}"),
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
    let repo = store::repo_records(db)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: name.into(),
            full_name: format!("org/{name}"),
            clone_url: format!("https://x.invalid/{name}.git"),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    store::platform_writer(db).write(repo.id, &AnalysisResult::parse(&analysis(name)).unwrap()).await.unwrap();
    // The application id == the row in applications; fetch via the list query.
    let page = store::platform_query(db)
        .list("applications", &pmp_iq::platform::ListQuery::new(Some(name.into()), None, None, Default::default()))
        .await
        .unwrap();
    Uuid::parse_str(page.items[0]["id"].as_str().unwrap()).unwrap()
}

async fn insert_vec(db: &pmp_iq::db::Database, id: Uuid, vector: Vec<f32>) {
    store::embeddings(db)
        .upsert(MODEL, &EntityEmbedding { entity_type: "application".into(), entity_id: id, vector, summary_hash: "h".into() })
        .await
        .unwrap();
}

async fn get(app: &Router, cookies: &[String], uri: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(Request::get(uri).header(COOKIE, cookie_header(cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "GET {uri}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn repository_nearest_orders_by_cosine() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let repo = store::embeddings(&db);
    let near = Uuid::new_v4();
    let far = Uuid::new_v4();
    repo.upsert(MODEL, &EntityEmbedding { entity_type: "application".into(), entity_id: near, vector: vec![1.0, 0.1], summary_hash: "a".into() }).await.unwrap();
    repo.upsert(MODEL, &EntityEmbedding { entity_type: "application".into(), entity_id: far, vector: vec![0.0, 1.0], summary_hash: "b".into() }).await.unwrap();

    let neighbours = repo.nearest(MODEL, vec![1.0, 0.0], Some("application".into()), 10).await.unwrap();
    assert_eq!(neighbours[0].entity_id, near);
    assert_eq!(neighbours[1].entity_id, far);

    // hashes() round-trips the stored summary hashes.
    let hashes = repo.hashes(MODEL).await.unwrap();
    assert_eq!(hashes.get(&("application".to_string(), near)), Some(&"a".to_string()));
}

#[tokio::test]
async fn search_falls_back_to_substring_without_embeddings() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    seed_app(&db, "billing-service").await;
    seed_app(&db, "auth-gateway").await;

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let body = get(&app, &cookies, "/api/platform/search?q=billing").await;
    assert_eq!(body["mode"], "substring");
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "billing-service");
}

#[tokio::test]
async fn similar_and_duplicates_disabled_without_provider() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let a = seed_app(&db, "solo").await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let sim = get(&app, &cookies, &format!("/api/platform/applications/{a}/similar")).await;
    assert_eq!(sim["enabled"], false);
    let dup = get(&app, &cookies, "/api/platform/duplicates").await;
    assert_eq!(dup["enabled"], false);
}

#[tokio::test]
async fn similar_and_duplicates_use_stored_vectors() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let a = seed_app(&db, "mailer-one").await;
    let b = seed_app(&db, "mailer-two").await;
    let c = seed_app(&db, "ledger").await;
    // a & b near-identical; c orthogonal.
    insert_vec(&db, a, vec![1.0, 0.0]).await;
    insert_vec(&db, b, vec![0.98, 0.02]).await;
    insert_vec(&db, c, vec![0.0, 1.0]).await;

    let app = build_router(build_state_sqlite_with_embeddings(&sqlite, "http://127.0.0.1:0/e", MODEL));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // similar(a) ranks b above c.
    let sim = get(&app, &cookies, &format!("/api/platform/applications/{a}/similar")).await;
    assert_eq!(sim["enabled"], true);
    let results = sim["results"].as_array().unwrap();
    assert_eq!(results[0]["entity_id"], b.to_string());

    // duplicates clusters a & b (threshold below their similarity, above c's).
    let dup = get(&app, &cookies, "/api/platform/duplicates?threshold=0.9").await;
    assert_eq!(dup["enabled"], true);
    let clusters = dup["clusters"].as_array().unwrap();
    assert_eq!(clusters.len(), 1);
    let members = clusters[0]["members"].as_array().unwrap();
    let ids: Vec<&str> = members.iter().map(|m| m["entity_id"].as_str().unwrap()).collect();
    assert!(ids.contains(&a.to_string().as_str()) && ids.contains(&b.to_string().as_str()));
    assert!(!ids.contains(&c.to_string().as_str()));
}
