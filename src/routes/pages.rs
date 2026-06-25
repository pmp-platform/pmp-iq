//! Server-rendered application shell pages (authenticated).

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::AppResult;
use crate::web::{PageContext, render_page};
use axum::Extension;
use axum::Router;
use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use minijinja::context;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(home))
}

async fn home(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "home");
    render_page(&state.engine, "home.html", &page, context! {})
}

