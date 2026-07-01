//! Semantic search, similarity and duplicate-cluster routes (M40). Search falls
//! back to substring matching when no embeddings are available.

use crate::app::AppState;
use crate::embeddings::{Neighbour, cluster, neighbours_of};
use crate::error::AppResult;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

const SEARCH_K: usize = 20;
const SIMILAR_K: usize = 10;
const DEFAULT_DUP_THRESHOLD: f64 = 0.85;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/platform/search", get(search))
        .route("/api/platform/applications/:id/similar", get(similar))
        .route("/api/platform/duplicates", get(duplicates))
}

/// `(entity_type, id) → display name` for every embeddable entity (a plain
/// catalog query, so it powers the substring fallback too).
async fn name_map(state: &AppState) -> AppResult<HashMap<(String, Uuid), String>> {
    let rows = state.platform.embedding_sources().await?;
    Ok(rows.into_iter().map(|r| ((r.entity_type, r.entity_id), r.name)).collect())
}

fn href_for(entity_type: &str, id: Uuid) -> Option<String> {
    match entity_type {
        "application" => Some(format!("/platform/applications/{id}")),
        _ => None,
    }
}

fn enrich(neighbours: &[Neighbour], names: &HashMap<(String, Uuid), String>) -> Vec<Value> {
    neighbours
        .iter()
        .map(|n| {
            let name = names.get(&(n.entity_type.clone(), n.entity_id)).cloned().unwrap_or_default();
            json!({
                "entity_type": n.entity_type,
                "entity_id": n.entity_id.to_string(),
                "name": name,
                "score": n.score,
                "href": href_for(&n.entity_type, n.entity_id),
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default, rename = "type")]
    entity_type: Option<String>,
}

async fn search(State(state): State<AppState>, Query(q): Query<SearchQuery>) -> AppResult<Json<Value>> {
    let names = name_map(&state).await?;
    if q.q.trim().is_empty() {
        return Ok(Json(json!({ "mode": "empty", "results": [] })));
    }
    // Try semantic search when a provider is configured and embeddings exist.
    if let Some(provider) = &state.embedding_provider {
        if let Ok(vectors) = provider.embed(std::slice::from_ref(&q.q)).await {
            if let Some(query_vec) = vectors.into_iter().next() {
                let neighbours = state
                    .embeddings
                    .nearest(&provider.model(), query_vec, q.entity_type.clone(), SEARCH_K)
                    .await?;
                if !neighbours.is_empty() {
                    return Ok(Json(json!({ "mode": "semantic", "results": enrich(&neighbours, &names) })));
                }
            }
        }
    }
    Ok(Json(json!({ "mode": "substring", "results": substring(&q, &names) })))
}

/// Substring fallback over entity names, ranked by match position.
fn substring(q: &SearchQuery, names: &HashMap<(String, Uuid), String>) -> Vec<Value> {
    let needle = q.q.to_lowercase();
    let mut hits: Vec<(usize, Value)> = names
        .iter()
        .filter(|((etype, _), _)| q.entity_type.as_deref().is_none_or(|t| t == etype))
        .filter_map(|((etype, id), name)| {
            name.to_lowercase().find(&needle).map(|pos| {
                (pos, json!({
                    "entity_type": etype,
                    "entity_id": id.to_string(),
                    "name": name,
                    "score": Value::Null,
                    "href": href_for(etype, *id),
                }))
            })
        })
        .collect();
    hits.sort_by_key(|(pos, _)| *pos);
    hits.into_iter().take(SEARCH_K).map(|(_, v)| v).collect()
}

async fn similar(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let Some(provider) = &state.embedding_provider else {
        return Ok(Json(json!({ "enabled": false, "results": [] })));
    };
    let all = state.embeddings.all(&provider.model(), Some("application".to_string())).await?;
    let neighbours = neighbours_of(&all, id, SIMILAR_K);
    let names = name_map(&state).await?;
    Ok(Json(json!({ "enabled": true, "results": enrich(&neighbours, &names) })))
}

#[derive(Deserialize)]
struct DuplicatesQuery {
    #[serde(default, rename = "type")]
    entity_type: Option<String>,
    #[serde(default)]
    threshold: Option<f64>,
}

async fn duplicates(State(state): State<AppState>, Query(q): Query<DuplicatesQuery>) -> AppResult<Json<Value>> {
    let Some(provider) = &state.embedding_provider else {
        return Ok(Json(json!({ "enabled": false, "clusters": [] })));
    };
    let entity_type = q.entity_type.unwrap_or_else(|| "application".into());
    let threshold = q.threshold.unwrap_or(DEFAULT_DUP_THRESHOLD).clamp(0.0, 1.0) as f32;
    let all = state.embeddings.all(&provider.model(), Some(entity_type.clone())).await?;
    let names = name_map(&state).await?;
    let clusters: Vec<Value> = cluster(&all, threshold)
        .into_iter()
        .map(|group| {
            let members: Vec<Value> = group
                .into_iter()
                .map(|id| {
                    json!({
                        "entity_id": id.to_string(),
                        "name": names.get(&(entity_type.clone(), id)).cloned().unwrap_or_default(),
                        "href": href_for(&entity_type, id),
                    })
                })
                .collect();
            json!({ "members": members })
        })
        .collect();
    Ok(Json(json!({ "enabled": true, "entity_type": entity_type, "clusters": clusters })))
}
