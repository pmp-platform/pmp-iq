//! Health/readiness endpoint.

use crate::app::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new().route("/healthz", get(healthz))
}

/// Reports process liveness and database connectivity.
async fn healthz(State(state): State<AppState>) -> Json<Value> {
    let db_ok = state.db.ping().await.is_ok();
    Json(json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "database": db_ok,
    }))
}
