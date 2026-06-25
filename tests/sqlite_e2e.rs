//! End-to-end test on the SQLite engine (no container): seed the platform model
//! via the SQLite writer, then exercise the list, detail, and graph APIs served
//! by the SQLite query/graph implementations.

mod common;

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platform_inspector::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platform_inspector::app::build_router;
use platform_inspector::db::Database;
use platform_inspector::platform::AnalysisResult;
use platform_inspector::repositories::RepoRecordInput;
use platform_inspector::store;
use serde_json::Value;
use tower::ServiceExt;

async fn body_json(resp: Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn get(path: &str, cookies: &[String]) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(COOKIE, cookie_header(cookies))
        .body(Body::empty())
        .unwrap()
}

async fn seed(db: &Database, name: &str, json: &str) {
    let account = store::accounts(db)
        .create(AccountInput {
            name: format!("acc-{name}"),
            provider_type: ProviderType::Local,
            auth_type: AuthType::None,
            base_url: Some("/r".into()),
            credentials_enc: None,
            selection_mode: SelectionMode::All,
            selection_value: None,
            enabled: true,
        })
        .await
        .unwrap();
    let record = store::repo_records(db)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: name.into(),
            full_name: format!("org/{name}"),
            clone_url: format!("/r/{name}"),
            default_branch: None,
        })
        .await
        .unwrap();
    let result = AnalysisResult::parse(json).unwrap();
    store::platform_writer(db).write(record.id, &result).await.unwrap();
}

#[tokio::test]
async fn sqlite_platform_model_lists_details_and_graph() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();

    seed(
        &db,
        "billing",
        r#"{"application":{"name":"billing","app_type":"api","primary_language":"Rust"},
            "languages":[{"name":"Rust","percentage":100}],
            "libraries":[{"name":"axum","ecosystem":"cargo","version":"0.7","scope":"runtime"}],
            "infrastructure":[{"name":"PostgreSQL","kind":"database","version":"16","usage":"primary"}],
            "dependencies":[{"target_name":"shipping","kind":"http"},{"target_name":"auth","kind":"http"}],
            "users":[{"username":"alice","email":"a@x.com","groups":["devs"]}],
            "groups":[{"name":"devs"}],
            "access":[{"principal_type":"group","principal_name":"devs","access_level":"write"}]}"#,
    )
    .await;
    seed(&db, "shipping", r#"{"application":{"name":"shipping"}}"#).await;

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Applications list + search + pagination.
    let list = body_json(
        app.clone().oneshot(get("/api/platform/applications", &cookies)).await.unwrap(),
    )
    .await;
    assert_eq!(list["total"], 2);
    let billing_id = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "billing")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let search = body_json(
        app.clone().oneshot(get("/api/platform/applications?search=bill", &cookies)).await.unwrap(),
    )
    .await;
    assert_eq!(search["total"], 1);
    assert_eq!(search["items"][0]["name"], "billing");

    // Application detail with relations assembled in Rust.
    let detail = body_json(
        app.clone()
            .oneshot(get(&format!("/api/platform/applications/{billing_id}"), &cookies))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(detail["detail"]["name"], "billing");
    assert_eq!(detail["detail"]["languages"][0]["name"], "Rust");
    assert_eq!(detail["detail"]["libraries"][0]["name"], "axum");
    assert_eq!(detail["detail"]["infrastructure"][0]["name"], "PostgreSQL");
    assert_eq!(detail["detail"]["access"][0]["principal_name"], "devs");

    // Other entity lists.
    for (entity, total) in [("infrastructure", 1), ("libraries", 1), ("users", 1), ("groups", 1)] {
        let resp =
            body_json(app.clone().oneshot(get(&format!("/api/platform/{entity}"), &cookies)).await.unwrap()).await;
        assert_eq!(resp["total"], total, "entity {entity}");
    }

    // Graph: 2 apps + 1 infra + 1 external (the unresolved "auth" dep).
    let graph = body_json(
        app.clone().oneshot(get("/api/platform/graph", &cookies)).await.unwrap(),
    )
    .await;
    let kinds: Vec<&str> = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["data"]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds.iter().filter(|k| **k == "application").count(), 2);
    assert!(kinds.contains(&"infrastructure"));
    assert!(kinds.contains(&"external"));

    // A 404 for unknown entities still works on SQLite.
    let missing = app.oneshot(get("/api/platform/widgets", &cookies)).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}
