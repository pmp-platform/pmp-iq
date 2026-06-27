//! Application wiring: shared state and HTTP router construction.

use crate::accounts::{AccountService, ProviderDeps};
use crate::ai::{AiProfileService, AiProviderDeps};
use crate::analysis_config::AnalysisConfigService;
use crate::auth::AuthService;
use crate::config::Config;
use crate::crypto::AesGcmEncryptor;
use crate::db::Database;
use crate::error::AppError;
use crate::fs::RealFileSystem;
use crate::git::Git2Client;
use crate::httpclient::{HttpClient, ReqwestClient, ThrottledHttpClient};
use crate::jobs::repository::{JobExecutionRepository, JobRepository};
use crate::jobs::{JobRunner, JobTypeRegistry, NoopJob, RunnerDeps, SystemClock};
use crate::platform::graph::GraphQuery;
use crate::platform::query::PlatformQuery;
use crate::platform::FileAnalyzer;
use crate::process::TokioCommandRunner;
use crate::repositories::RepoRecordRepository;
use crate::review::{ReviewDeps, ReviewRepositoriesJob};
use crate::routes;
use crate::store;
use crate::web::TemplateEngine;
use crate::workspace::Workspace;
use axum::Router;
use std::sync::Arc;

/// Shared, cheaply-cloneable application state handed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
    pub engine: TemplateEngine,
    pub auth: Arc<AuthService>,
    pub deps: ProviderDeps,
    pub accounts: AccountService,
    pub ai_deps: AiProviderDeps,
    pub ai: AiProfileService,
    pub analysis_config: AnalysisConfigService,
    pub jobs_repo: Arc<dyn JobRepository>,
    pub executions_repo: Arc<dyn JobExecutionRepository>,
    pub job_types: Arc<JobTypeRegistry>,
    pub runner: Arc<JobRunner>,
    pub repo_records: Arc<dyn RepoRecordRepository>,
    pub platform: Arc<dyn PlatformQuery>,
    pub graph: Arc<dyn GraphQuery>,
}

impl AppState {
    /// Build application state, constructing infrastructure services. Fails if
    /// the encryption key is invalid. The concrete Postgres or SQLite
    /// repository implementations are chosen from `db`'s engine.
    pub fn build(config: Config, db: Database, auth: Arc<AuthService>) -> Result<Self, AppError> {
        let encryptor = AesGcmEncryptor::from_base64(&config.auth.encryption_key)
            .map_err(|e| AppError::internal(format!("invalid ENCRYPTION_KEY: {e}")))?;
        let raw_http: Arc<dyn HttpClient> = Arc::new(ReqwestClient::new());
        // Git-provider calls are throttled to stay under API rate limits.
        let throttled_http: Arc<dyn HttpClient> = Arc::new(ThrottledHttpClient::new(
            raw_http.clone(),
            std::time::Duration::from_millis(config.git_min_interval_ms),
        ));
        let deps = ProviderDeps {
            http: throttled_http,
            fs: Arc::new(RealFileSystem),
            encryptor: Arc::new(encryptor),
        };
        let accounts = AccountService::new(store::accounts(&db), deps.clone());

        let ai_deps = AiProviderDeps {
            http: raw_http,
            runner: Arc::new(TokioCommandRunner),
            encryptor: deps.encryptor.clone(),
        };
        let ai = AiProfileService::new(store::ai_profiles(&db), ai_deps.clone());
        let analysis_config = AnalysisConfigService::new(
            store::entity_kinds(&db),
            store::entity_properties(&db),
        );

        let jobs_repo = store::jobs(&db);
        let executions_repo = store::job_executions(&db);
        let repo_records = store::repo_records(&db);
        let workspace = Workspace::new(Arc::new(RealFileSystem), config.workspace_dir.clone());
        let review_job = ReviewRepositoriesJob::new(ReviewDeps {
            accounts: accounts.clone(),
            repositories: repo_records.clone(),
            git: Arc::new(Git2Client),
            workspace,
            analyzer: Arc::new(FileAnalyzer::new(Arc::new(RealFileSystem))),
            writer: store::platform_writer(&db),
            platform: store::platform_query(&db),
            ai: ai.clone(),
            ai_deps: ai_deps.clone(),
            analysis_config: analysis_config.clone(),
        });

        let mut registry = JobTypeRegistry::new();
        registry.register(Arc::new(NoopJob));
        registry.register(Arc::new(review_job));
        let registry = Arc::new(registry);
        let runner = Arc::new(JobRunner::new(RunnerDeps {
            jobs: jobs_repo.clone(),
            executions: executions_repo.clone(),
            registry: registry.clone(),
            clock: Arc::new(SystemClock),
            log_sink: store::log_sink(&db),
        }));

        let platform = store::platform_query(&db);
        let graph = store::graph_query(&db);
        let asset_version = config.app_version.clone();

        Ok(Self {
            config: Arc::new(config),
            db,
            engine: TemplateEngine::with_version(asset_version),
            auth,
            deps,
            accounts,
            ai_deps,
            ai,
            analysis_config,
            jobs_repo,
            executions_repo,
            job_types: registry,
            runner,
            repo_records,
            platform,
            graph,
        })
    }
}

/// Build the full HTTP router for the application.
pub fn build_router(state: AppState) -> Router {
    routes::router(state)
}
