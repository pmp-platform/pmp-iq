//! Version currency + tech radar routes (M45): per-application and fleet currency
//! reports, and the operator-curated tech radar.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{AppError, AppResult};
use crate::platform::ListQuery;
use crate::techradar::repository::VersionPolicy;
use crate::techradar::{DepCurrency, DepInput, RadarInput, assess, currency_score};
use crate::web::{PageContext, render_page};
use axum::extract::{Path, State};
use axum::response::Html;
use axum::routing::{delete, get};
use axum::{Extension, Json, Router};
use chrono::{NaiveDate, Utc};
use minijinja::context;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

const EOL_SOON_DAYS: i64 = 90;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/platform/tech-radar", get(radar_page))
        .route("/api/platform/applications/:id/currency", get(app_currency))
        .route("/api/platform/currency", get(fleet_currency))
        .route("/api/platform/tech-radar", get(list_radar).post(upsert_radar))
        .route("/api/platform/tech-radar/:id", delete(delete_radar))
}

fn parse_eol(date: &Option<String>) -> Option<NaiveDate> {
    date.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}

/// `(ecosystem, name) → policy` for fast lookup.
async fn policy_map(state: &AppState) -> AppResult<HashMap<(String, String), VersionPolicy>> {
    Ok(state
        .techradar
        .list_policies()
        .await?
        .into_iter()
        .map(|p| ((p.ecosystem.clone(), p.name.clone()), p))
        .collect())
}

/// Assess an application's libraries against the policy map.
fn assess_libraries(detail: &Value, policies: &HashMap<(String, String), VersionPolicy>, today: NaiveDate) -> Vec<DepCurrency> {
    detail
        .get("libraries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|lib| {
            let name = lib.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let ecosystem = lib.get("ecosystem").and_then(Value::as_str).unwrap_or("").to_string();
            let version = lib.get("version").and_then(Value::as_str).unwrap_or("").to_string();
            let policy = policies.get(&(ecosystem.clone(), name.clone()));
            let dep = DepInput {
                name,
                ecosystem,
                version,
                latest: policy.and_then(|p| p.latest.clone()),
                eol: policy.and_then(|p| parse_eol(&p.eol_date)),
            };
            assess(&dep, today, EOL_SOON_DAYS)
        })
        .collect()
}

async fn app_currency(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let detail = state.platform.detail("applications", id).await?;
    let deps = assess_libraries(&detail, &policy_map(&state).await?, Utc::now().date_naive());
    Ok(Json(json!({ "score": currency_score(&deps), "dependencies": deps })))
}

async fn fleet_currency(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let policies = policy_map(&state).await?;
    let today = Utc::now().date_naive();
    let page = state
        .platform
        .list("applications", &ListQuery::new(None, None, Some(500), Default::default()))
        .await?;
    let mut rows = Vec::new();
    for item in &page.items {
        let Some(id) = item.get("id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) else {
            continue;
        };
        let detail = state.platform.detail("applications", id).await?;
        let deps = assess_libraries(&detail, &policies, today);
        let eol = deps.iter().filter(|d| d.eol_status == "eol").count();
        rows.push(json!({
            "id": id.to_string(),
            "name": item.get("name").cloned().unwrap_or(Value::Null),
            "href": format!("/platform/applications/{id}"),
            "score": currency_score(&deps),
            "eol_count": eol,
        }));
    }
    rows.sort_by(|a, b| a["score"].as_f64().unwrap_or(1.0).partial_cmp(&b["score"].as_f64().unwrap_or(1.0)).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Json(json!({ "currency": rows })))
}

async fn list_radar(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "radar": state.techradar.list_radar().await? })))
}

#[derive(Deserialize)]
struct RadarBody {
    quadrant: String,
    name: String,
    ring: String,
    #[serde(default)]
    note: Option<String>,
}

async fn upsert_radar(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Json(body): Json<RadarBody>,
) -> AppResult<Json<Value>> {
    if !user.has_role("admin") {
        return Err(AppError::Unauthorized);
    }
    if body.quadrant.trim().is_empty() || body.name.trim().is_empty() || body.ring.trim().is_empty() {
        return Err(AppError::BadRequest("quadrant, name and ring are required".into()));
    }
    state
        .techradar
        .upsert_radar(RadarInput { quadrant: body.quadrant, name: body.name, ring: body.ring, note: body.note })
        .await?;
    Ok(Json(json!({ "saved": true })))
}

async fn delete_radar(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    if !user.has_role("admin") {
        return Err(AppError::Unauthorized);
    }
    state.techradar.delete_radar(id).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn radar_page(State(state): State<AppState>, Extension(user): Extension<Principal>) -> AppResult<Html<String>> {
    let page = PageContext::new(Some(user.display_name), "platform");
    render_page(&state.engine, "tech_radar.html", &page, context! { active_tab => "tech-radar" })
}
