//! Settings: AI agent profiles CRUD API.

use crate::ai::{AiProfile, AiProviderType, ProfileForm};
use crate::app::AppState;
use crate::error::{AppError, AppResult};
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/settings/ai-profiles",
            get(list_profiles).post(create_profile),
        )
        .route(
            "/api/settings/ai-profiles/:id",
            axum::routing::put(update_profile).delete(delete_profile),
        )
        .route("/api/settings/ai-profiles/:id/validate", post(validate_profile))
        .route("/api/settings/ai-profiles/:id/test", post(test_profile))
}

/// API payload for creating/updating a profile (api_key is plaintext).
#[derive(Deserialize)]
struct ProfilePayload {
    name: String,
    provider_type: AiProviderType,
    #[serde(default = "default_config")]
    config: Value,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_config() -> Value {
    json!({})
}

fn default_true() -> bool {
    true
}

impl From<ProfilePayload> for ProfileForm {
    fn from(p: ProfilePayload) -> Self {
        ProfileForm {
            name: p.name,
            provider_type: p.provider_type,
            config: p.config,
            api_key: p.api_key,
            enabled: p.enabled,
        }
    }
}

/// Profile representation safe to return (never includes secrets).
#[derive(Serialize)]
struct ProfileView {
    id: Uuid,
    name: String,
    provider_type: AiProviderType,
    config: Value,
    enabled: bool,
    has_secret: bool,
}

impl From<&AiProfile> for ProfileView {
    fn from(p: &AiProfile) -> Self {
        ProfileView {
            id: p.id,
            name: p.name.clone(),
            provider_type: p.provider_type,
            config: p.config.clone(),
            enabled: p.enabled,
            has_secret: p.secrets_enc.is_some(),
        }
    }
}

#[derive(Deserialize)]
struct TestRequest {
    prompt: String,
}

async fn list_profiles(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let profiles = state.ai.list().await?;
    let views: Vec<ProfileView> = profiles.iter().map(ProfileView::from).collect();
    Ok(Json(json!({ "profiles": views })))
}

async fn create_profile(
    State(state): State<AppState>,
    Json(payload): Json<ProfilePayload>,
) -> AppResult<Json<ProfileView>> {
    let profile = state.ai.create(payload.into()).await?;
    Ok(Json(ProfileView::from(&profile)))
}

async fn update_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<ProfilePayload>,
) -> AppResult<Json<ProfileView>> {
    let profile = state.ai.update(id, payload.into()).await?;
    Ok(Json(ProfileView::from(&profile)))
}

async fn delete_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    state.ai.delete(id).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn validate_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    state.ai.validate(id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn test_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<TestRequest>,
) -> AppResult<Json<Value>> {
    if req.prompt.trim().is_empty() {
        return Err(AppError::BadRequest("prompt is required".into()));
    }
    let response = state.ai.test_prompt(id, &req.prompt).await?;
    Ok(Json(json!({ "response": response })))
}
