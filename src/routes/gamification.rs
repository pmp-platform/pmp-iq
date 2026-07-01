//! Gamification routes (M44): the logged-in operator's profile, the leaderboard,
//! the leaderboard page, and an admin-triggered replay.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::web::{PageContext, render_page};
use axum::extract::State;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use minijinja::context;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/platform/leaderboard", get(leaderboard_page))
        .route("/api/gamification/me", get(my_profile))
        .route("/api/gamification/leaderboard", get(leaderboard))
        .route("/api/gamification/replay", post(replay))
}

async fn my_profile(State(state): State<AppState>, Extension(user): Extension<Principal>) -> AppResult<Json<Value>> {
    Ok(Json(json!(state.gamification.profile(&user.username).await?)))
}

async fn leaderboard(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "leaderboard": state.gamification.leaderboard().await? })))
}

/// Admin-triggered replay of recorded actions into XP/badges.
async fn replay(State(state): State<AppState>, Extension(user): Extension<Principal>) -> AppResult<Json<Value>> {
    if !user.has_role("admin") {
        return Err(AppError::Unauthorized);
    }
    let awarded = state.gamification.replay().await?;
    Ok(Json(json!({ "awarded": awarded })))
}

async fn leaderboard_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(&state.engine, "leaderboard.html", &page, context! { active_tab => "leaderboard" })
}
