//! Jobs subsystem: typed job definitions, a runner with status tracking, and a
//! cron scheduler.

pub mod builtin;
pub mod clock;
pub mod controller;
pub mod job_type;
pub mod leader;
pub mod log_sink;
pub mod model;
pub mod repository;
pub mod runner;
pub mod scheduler;

pub use builtin::NoopJob;
pub use clock::{Clock, SystemClock};
pub use controller::{ControllerDeps, JobController};
pub use leader::{LeaderLock, PgLeaderLock, SqliteLeaderLock};
pub use job_type::{JobContext, JobType, JobTypeInfo, JobTypeRegistry};
pub use log_sink::{LogSink, PgLogSink, SqliteLogSink};
pub use model::{
    ExecStatus, ExecutionUpdate, Job, JobError, JobExecution, JobInput, JobOutcome, TriggerType,
};
pub use repository::{
    JobExecutionRepository, JobRepository, PgJobExecutionRepository, PgJobRepository,
    SqliteJobExecutionRepository, SqliteJobRepository,
};
pub use runner::{JobRunner, RunnerDeps};
pub use scheduler::{CronScheduler, Scheduler};
