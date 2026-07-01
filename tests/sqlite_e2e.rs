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
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::analysis_config::AnalysisConfigService;
use pmp_iq::app::build_router;
use pmp_iq::db::Database;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
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
            organization: None,
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
            "libraries":[{"name":"axum","ecosystem":"cargo","version":"0.7","scope":"runtime","metadata":{"license":"MIT"}}],
            "infrastructure":[{"name":"PostgreSQL","kind":"database","version":"16","usage":"primary"}],
            "tools":[{"name":"docker compose","kind":"orchestration","version":"2","metadata":{"file":"docker-compose.yml"}}],
            "cloud_providers":[{"name":"AWS","kind":"cloud"}],
            "services":[{"name":"Stripe","kind":"payments"}],
            "platforms":[{"name":"Datadog","kind":"observability"}],
            "external":[{"name":"SomeAPI","kind":"misc"}],
            "dependencies":[{"target_name":"shipping","kind":"http","component":"InvoiceController"},{"target_name":"auth","kind":"http"}],
            "users":[{"username":"alice","email":"a@x.com","groups":["devs"],"metadata":{"role":"lead"}}],
            "groups":[{"name":"devs","metadata":{"description":"engineers"}}],
            "access":[{"principal_type":"group","principal_name":"devs","access_level":"write"}],
            "components":[{"name":"InvoiceController","kind":"controller","observability_signals":[{"name":"invoices_total","kind":"metric"}]}],
            "use_cases":[{"name":"Issue invoice","description":"bill a customer","components":["InvoiceController"],"diagrams":[{"name":"Flow","kind":"flowchart","content":"flowchart TD; A-->B"}]}]}"#,
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

    // Server-side filter on an allowlisted field (only billing is app_type=api).
    let filtered = body_json(
        app.clone().oneshot(get("/api/platform/applications?app_type=api", &cookies)).await.unwrap(),
    )
    .await;
    assert_eq!(filtered["total"], 1);
    assert_eq!(filtered["items"][0]["name"], "billing");

    // Facets list distinct values for the filterable fields.
    let facets = body_json(
        app.clone().oneshot(get("/api/platform/applications/facets", &cookies)).await.unwrap(),
    )
    .await;
    assert_eq!(facets["app_type"], serde_json::json!(["api"]));
    assert_eq!(facets["primary_language"], serde_json::json!(["Rust"]));

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
    assert_eq!(detail["detail"]["tools"][0]["name"], "docker compose");
    assert_eq!(detail["detail"]["cloud-providers"][0]["name"], "AWS");
    assert_eq!(detail["detail"]["access"][0]["association_type"], "codeowner");
    assert_eq!(detail["detail"]["components"][0]["name"], "InvoiceController");
    assert_eq!(detail["detail"]["components"][0]["observability_signals"][0]["name"], "invoices_total");
    assert_eq!(detail["detail"]["use_cases"][0]["name"], "Issue invoice");
    assert_eq!(detail["detail"]["use_cases"][0]["components"][0]["name"], "InvoiceController");
    assert_eq!(detail["detail"]["use_cases"][0]["diagrams"][0]["content"], "flowchart TD; A-->B");
    assert_eq!(detail["detail"]["services"][0]["name"], "Stripe");
    assert_eq!(detail["detail"]["access"][0]["principal_name"], "devs");
    // Relation rows carry ids so the detail page can link onward.
    assert!(detail["detail"]["libraries"][0]["id"].is_string());
    assert!(detail["detail"]["tools"][0]["id"].is_string());
    // Extracted metadata round-trips for libraries (a newly metadata-bearing entity).
    assert_eq!(detail["detail"]["libraries"][0]["metadata"]["license"], "MIT");
    // The "shipping" dependency resolves to the existing application.
    let deps = detail["detail"]["dependencies"].as_array().unwrap();
    let shipping = deps.iter().find(|d| d["target_name"] == "shipping").unwrap();
    assert!(shipping["target_app_id"].is_string(), "shipping dep should link to the app");
    assert_eq!(shipping["component_name"], "InvoiceController", "dependency mapped to component");

    // Other entity lists, including the new linked entities.
    for (entity, total) in [
        ("infrastructure", 1),
        ("tools", 1),
        ("cloud-providers", 1),
        ("services", 1),
        ("platforms", 1),
        ("external", 1),
        ("libraries", 1),
        ("users", 1),
        ("groups", 1),
    ] {
        let resp =
            body_json(app.clone().oneshot(get(&format!("/api/platform/{entity}"), &cookies)).await.unwrap()).await;
        assert_eq!(resp["total"], total, "entity {entity}");
    }

    // A new linked entity detail lists the applications using it.
    let tools = body_json(app.clone().oneshot(get("/api/platform/tools", &cookies)).await.unwrap()).await;
    let tool_id = tools["items"][0]["id"].as_str().unwrap().to_string();
    let tool = body_json(
        app.clone().oneshot(get(&format!("/api/platform/tools/{tool_id}"), &cookies)).await.unwrap(),
    )
    .await;
    assert_eq!(tool["detail"]["name"], "docker compose");
    assert_eq!(tool["detail"]["applications"][0]["name"], "billing");

    // Users/groups metadata (newly metadata-bearing entities) round-trips.
    let users = body_json(app.clone().oneshot(get("/api/platform/users", &cookies)).await.unwrap()).await;
    assert_eq!(users["items"][0]["metadata"]["role"], "lead");
    let groups = body_json(app.clone().oneshot(get("/api/platform/groups", &cookies)).await.unwrap()).await;
    assert_eq!(groups["items"][0]["metadata"]["description"], "engineers");

    // Migration 009 seeded the analysis-config tables, served via the API.
    let seeded_kinds = body_json(app.clone().oneshot(get("/api/settings/entity-kinds", &cookies)).await.unwrap()).await;
    assert!(!seeded_kinds["kinds"].as_array().unwrap().is_empty(), "kinds seeded");
    let seeded_props = body_json(app.clone().oneshot(get("/api/settings/entity-properties", &cookies)).await.unwrap()).await;
    assert!(!seeded_props["properties"].as_array().unwrap().is_empty(), "properties seeded");

    // Seeded config is strict: an entity with an unlisted kind is dropped on import.
    let cfg = AnalysisConfigService::new(
        store::entity_kinds(&db),
        store::entity_properties(&db),
        store::extraction_prompts(&db),
    )
    .load()
    .await
    .unwrap();
    let mut bogus = AnalysisResult::parse(
        r#"{"application":{"name":"x"},"services":[{"name":"Mystery","kind":"totally-made-up"},{"name":"Pay","kind":"payments"}]}"#,
    )
    .unwrap();
    bogus.apply_config(&cfg);
    assert_eq!(bogus.services.len(), 1, "unlisted-kind service dropped");
    assert_eq!(bogus.services[0].kind, "payments");

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
    assert!(kinds.contains(&"tools"));
    assert!(kinds.contains(&"cloud-providers"));
    assert!(kinds.contains(&"services"));
    assert!(kinds.contains(&"external"));
    // Nodes that point at an entity carry a navigation href.
    let tool_node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["data"]["kind"] == "tools")
        .unwrap();
    assert!(
        tool_node["data"]["href"].as_str().unwrap().starts_with("/platform/tools/"),
        "tool node should link to its detail page"
    );

    // A 404 for unknown entities still works on SQLite.
    let missing = app.oneshot(get("/api/platform/widgets", &cookies)).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}
