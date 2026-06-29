//! Application entrypoint. Loads configuration, builds services, starts the
//! HTTP server. Contains no business logic.

use clap::Parser;
use platform_inspector::app::{AppState, build_router};
use platform_inspector::auth::{
    Argon2Hasher, AuthService, GitHubIdentity, HttpGitHubIdentity, RandomSecretGenerator,
};
use platform_inspector::config::{AuthProvider, Config, ConfigLoader, SystemEnv};
use platform_inspector::db::Database;
use platform_inspector::fs::RealFileSystem;
use platform_inspector::httpclient::ReqwestClient;
use platform_inspector::jobs::{
    ControllerDeps, CronScheduler, JobController, Scheduler, SystemClock,
};
use platform_inspector::telemetry;
use std::sync::Arc;
use std::time::Duration;

/// Platform Inspector server.
#[derive(Debug, Parser)]
#[command(name = "platform-inspector", version, about)]
struct Cli {
    /// Path to a `config.yaml` (defaults to one beside the binary, then `./config.yaml`).
    #[arg(long = "config-file")]
    config_file: Option<String>,
}

/// Directory containing the running executable, used to find `config.yaml`.
fn exe_dir() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()))
}

/// Build the GitHub identity client when GitHub auth is configured.
fn build_github_identity(config: &Config) -> Option<Arc<dyn GitHubIdentity>> {
    match (&config.auth.provider, &config.auth.github) {
        (AuthProvider::Github, Some(gh)) => {
            Some(Arc::new(HttpGitHubIdentity::new(Arc::new(ReqwestClient::new()), gh)))
        }
        _ => None,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = ConfigLoader {
        fs: &RealFileSystem,
        env: &SystemEnv,
    }
    .load(cli.config_file.as_deref(), exe_dir().as_deref())?;
    telemetry::init(Some(&config.log_level));

    let db = Database::connect(&config.database).await?;

    // SQLite is the zero-config default: create the schema automatically.
    // PostgreSQL deployments manage schema with dbmate (see README).
    if matches!(db, Database::Sqlite(_)) {
        platform_inspector::db::migrate::apply(&db, platform_inspector::db::migrate::SQLITE_MIGRATIONS)
            .await?;
        tracing::info!("sqlite schema ensured");
    }

    let github_identity = build_github_identity(&config);
    let boot = AuthService::from_config(
        &config.auth,
        Arc::new(Argon2Hasher),
        &RandomSecretGenerator,
        github_identity.clone(),
    )?;
    if let Some(password) = &boot.admin.generated_password {
        tracing::warn!(
            user = %boot.admin.username,
            password = %password,
            "no ADMIN_PASS set — generated a random admin password (shown once)"
        );
    }

    let addr = config.server.socket_addr();
    let state = AppState::build(config, db, Arc::new(boot.service), github_identity)?;

    // Ensure the singleton job that backs per-application LLM questions exists.
    if let Err(e) = platform_inspector::llm_request::ensure_job(state.jobs_repo.as_ref()).await {
        tracing::warn!(error = %e, "failed to seed llm-repository-request job");
    }
    // Ensure the singleton job that backs application AI Agent tasks exists.
    if let Err(e) = platform_inspector::agent_tasks::ensure_job(state.jobs_repo.as_ref()).await {
        tracing::warn!(error = %e, "failed to seed application-agent-task job");
    }

    let scheduler = Arc::new(CronScheduler::new(
        state.runner.clone(),
        state.jobs_repo.clone(),
    ));
    if let Err(e) = scheduler.start().await {
        tracing::warn!(error = %e, "failed to start cron scheduler");
    }

    // Leader-elected controller: resumes paused jobs whose resume_at elapsed.
    let controller = Arc::new(JobController::new(
        ControllerDeps {
            runner: state.runner.clone(),
            jobs: state.jobs_repo.clone(),
            executions: state.executions_repo.clone(),
            lock: state.lock.clone(),
            clock: Arc::new(SystemClock),
        },
        Duration::from_secs(10),
    ));
    tokio::spawn(controller.run());

    let router = build_router(state);

    tracing::info!(%addr, "starting platform-inspector");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolve when the process receives Ctrl-C, allowing in-flight requests to
/// drain before exit.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
