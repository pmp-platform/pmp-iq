//! Application wiring: shared state and HTTP router construction.

use crate::accounts::{AccountService, ProviderDeps};
use crate::agent_tasks::repository::AgentTaskRepository;
use crate::agent_tasks::{AgentTaskDeps, AgentTaskJob};
use crate::ai::{AiProfileService, AiProviderDeps};
use crate::analysis_config::AnalysisConfigService;
use crate::auth::{AuthService, GitHubIdentity};
use crate::config::{AuthProvider, Config, GitHubAuthConfig};
use crate::crypto::AesGcmEncryptor;
use crate::db::Database;
use crate::error::AppError;
use crate::fs::RealFileSystem;
use crate::git::Git2Client;
use crate::hints::EntityHintRepository;
use crate::httpclient::{HttpClient, ReqwestClient, ThrottledHttpClient};
use crate::jobs::repository::{JobExecutionRepository, JobRepository};
use crate::jobs::{JobRunner, JobTypeRegistry, NoopJob, RunnerDeps, SystemClock};
use crate::llm_request::{LlmRepositoryRequestJob, LlmRequestDeps};
use crate::locks::DistributedLock;
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

/// GitHub authentication state for the OAuth web-flow routes (M21). Present only
/// when `auth.provider` is `github` and an identity client was configured.
#[derive(Clone)]
pub struct GitHubAuthState {
    pub identity: Arc<dyn GitHubIdentity>,
    pub config: GitHubAuthConfig,
}

/// Shared, cheaply-cloneable application state handed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
    pub engine: TemplateEngine,
    pub auth: Arc<AuthService>,
    pub github_auth: Option<GitHubAuthState>,
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
    pub lock: Arc<dyn DistributedLock>,
    pub hints: Arc<dyn EntityHintRepository>,
    pub agent_tasks: Arc<dyn AgentTaskRepository>,
    pub metrics: Arc<dyn crate::metrics::ApplicationMetricsRepository>,
    pub campaigns: Arc<dyn crate::campaigns::CampaignRepository>,
}

impl AppState {
    /// Build application state, constructing infrastructure services. Fails if
    /// the encryption key is invalid. The concrete Postgres or SQLite
    /// repository implementations are chosen from `db`'s engine.
    pub fn build(
        config: Config,
        db: Database,
        auth: Arc<AuthService>,
        github_identity: Option<Arc<dyn GitHubIdentity>>,
    ) -> Result<Self, AppError> {
        let encryptor = AesGcmEncryptor::from_base64(&config.auth.encryption_key)
            .map_err(|e| AppError::internal(format!("invalid ENCRYPTION_KEY: {e}")))?;
        let github_auth = match (config.auth.provider, &config.auth.github, &github_identity) {
            (AuthProvider::Github, Some(gh), Some(identity)) => Some(GitHubAuthState {
                identity: identity.clone(),
                config: gh.clone(),
            }),
            _ => None,
        };
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
        let lock = store::distributed_lock(&db, &config.redis)?;
        let hints = store::entity_hints(&db);
        let agent_tasks = store::agent_tasks(&db);
        let app_metrics = store::application_metrics(&db);
        let campaigns = store::campaigns(&db);
        let workspace = Workspace::new(Arc::new(RealFileSystem), config.workspace_dir.clone());
        let review_job = ReviewRepositoriesJob::new(ReviewDeps {
            accounts: accounts.clone(),
            repositories: repo_records.clone(),
            git: Arc::new(Git2Client),
            workspace: workspace.clone(),
            analyzer: Arc::new(FileAnalyzer::new(Arc::new(RealFileSystem))),
            writer: store::platform_writer(&db),
            platform: store::platform_query(&db),
            ai: ai.clone(),
            ai_deps: ai_deps.clone(),
            analysis_config: analysis_config.clone(),
            lock: lock.clone(),
            hints: hints.clone(),
        });
        let llm_job = LlmRepositoryRequestJob::new(LlmRequestDeps {
            accounts: accounts.clone(),
            repositories: repo_records.clone(),
            git: Arc::new(Git2Client),
            workspace: workspace.clone(),
            ai: ai.clone(),
            ai_deps: ai_deps.clone(),
            lock: lock.clone(),
        });
        let agent_job = AgentTaskJob::new(AgentTaskDeps {
            tasks: agent_tasks.clone(),
            accounts: accounts.clone(),
            repositories: repo_records.clone(),
            git: Arc::new(Git2Client),
            workspace: workspace.clone(),
            ai: ai.clone(),
            ai_deps: ai_deps.clone(),
            lock: lock.clone(),
        });
        let metrics_job = crate::metrics::CollectMetricsJob::new(crate::metrics::CollectMetricsDeps {
            platform: store::platform_query(&db),
            repositories: repo_records.clone(),
            accounts: accounts.clone(),
            git: Arc::new(Git2Client),
            workspace,
            ai: ai.clone(),
            ai_deps: ai_deps.clone(),
            metrics: app_metrics.clone(),
            lock: lock.clone(),
        });
        let pr_watcher_job = crate::pr_watcher::PrWatcherJob::new(crate::pr_watcher::PrWatcherDeps {
            tasks: agent_tasks.clone(),
            repositories: repo_records.clone(),
            accounts: accounts.clone(),
            ai: ai.clone(),
            jobs: jobs_repo.clone(),
            executions: executions_repo.clone(),
            agent_max_concurrency: config.agent_max_concurrency,
        });

        let mut registry = JobTypeRegistry::new();
        registry.register(Arc::new(NoopJob));
        registry.register(Arc::new(review_job));
        registry.register(Arc::new(llm_job));
        registry.register(Arc::new(agent_job));
        registry.register(Arc::new(pr_watcher_job));
        registry.register(Arc::new(metrics_job));
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
            github_auth,
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
            lock,
            hints,
            agent_tasks,
            metrics: app_metrics,
            campaigns,
        })
    }
}

/// Build the full HTTP router for the application.
pub fn build_router(state: AppState) -> Router {
    routes::router(state)
}
