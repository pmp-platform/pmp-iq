//! End-to-end happy path: log in, configure a repository account and an AI
//! profile (mocked), run the review job, and explore the populated platform via
//! the table and graph APIs.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platform_inspector::app::build_router;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tower::ServiceExt;

const ANALYSIS_JSON: &str = r#"{"application":{"name":"service-a","app_type":"api","primary_language":"Rust"},
"languages":[{"name":"Rust","percentage":100}],
"libraries":[{"name":"axum","ecosystem":"cargo","version":"0.7","scope":"runtime"}],
"infrastructure":[{"name":"PostgreSQL","kind":"database","version":"16","usage":"primary"}],
"dependencies":[{"target_name":"auth","kind":"http"}],
"users":[{"username":"alice","email":"alice@x.com","groups":["devs"]}],
"groups":[{"name":"devs"}],
"access":[{"principal_type":"group","principal_name":"devs","access_level":"write"}]}"#;

async fn body_json(resp: Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn authed(method: Method, path: &str, cookies: &[String], body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(COOKIE, cookie_header(cookies));
    let body = match body {
        Some(v) => {
            builder = builder.header(CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).unwrap()
}

fn init_repo(path: &Path) {
    let repo = git2::Repository::init(path).unwrap();
    std::fs::write(path.join("Cargo.toml"), "[package]\nname=\"service-a\"\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("Cargo.toml")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("T", "t@e.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
}

async fn start_mock_anthropic() -> String {
    async fn messages() -> Json<Value> {
        Json(json!({
            "content": [{ "type": "text", "text": ANALYSIS_JSON }],
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        }))
    }
    let app = Router::new().route("/v1/messages", post(messages));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{}", addr.port())
}

#[tokio::test]
async fn full_platform_inspection_flow() {
    let base_url = start_mock_anthropic().await;
    let src = std::env::temp_dir().join(format!("pi-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(src.join("service-a")).unwrap();
    init_repo(&src.join("service-a"));

    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    // 1. Unauthenticated access is denied.
    let denied = app
        .clone()
        .oneshot(Request::get("/api/platform/applications").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    // 2. Log in.
    let cookies = login_cookies(&app, "admin", "admin").await;

    // 3. Configure an AI profile + local account.
    let profile = json!({
        "name": "ai", "provider_type": "anthropic", "api_key": "k",
        "config": { "model": "claude-opus-4-8", "base_url": base_url }
    });
    let profile_id = body_json(
        app.clone()
            .oneshot(authed(Method::POST, "/api/settings/ai-profiles", &cookies, Some(profile)))
            .await
            .unwrap(),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let account = json!({
        "name": "local", "provider_type": "local", "auth_type": "none",
        "base_url": src.to_string_lossy(), "selection_mode": "all"
    });
    app.clone()
        .oneshot(authed(Method::POST, "/api/settings/accounts", &cookies, Some(account)))
        .await
        .unwrap();

    // 4. Create and run the review job.
    let job = json!({
        "name": "review", "job_type": "review-repositories", "trigger_type": "manual",
        "config": { "ai_profile_id": profile_id }
    });
    let job_id = body_json(
        app.clone()
            .oneshot(authed(Method::POST, "/api/jobs", &cookies, Some(job)))
            .await
            .unwrap(),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let execution_id = body_json(
        app.clone()
            .oneshot(authed(Method::POST, &format!("/api/jobs/{job_id}/run"), &cookies, None))
            .await
            .unwrap(),
    )
    .await["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 5. Wait for completion.
    for _ in 0..80 {
        let exec = body_json(
            app.clone()
                .oneshot(authed(
                    Method::GET,
                    &format!("/api/jobs/executions/{execution_id}"),
                    &cookies,
                    None,
                ))
                .await
                .unwrap(),
        )
        .await;
        if exec["execution"]["status"] == "succeeded" {
            break;
        }
        assert_ne!(exec["execution"]["status"], "failed", "logs: {}", exec["execution"]["logs"]);
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    // 6. Explore the platform: applications list shows the analysed app.
    let apps = body_json(
        app.clone()
            .oneshot(authed(Method::GET, "/api/platform/applications", &cookies, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(apps["total"], 1);
    assert_eq!(apps["items"][0]["name"], "service-a");

    // 7. The graph has the application, its infrastructure, and an external dep.
    let graph = body_json(
        app.clone()
            .oneshot(authed(Method::GET, "/api/platform/graph", &cookies, None))
            .await
            .unwrap(),
    )
    .await;
    let kinds: Vec<&str> = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["data"]["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"application"));
    assert!(kinds.contains(&"infrastructure"));
    assert!(kinds.contains(&"external")); // the unresolved "auth" dependency

    std::fs::remove_dir_all(&src).ok();
}
