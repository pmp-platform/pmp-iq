//! Integration tests for RBAC (M37) on SQLite: admin-gated team management and
//! ownership-scoped application mutations.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::Request;
use common::{
    SqliteDb, build_state_sqlite, build_state_sqlite_multitenant, cookie_header, login_cookies,
};
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::app::build_router;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::rbac::{Role, TeamInput};
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{"application":{"name":"NAME","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
"languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],"platforms":[],
"external":[],"dependencies":[],"components":[],"use_cases":[],"users":[],"groups":[],"access":[]}"#;

async fn seed_app(db: &pmp_iq::db::Database, name: &str) -> Uuid {
    let account = store::accounts(db)
        .create(AccountInput {
            name: format!("a-{name}"),
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
    store::platform_writer(db)
        .write(repo.id, &AnalysisResult::parse(&ANALYSIS.replace("NAME", name)).unwrap())
        .await
        .unwrap()
}

async fn post(app: &Router, cookies: &[String], uri: &str, body: serde_json::Value) -> u16 {
    app.clone()
        .oneshot(
            Request::post(uri)
                .header(COOKIE, cookie_header(cookies))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .as_u16()
}

async fn get_json(app: &Router, cookies: &[String], uri: &str) -> serde_json::Value {
    let resp = app
        .clone()
        .oneshot(Request::get(uri).header(COOKIE, cookie_header(cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn admin_can_manage_teams_members_owners_and_roles() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let app_id = seed_app(&db, "shop").await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Create a team (empty roles table → bootstrap admin → allowed).
    let resp = app
        .clone()
        .oneshot(
            Request::post("/api/teams")
                .header(COOKIE, cookie_header(&cookies))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "name": "platform", "tenant_id": "acme" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
    let team: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let team_id = team["id"].as_str().unwrap().to_string();

    // List shows it.
    assert_eq!(get_json(&app, &cookies, "/api/teams").await["teams"].as_array().unwrap().len(), 1);

    // Add a member + assign application ownership.
    assert_eq!(post(&app, &cookies, &format!("/api/teams/{team_id}/members"), json!({ "principal": "alice" })).await, 200);
    assert_eq!(post(&app, &cookies, &format!("/api/teams/{team_id}/applications"), json!({ "application_id": app_id })).await, 200);

    // Assign a role + list roles.
    assert_eq!(post(&app, &cookies, "/api/roles", json!({ "principal": "alice", "role": "maintainer" })).await, 200);
    let roles = get_json(&app, &cookies, "/api/roles").await;
    assert!(roles["roles"].as_array().unwrap().iter().any(|r| r["principal"] == "alice" && r["role"] == "maintainer"));

    // Delete the team.
    let del = app
        .clone()
        .oneshot(
            Request::delete(format!("/api/teams/{team_id}"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del.status(), 200);
    assert!(get_json(&app, &cookies, "/api/teams").await["teams"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn viewer_is_denied_team_management() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    // Explicitly demote the admin principal to viewer (table assignment wins).
    store::roles(&db).set("admin", Role::Viewer).await.unwrap();
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    assert_eq!(post(&app, &cookies, "/api/teams", json!({ "name": "x" })).await, 403);
}

#[tokio::test]
async fn maintainer_can_sync_only_owned_apps() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let owned = seed_app(&db, "owned").await;
    let foreign = seed_app(&db, "foreign").await;

    // The admin principal becomes a maintainer who owns `owned` via a team.
    store::roles(&db).set("admin", Role::Maintainer).await.unwrap();
    let team = store::teams(&db).create(TeamInput { name: "team-a".into(), tenant_id: None }).await.unwrap();
    store::teams(&db).add_member(team.id, "admin").await.unwrap();
    store::teams(&db).set_owner(team.id, owned).await.unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Owned app: allowed (200, enqueues a sync).
    assert_eq!(post(&app, &cookies, &format!("/api/platform/applications/{owned}/sync"), json!({})).await, 200);
    // Foreign app: forbidden.
    assert_eq!(post(&app, &cookies, &format!("/api/platform/applications/{foreign}/sync"), json!({})).await, 401);
}

#[tokio::test]
async fn multitenant_scopes_application_visibility() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let mine = seed_app(&db, "mine").await;
    let theirs = seed_app(&db, "theirs").await;

    // The admin principal is demoted to viewer and joined to tenant "acme",
    // which owns only `mine`.
    store::roles(&db).set("admin", Role::Viewer).await.unwrap();
    let team = store::teams(&db).create(TeamInput { name: "acme-team".into(), tenant_id: Some("acme".into()) }).await.unwrap();
    store::teams(&db).add_member(team.id, "admin").await.unwrap();
    store::teams(&db).set_owner(team.id, mine).await.unwrap();

    let app = build_router(build_state_sqlite_multitenant(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // The applications list is scoped to the tenant's apps.
    let list = get_json(&app, &cookies, "/api/platform/applications").await;
    let ids: Vec<&str> = list["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&mine.to_string().as_str()));
    assert!(!ids.contains(&theirs.to_string().as_str()));

    // A foreign application's detail is hidden (404).
    let resp = app
        .clone()
        .oneshot(
            Request::get(format!("/api/platform/applications/{theirs}"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
