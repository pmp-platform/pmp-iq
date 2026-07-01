//! Integration test for operator gamification (M44) on SQLite: a recorded action
//! (team creation) is replayed into XP + a badge, surfaced on the profile and
//! leaderboard.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::app::build_router;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn send(app: &Router, cookies: &[String], method: &str, uri: &str, body: Value) -> (u16, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(COOKIE, cookie_header(cookies))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let s = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (s, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
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
async fn actions_replay_into_xp_skills_and_badges() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // A recorded mutating action (team.create audited for "admin").
    assert_eq!(send(&app, &cookies, "POST", "/api/teams", json!({ "name": "platform" })).await.0, 200);

    // Replay awards the action (idempotent — replaying again awards nothing new).
    let (rs, replayed) = send(&app, &cookies, "POST", "/api/gamification/replay", json!({})).await;
    assert_eq!(rs, 200);
    assert!(replayed["awarded"].as_u64().unwrap() >= 1);
    let (_, again) = send(&app, &cookies, "POST", "/api/gamification/replay", json!({})).await;
    assert_eq!(again["awarded"], 0, "replay is idempotent");

    // The profile shows XP, a skill and the first-action badge.
    let me = get(&app, &cookies, "/api/gamification/me").await;
    assert!(me["total_xp"].as_i64().unwrap() >= 10, "{me}");
    assert_eq!(me["level"]["level"], 1);
    assert!(me["skills"].as_array().unwrap().iter().any(|s| s["skill"] == "platform"));
    assert!(me["badges"].as_array().unwrap().iter().any(|b| b == "first_action"));

    // The leaderboard ranks the admin.
    let board = get(&app, &cookies, "/api/gamification/leaderboard").await;
    let top = board["leaderboard"].as_array().unwrap();
    assert!(top.iter().any(|r| r["actor"] == "admin" && r["points"].as_i64().unwrap() >= 10));

    // The leaderboard page renders.
    let page = app
        .clone()
        .oneshot(Request::get("/platform/leaderboard").header(COOKIE, cookie_header(&cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
}
