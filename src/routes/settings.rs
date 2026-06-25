//! Settings: repository accounts CRUD API and page.

use crate::accounts::{
    AccountForm, AuthType, ProviderType, RepositoryAccount, SelectionMode,
};
use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::web::{PageContext, render_page};
use axum::Extension;
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::response::Html;
use axum::routing::{get, post};
use minijinja::context;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings", get(settings_page))
        .route(
            "/api/settings/accounts",
            get(list_accounts).post(create_account),
        )
        .route(
            "/api/settings/accounts/:id",
            axum::routing::put(update_account).delete(delete_account),
        )
        .route("/api/settings/accounts/:id/validate", post(validate_account))
        .route(
            "/api/settings/accounts/:id/repositories",
            get(preview_account),
        )
}

/// API payload for creating/updating an account (token is plaintext).
#[derive(Deserialize)]
struct AccountPayload {
    name: String,
    provider_type: ProviderType,
    auth_type: AuthType,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    token: Option<String>,
    selection_mode: SelectionMode,
    #[serde(default)]
    selection_value: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

impl From<AccountPayload> for AccountForm {
    fn from(p: AccountPayload) -> Self {
        AccountForm {
            name: p.name,
            provider_type: p.provider_type,
            auth_type: p.auth_type,
            base_url: p.base_url,
            token: p.token,
            selection_mode: p.selection_mode,
            selection_value: p.selection_value,
            enabled: p.enabled,
        }
    }
}

/// Account representation safe to return to clients (never includes secrets).
#[derive(Serialize)]
struct AccountView {
    id: Uuid,
    name: String,
    provider_type: ProviderType,
    auth_type: AuthType,
    base_url: Option<String>,
    selection_mode: SelectionMode,
    selection_value: Option<String>,
    enabled: bool,
    has_credentials: bool,
}

impl From<&RepositoryAccount> for AccountView {
    fn from(a: &RepositoryAccount) -> Self {
        AccountView {
            id: a.id,
            name: a.name.clone(),
            provider_type: a.provider_type,
            auth_type: a.auth_type,
            base_url: a.base_url.clone(),
            selection_mode: a.selection_mode,
            selection_value: a.selection_value.clone(),
            enabled: a.enabled,
            has_credentials: a.credentials_enc.is_some(),
        }
    }
}

async fn settings_page(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Html<String>> {
    let accounts = state.accounts.list().await?;
    let views: Vec<AccountView> = accounts.iter().map(AccountView::from).collect();
    let json_accounts =
        serde_json::to_string(&views).map_err(AppError::internal)?;
    let page = PageContext::new(Some(user.display_name), "settings");
    render_page(
        &state.engine,
        "settings.html",
        &page,
        context! { accounts => views.len(), accounts_json => json_accounts },
    )
}

async fn list_accounts(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let accounts = state.accounts.list().await?;
    let views: Vec<AccountView> = accounts.iter().map(AccountView::from).collect();
    Ok(Json(json!({ "accounts": views })))
}

async fn create_account(
    State(state): State<AppState>,
    Json(payload): Json<AccountPayload>,
) -> AppResult<Json<AccountView>> {
    let account = state.accounts.create(payload.into()).await?;
    Ok(Json(AccountView::from(&account)))
}

async fn update_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<AccountPayload>,
) -> AppResult<Json<AccountView>> {
    let account = state.accounts.update(id, payload.into()).await?;
    Ok(Json(AccountView::from(&account)))
}

async fn delete_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    state.accounts.delete(id).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn validate_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    state.accounts.validate(id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn preview_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let repos = state.accounts.preview(id).await?;
    Ok(Json(json!({ "repositories": repos })))
}
