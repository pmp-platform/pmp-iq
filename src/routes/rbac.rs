//! Team & role management routes (M37), gated to admins via `role_guard`.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::rbac::{Role, TeamInput, role_guard};
use axum::extract::{Path, State};
use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams", get(list_teams).post(create_team))
        .route("/api/teams/:id", delete(delete_team))
        .route("/api/teams/:id/members", post(add_member))
        .route("/api/teams/:id/applications", post(set_owner))
        .route("/api/roles", get(list_roles).post(set_role))
        .route_layer(from_fn_with_state(Role::Admin, role_guard))
}

async fn list_teams(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "teams": state.rbac.list_teams().await? })))
}

#[derive(Deserialize)]
struct NewTeam {
    name: String,
    #[serde(default)]
    tenant_id: Option<String>,
}

async fn create_team(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Json(body): Json<NewTeam>,
) -> AppResult<Json<Value>> {
    if body.name.trim().is_empty() {
        return Err(AppError::BadRequest("team name is required".into()));
    }
    let team = state.rbac.create_team(TeamInput { name: body.name, tenant_id: body.tenant_id }).await?;
    state.audit.record(&user.username, "team.create", Some(&team.name), json!({})).await;
    Ok(Json(json!(team)))
}

async fn delete_team(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    state.rbac.delete_team(id).await?;
    state.audit.record(&user.username, "team.delete", Some(&id.to_string()), json!({})).await;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
struct MemberBody {
    principal: String,
}

async fn add_member(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<MemberBody>,
) -> AppResult<Json<Value>> {
    state.rbac.add_member(id, &body.principal).await?;
    Ok(Json(json!({ "added": true })))
}

#[derive(Deserialize)]
struct OwnerBody {
    application_id: Uuid,
}

async fn set_owner(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<OwnerBody>,
) -> AppResult<Json<Value>> {
    state.rbac.set_owner(id, body.application_id).await?;
    Ok(Json(json!({ "owned": true })))
}

async fn list_roles(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let roles: Vec<Value> = state
        .rbac
        .list_roles()
        .await?
        .into_iter()
        .map(|(principal, role)| json!({ "principal": principal, "role": role }))
        .collect();
    Ok(Json(json!({ "roles": roles })))
}

#[derive(Deserialize)]
struct RoleBody {
    principal: String,
    role: String,
}

async fn set_role(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Json(body): Json<RoleBody>,
) -> AppResult<Json<Value>> {
    let role = Role::parse(&body.role);
    state.rbac.set_role(&body.principal, role).await?;
    state.audit.record(&user.username, "role.set", Some(&body.principal), json!({ "role": role })).await;
    Ok(Json(json!({ "principal": body.principal, "role": role })))
}
