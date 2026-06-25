//! Platform section: overview, entity tables, detail pages, and read API.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::platform::{GraphScope, ListQuery, is_entity};
use crate::web::{PageContext, render_page};
use axum::Extension;
use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::Html;
use axum::routing::get;
use minijinja::context;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/platform", get(overview_page))
        .route("/platform/graph", get(graph_page))
        .route("/api/platform/graph", get(graph_api))
        .route("/platform/:entity", get(list_page))
        .route("/platform/:entity/:id", get(detail_page))
        .route("/api/platform/:entity", get(list_api))
        .route("/api/platform/:entity/:id", get(detail_api))
}

#[derive(Deserialize)]
struct ListParams {
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
}

fn validate_entity(entity: &str) -> AppResult<()> {
    if is_entity(entity) {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("unknown entity '{entity}'")))
    }
}

async fn overview_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(&state.engine, "platform.html", &page, context! {})
}

#[derive(Deserialize)]
struct GraphParams {
    #[serde(default)]
    center: Option<Uuid>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn graph_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(&state.engine, "platform_graph.html", &page, context! {})
}

async fn graph_api(
    State(state): State<AppState>,
    Query(params): Query<GraphParams>,
) -> AppResult<Json<Value>> {
    let scope = GraphScope::new(params.center, params.limit);
    let graph = state.graph.build(&scope).await?;
    Ok(Json(graph))
}

async fn list_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(entity): Path<String>,
) -> AppResult<Html<String>> {
    validate_entity(&entity)?;
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(
        &state.engine,
        "platform_list.html",
        &page,
        context! { entity => entity },
    )
}

async fn detail_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path((entity, id)): Path<(String, Uuid)>,
) -> AppResult<Html<String>> {
    validate_entity(&entity)?;
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(
        &state.engine,
        "platform_detail.html",
        &page,
        context! { entity => entity, entity_id => id.to_string() },
    )
}

async fn list_api(
    State(state): State<AppState>,
    Path(entity): Path<String>,
    Query(params): Query<ListParams>,
) -> AppResult<Json<Value>> {
    validate_entity(&entity)?;
    let query = ListQuery::new(params.search, params.page, params.page_size);
    let page = state.platform.list(&entity, &query).await?;
    Ok(Json(json!({
        "items": page.items,
        "total": page.total,
        "page": page.page,
        "page_size": page.page_size,
    })))
}

async fn detail_api(
    State(state): State<AppState>,
    Path((entity, id)): Path<(String, Uuid)>,
) -> AppResult<Json<Value>> {
    validate_entity(&entity)?;
    let detail = state.platform.detail(&entity, id).await?;
    Ok(Json(json!({ "detail": detail })))
}
