//! Application entrypoint. Loads configuration, builds services, starts the
//! HTTP server. Contains no business logic.

use platform_inspector::app::{AppState, build_router};
use platform_inspector::auth::{Argon2Hasher, AuthService, RandomSecretGenerator};
use platform_inspector::config::Config;
use platform_inspector::db::Database;
use platform_inspector::jobs::{
    ControllerDeps, CronScheduler, JobController, Scheduler, SystemClock,
};
use platform_inspector::store;
use platform_inspector::telemetry;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init();

    let config = Config::from_env()?;
    let db = Database::connect(&config.database).await?;

    // SQLite is the zero-config default: create the schema automatically.
    // PostgreSQL deployments manage schema with dbmate (see README).
    if matches!(db, Database::Sqlite(_)) {
        platform_inspector::db::migrate::apply(&db, platform_inspector::db::migrate::SQLITE_MIGRATIONS)
            .await?;
        tracing::info!("sqlite schema ensured");
    }

    let boot = AuthService::from_config(
        &config.auth,
        Arc::new(Argon2Hasher),
        &RandomSecretGenerator,
    )?;
    if let Some(password) = &boot.admin.generated_password {
        tracing::warn!(
            user = %boot.admin.username,
            password = %password,
            "no ADMIN_PASS set — generated a random admin password (shown once)"
        );
    }

    let addr = config.server.socket_addr();
    let state = AppState::build(config, db, Arc::new(boot.service))?;

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
            executions: state.executions_repo.clone(),
            lock: store::leader_lock(&state.db),
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
