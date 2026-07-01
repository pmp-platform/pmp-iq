//! Provider webhooks (M25): verified GitHub PR + push events. PR events trigger
//! an immediate PR-watcher reconcile (the M24 logic); a push to a repository's
//! default branch enqueues a scoped `sync-repositories` run. Public + signature-
//! verified; always acks fast and does work via jobs.

use crate::app::AppState;
use crate::error::{AppError, AppResult};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;

pub fn routes() -> Router<AppState> {
    Router::new().route("/webhooks/github", post(github_webhook))
}

/// Verify a GitHub `X-Hub-Signature-256: sha256=<hex>` HMAC over the raw body.
fn verify_github_signature(secret: &str, body: &[u8], header: &str) -> bool {
    let Some(hex_sig) = header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(sig) = hex::decode(hex_sig) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&sig).is_ok() // constant-time
}

async fn github_webhook(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    // Verify the signature when a secret is configured.
    if let Some(secret) = state.config.webhook_github_secret.as_deref() {
        let sig = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !verify_github_signature(secret, &body, sig) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    // Ack fast; enqueue work via jobs (never block the webhook response).
    if let Err(e) = dispatch_event(&state, &event, &payload).await {
        tracing::warn!(error = %e, %event, "webhook dispatch failed");
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn dispatch_event(state: &AppState, event: &str, payload: &Value) -> AppResult<()> {
    match event {
        "push" => handle_push(state, payload).await,
        "deployment_status" | "release" => handle_deployment(state, event, payload).await,
        "pull_request" | "pull_request_review" | "pull_request_review_comment" | "issue_comment"
        | "check_run" | "check_suite" => trigger_watcher(state).await,
        _ => Ok(()),
    }
}

/// A successful GitHub `deployment_status` (state=success) or a published
/// `release` records a DORA deployment for the mapped application (M47).
async fn handle_deployment(state: &AppState, event: &str, payload: &Value) -> AppResult<()> {
    let full_name = payload["repository"]["full_name"].as_str().unwrap_or("");
    if full_name.is_empty() {
        return Ok(());
    }
    // For deployment_status, only record terminal success/failure states.
    let succeeded = match event {
        "deployment_status" => match payload["deployment_status"]["state"].as_str().unwrap_or("") {
            "success" => true,
            "failure" | "error" => false,
            _ => return Ok(()), // pending/in_progress/queued — ignore
        },
        _ => true, // a published release
    };
    let repos = state.repo_records.list().await?;
    let Some(repo) = repos.into_iter().find(|r| r.full_name == full_name) else {
        return Ok(()); // not a tracked repository
    };
    let Some(app_id) = state.platform.repository_application(repo.id).await? else {
        return Ok(()); // no application mapped yet
    };
    let environment = payload["deployment"]["environment"].as_str().unwrap_or("production").to_string();
    let sha = payload["deployment"]["sha"]
        .as_str()
        .or_else(|| payload["release"]["tag_name"].as_str())
        .map(str::to_string);
    let _ = state
        .dora
        .record_deployment(crate::dora::NewDeployment {
            application_id: app_id,
            environment,
            sha,
            succeeded,
            first_commit_at: None,
        })
        .await?;
    Ok(())
}

/// Trigger an immediate PR-watcher reconcile (deduped: skip if one is in flight).
async fn trigger_watcher(state: &AppState) -> AppResult<()> {
    let job_id = crate::pr_watcher::ensure_job(state.jobs_repo.as_ref()).await?;
    if state.executions_repo.count_running(job_id).await.unwrap_or(0) == 0 {
        let _ = state.runner.start(job_id, "webhook").await;
    }
    Ok(())
}

/// A push to a repository's default branch enqueues a scoped re-sync.
async fn handle_push(state: &AppState, payload: &Value) -> AppResult<()> {
    let git_ref = payload["ref"].as_str().unwrap_or("");
    let default = payload["repository"]["default_branch"].as_str().unwrap_or("");
    let full_name = payload["repository"]["full_name"].as_str().unwrap_or("");
    if default.is_empty() || full_name.is_empty() || git_ref != format!("refs/heads/{default}") {
        return Ok(()); // not a default-branch push
    }
    let repos = state.repo_records.list().await?;
    let Some(repo) = repos.into_iter().find(|r| r.full_name == full_name) else {
        return Ok(()); // not a tracked repository
    };
    let profile = state
        .ai
        .list()
        .await
        .ok()
        .and_then(|ps| ps.iter().find(|p| p.enabled).or_else(|| ps.first()).map(|p| p.id));
    let job_id = crate::review::ensure_sync_job(state.jobs_repo.as_ref(), profile).await?;
    // Webhook-scoped re-syncs default to incremental analysis (M41).
    let _ = state
        .runner
        .start_with_params(job_id, "webhook", json!({ "repository_id": repo.id, "incremental": true }))
        .await
        .map_err(|e: AppError| e);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn verifies_a_correct_signature() {
        let body = br#"{"hello":"world"}"#;
        let sig = sign("topsecret", body);
        assert!(verify_github_signature("topsecret", body, &sig));
    }

    #[test]
    fn rejects_bad_signature_secret_or_format() {
        let body = br#"{"hello":"world"}"#;
        let sig = sign("topsecret", body);
        assert!(!verify_github_signature("wrong-secret", body, &sig));
        assert!(!verify_github_signature("topsecret", b"tampered", &sig));
        assert!(!verify_github_signature("topsecret", body, "not-prefixed"));
    }
}
