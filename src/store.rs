//! Engine-dispatching factories: given a [`Database`], build the right
//! Postgres or SQLite implementation behind each repository trait.

use crate::accounts::repository::{
    PgRepositoryAccountRepository, RepositoryAccountRepository, SqliteRepositoryAccountRepository,
};
use crate::agent_tasks::repository::{
    AgentTaskRepository, PgAgentTaskRepository, SqliteAgentTaskRepository,
};
use crate::campaigns::repository::{
    CampaignRepository, PgCampaignRepository, SqliteCampaignRepository,
};
use crate::ai::repository::{AiProfileRepository, PgAiProfileRepository, SqliteAiProfileRepository};
use crate::analysis_config::repository::{
    EntityKindRepository, EntityPropertyRepository, PgEntityKindRepository,
    PgEntityPropertyRepository, SqliteEntityKindRepository, SqliteEntityPropertyRepository,
};
use crate::appsettings::{PgSettingsRepository, SettingsRepository, SqliteSettingsRepository};
use crate::db::Database;
use crate::hints::{EntityHintRepository, PgEntityHintRepository, SqliteEntityHintRepository};
use crate::metrics::repository::{
    ApplicationMetricsRepository, PgApplicationMetricsRepository, SqliteApplicationMetricsRepository,
};
use crate::jobs::clock::{Clock, SystemClock};
use crate::jobs::log_sink::{LogSink, PgLogSink, SqliteLogSink};
use crate::config::RedisConfig;
use crate::error::AppError;
use crate::locks::{DistributedLock, PgSqlLock, RedisClientImpl, RedisLock, SqliteSqlLock};
use crate::jobs::repository::{
    JobExecutionRepository, JobRepository, PgJobExecutionRepository, PgJobRepository,
    SqliteJobExecutionRepository, SqliteJobRepository,
};
use crate::platform::graph::{GraphQuery, PgGraphQuery, SqliteGraphQuery};
use crate::platform::query::{PgPlatformQuery, PlatformQuery, SqlitePlatformQuery};
use crate::platform::writer::{PgPlatformWriter, PlatformWriter, SqlitePlatformWriter};
use crate::repositories::repository::{
    PgRepoRecordRepository, RepoRecordRepository, SqliteRepoRecordRepository,
};
use std::sync::Arc;

/// Generate an engine-dispatching factory returning a trait object.
macro_rules! engine_factory {
    ($fn:ident, $trait:path, $pg:ty, $sqlite:ty) => {
        pub fn $fn(db: &Database) -> Arc<dyn $trait> {
            match db {
                Database::Postgres(pool) => Arc::new(<$pg>::new(pool.clone())),
                Database::Sqlite(pool) => Arc::new(<$sqlite>::new(pool.clone())),
            }
        }
    };
}

engine_factory!(settings, SettingsRepository, PgSettingsRepository, SqliteSettingsRepository);
engine_factory!(
    accounts,
    RepositoryAccountRepository,
    PgRepositoryAccountRepository,
    SqliteRepositoryAccountRepository
);
engine_factory!(ai_profiles, AiProfileRepository, PgAiProfileRepository, SqliteAiProfileRepository);
engine_factory!(
    entity_kinds,
    EntityKindRepository,
    PgEntityKindRepository,
    SqliteEntityKindRepository
);
engine_factory!(
    entity_properties,
    EntityPropertyRepository,
    PgEntityPropertyRepository,
    SqliteEntityPropertyRepository
);
engine_factory!(jobs, JobRepository, PgJobRepository, SqliteJobRepository);
engine_factory!(
    job_executions,
    JobExecutionRepository,
    PgJobExecutionRepository,
    SqliteJobExecutionRepository
);
engine_factory!(log_sink, LogSink, PgLogSink, SqliteLogSink);
engine_factory!(repo_records, RepoRecordRepository, PgRepoRecordRepository, SqliteRepoRecordRepository);
engine_factory!(platform_writer, PlatformWriter, PgPlatformWriter, SqlitePlatformWriter);
engine_factory!(platform_query, PlatformQuery, PgPlatformQuery, SqlitePlatformQuery);
engine_factory!(graph_query, GraphQuery, PgGraphQuery, SqliteGraphQuery);
engine_factory!(
    entity_hints,
    EntityHintRepository,
    PgEntityHintRepository,
    SqliteEntityHintRepository
);
engine_factory!(
    agent_tasks,
    AgentTaskRepository,
    PgAgentTaskRepository,
    SqliteAgentTaskRepository
);
engine_factory!(
    application_metrics,
    ApplicationMetricsRepository,
    PgApplicationMetricsRepository,
    SqliteApplicationMetricsRepository
);
engine_factory!(campaigns, CampaignRepository, PgCampaignRepository, SqliteCampaignRepository);

/// Build the distributed lock. When Redis is enabled it backs the lock (correct
/// across instances through a shared Redis); otherwise the SQL-backed lock over
/// `controller_locks` is used. Takes a wall-clock for lease expiry.
pub fn distributed_lock(
    db: &Database,
    redis: &RedisConfig,
) -> Result<Arc<dyn DistributedLock>, AppError> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    if redis.enabled {
        let client = RedisClientImpl::connect(&redis.url)
            .map_err(|e| AppError::internal(format!("invalid REDIS_URL: {e}")))?;
        return Ok(Arc::new(RedisLock::new(Arc::new(client), clock)));
    }
    Ok(match db {
        Database::Postgres(pool) => Arc::new(PgSqlLock::new(pool.clone(), clock)),
        Database::Sqlite(pool) => Arc::new(SqliteSqlLock::new(pool.clone(), clock)),
    })
}
