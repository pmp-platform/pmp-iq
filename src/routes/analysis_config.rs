//! Settings endpoints for the analysis vocabulary: allowed entity kinds and
//! extraction properties.

use crate::analysis_config::model::{DataType, EntityKindInput, EntityPropertyInput};
use crate::app::AppState;
use crate::error::AppResult;
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/settings/entity-kinds",
            get(list_kinds).post(create_kind),
        )
        .route(
            "/api/settings/entity-kinds/:id",
            put(update_kind).delete(delete_kind),
        )
        .route(
            "/api/settings/entity-properties",
            get(list_properties).post(create_property),
        )
        .route(
            "/api/settings/entity-properties/:id",
            put(update_property).delete(delete_property),
        )
}

#[derive(Serialize)]
struct KindView {
    id: Uuid,
    entity_type: String,
    kind_id: String,
    name: String,
    description: String,
    config: Value,
}

fn empty_object() -> Value {
    json!({})
}

#[derive(Serialize)]
struct PropertyView {
    id: Uuid,
    entity_type: String,
    prop_id: String,
    name: String,
    description: String,
    data_type: DataType,
}

#[derive(Deserialize)]
struct KindPayload {
    entity_type: String,
    kind_id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "empty_object")]
    config: Value,
}

#[derive(Deserialize)]
struct PropertyPayload {
    entity_type: String,
    prop_id: String,
    name: String,
    #[serde(default)]
    description: String,
    data_type: DataType,
}

fn kind_input(p: KindPayload) -> EntityKindInput {
    EntityKindInput {
        entity_type: p.entity_type,
        kind_id: p.kind_id,
        name: p.name,
        description: p.description,
        config: p.config,
    }
}

fn kind_view(k: crate::analysis_config::model::EntityKind) -> KindView {
    KindView {
        id: k.id,
        entity_type: k.entity_type,
        kind_id: k.kind_id,
        name: k.name,
        description: k.description,
        config: k.config,
    }
}

async fn list_kinds(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let kinds = state.analysis_config.list_kinds().await?;
    let views: Vec<KindView> = kinds.into_iter().map(kind_view).collect();
    Ok(Json(json!({ "kinds": views })))
}

async fn create_kind(
    State(state): State<AppState>,
    Json(payload): Json<KindPayload>,
) -> AppResult<Json<KindView>> {
    let kind = state.analysis_config.create_kind(kind_input(payload)).await?;
    Ok(Json(kind_view(kind)))
}

async fn update_kind(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<KindPayload>,
) -> AppResult<Json<KindView>> {
    let kind = state.analysis_config.update_kind(id, kind_input(payload)).await?;
    Ok(Json(kind_view(kind)))
}

async fn delete_kind(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    state.analysis_config.delete_kind(id).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn list_properties(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let props = state.analysis_config.list_properties().await?;
    let views: Vec<PropertyView> = props.into_iter().map(property_view).collect();
    Ok(Json(json!({ "properties": views })))
}

fn property_input(p: PropertyPayload) -> EntityPropertyInput {
    EntityPropertyInput {
        entity_type: p.entity_type,
        prop_id: p.prop_id,
        name: p.name,
        description: p.description,
        data_type: p.data_type,
    }
}

fn property_view(p: crate::analysis_config::model::EntityProperty) -> PropertyView {
    PropertyView {
        id: p.id,
        entity_type: p.entity_type,
        prop_id: p.prop_id,
        name: p.name,
        description: p.description,
        data_type: p.data_type,
    }
}

async fn create_property(
    State(state): State<AppState>,
    Json(payload): Json<PropertyPayload>,
) -> AppResult<Json<PropertyView>> {
    let prop = state.analysis_config.create_property(property_input(payload)).await?;
    Ok(Json(property_view(prop)))
}

async fn update_property(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<PropertyPayload>,
) -> AppResult<Json<PropertyView>> {
    let prop = state.analysis_config.update_property(id, property_input(payload)).await?;
    Ok(Json(property_view(prop)))
}

async fn delete_property(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    state.analysis_config.delete_property(id).await?;
    Ok(Json(json!({ "deleted": true })))
}
