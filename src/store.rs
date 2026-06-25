//! Engine-dispatching factories: given a [`Database`], build the right
//! Postgres or SQLite implementation behind each repository trait.

use crate::accounts::repository::{
    PgRepositoryAccountRepository, RepositoryAccountRepository, SqliteRepositoryAccountRepository,
};
use crate::ai::repository::{AiProfileRepository, PgAiProfileRepository, SqliteAiProfileRepository};
use crate::appsettings::{PgSettingsRepository, SettingsRepository, SqliteSettingsRepository};
use crate::db::Database;
use crate::jobs::leader::{LeaderLock, PgLeaderLock, SqliteLeaderLock};
use crate::jobs::log_sink::{LogSink, PgLogSink, SqliteLogSink};
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
engine_factory!(jobs, JobRepository, PgJobRepository, SqliteJobRepository);
engine_factory!(
    job_executions,
    JobExecutionRepository,
    PgJobExecutionRepository,
    SqliteJobExecutionRepository
);
engine_factory!(log_sink, LogSink, PgLogSink, SqliteLogSink);
engine_factory!(leader_lock, LeaderLock, PgLeaderLock, SqliteLeaderLock);
engine_factory!(repo_records, RepoRecordRepository, PgRepoRecordRepository, SqliteRepoRecordRepository);
engine_factory!(platform_writer, PlatformWriter, PgPlatformWriter, SqlitePlatformWriter);
engine_factory!(platform_query, PlatformQuery, PgPlatformQuery, SqlitePlatformQuery);
engine_factory!(graph_query, GraphQuery, PgGraphQuery, SqliteGraphQuery);
