//! Built-in job types that don't require external dependencies.

use super::job_type::{JobContext, JobType};
use super::model::{JobError, JobOutcome};
use async_trait::async_trait;
use serde_json::json;

/// A no-op job that simply records a log line. Useful for verifying the jobs
/// pipeline end to end.
pub struct NoopJob;

#[async_trait]
impl JobType for NoopJob {
    fn id(&self) -> &str {
        "noop"
    }

    fn description(&self) -> &str {
        "No-op job that records a log line — useful for verifying the jobs pipeline end to end."
    }

    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        ctx.log("noop job executed").await;
        Ok(JobOutcome::completed(json!({ "noop": true })))
    }
}
