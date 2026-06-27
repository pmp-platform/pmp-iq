//! Platform section: overview, entity tables, detail pages, and read API.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::platform::{GraphScope, ListQuery, filter_fields, is_entity};
use crate::web::{PageContext, render_page};
use axum::Extension;
use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::{Html, Redirect};
use axum::routing::get;
use minijinja::context;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/platform", get(overview_page))
        .route("/platform/graph", get(graph_page))
        .route("/api/platform/graph", get(graph_api))
        .route("/platform/:entity", get(list_page))
        .route("/platform/:entity/:id", get(detail_page))
        .route("/api/platform/:entity", get(list_api))
        .route("/api/platform/:entity/facets", get(facets_api))
        .route("/api/platform/:entity/:id", get(detail_api))
}

/// Build a list query from raw query params: `search`/`page`/`page_size` plus
/// any allowlisted equality filters for the entity.
fn build_list_query(entity: &str, params: &HashMap<String, String>) -> ListQuery {
    let search = params.get("search").cloned();
    let page = params.get("page").and_then(|v| v.parse().ok());
    let page_size = params.get("page_size").and_then(|v| v.parse().ok());
    let mut filters = BTreeMap::new();
    for &field in filter_fields(entity) {
        if let Some(value) = params.get(field).filter(|v| !v.is_empty()) {
            filters.insert(field.to_string(), value.clone());
        }
    }
    ListQuery::new(search, page, page_size, filters)
}

fn validate_entity(entity: &str) -> AppResult<()> {
    if is_entity(entity) {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("unknown entity '{entity}'")))
    }
}

/// Entering the Platform section lands on the Graph tab by default.
async fn overview_page() -> Redirect {
    Redirect::to("/platform/graph")
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
    render_page(&state.engine, "platform_graph.html", &page, context! { active_tab => "graph" })
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
        context! { entity => entity, active_tab => entity },
    )
}

async fn detail_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path((entity, id)): Path<(String, Uuid)>,
) -> AppResult<Html<String>> {
    validate_entity(&entity)?;
    let page = PageContext::new(Some(user.display_name), "platform");
    let template = if entity == "applications" {
        "platform_app_detail.html"
    } else {
        "platform_detail.html"
    };
    render_page(
        &state.engine,
        template,
        &page,
        context! { entity => entity, entity_id => id.to_string(), active_tab => entity },
    )
}

async fn list_api(
    State(state): State<AppState>,
    Path(entity): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<Json<Value>> {
    validate_entity(&entity)?;
    let query = build_list_query(&entity, &params);
    let page = state.platform.list(&entity, &query).await?;
    Ok(Json(json!({
        "items": page.items,
        "total": page.total,
        "page": page.page,
        "page_size": page.page_size,
    })))
}

/// Distinct values for each filterable field of an entity (filter dropdowns).
async fn facets_api(
    State(state): State<AppState>,
    Path(entity): Path<String>,
) -> AppResult<Json<Value>> {
    validate_entity(&entity)?;
    let facets = state.platform.facets(&entity).await?;
    Ok(Json(facets))
}

async fn detail_api(
    State(state): State<AppState>,
    Path((entity, id)): Path<(String, Uuid)>,
) -> AppResult<Json<Value>> {
    validate_entity(&entity)?;
    let detail = state.platform.detail(&entity, id).await?;
    Ok(Json(json!({ "detail": detail })))
}
