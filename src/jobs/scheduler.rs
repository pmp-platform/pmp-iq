//! Cron scheduling of jobs.

use super::repository::JobRepository;
use super::runner::JobRunner;
use async_trait::async_trait;
use std::sync::Arc;
use tokio_cron_scheduler::{Job as CronJob, JobScheduler};

/// Starts background scheduling of enabled cron jobs.
#[async_trait]
pub trait Scheduler: Send + Sync {
    async fn start(self: Arc<Self>) -> anyhow::Result<()>;
}

/// `tokio-cron-scheduler`-backed implementation.
pub struct CronScheduler {
    runner: Arc<JobRunner>,
    jobs: Arc<dyn JobRepository>,
}

impl CronScheduler {
    pub fn new(runner: Arc<JobRunner>, jobs: Arc<dyn JobRepository>) -> Self {
        Self { runner, jobs }
    }
}

/// `tokio-cron-scheduler` expects a 6-field (seconds-first) cron expression.
/// Normalise a standard 5-field expression by prepending a `0` seconds field.
pub fn normalize_cron(expr: &str) -> String {
    let fields = expr.split_whitespace().count();
    if fields == 5 {
        format!("0 {expr}")
    } else {
        expr.to_string()
    }
}

#[async_trait]
impl Scheduler for CronScheduler {
    async fn start(self: Arc<Self>) -> anyhow::Result<()> {
        let scheduler = JobScheduler::new().await?;
        let cron_jobs = self.jobs.list_enabled_cron().await?;

        for job in cron_jobs {
            let Some(expr) = job.cron_expr.clone() else {
                continue;
            };
            let runner = self.runner.clone();
            let job_id = job.id;
            let cron = normalize_cron(&expr);
            let scheduled = CronJob::new_async(cron.as_str(), move |_uuid, _l| {
                let runner = runner.clone();
                Box::pin(async move {
                    if let Err(e) = runner.start(job_id, "cron").await {
                        tracing::warn!(%job_id, error = %e, "cron trigger failed");
                    }
                })
            });
            match scheduled {
                Ok(cron_job) => {
                    scheduler.add(cron_job).await?;
                }
                Err(e) => {
                    tracing::warn!(%job_id, expr = %expr, error = %e, "invalid cron expression");
                }
            }
        }

        scheduler.start().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_adds_seconds_field_to_five_field_expr() {
        assert_eq!(normalize_cron("*/5 * * * *"), "0 */5 * * * *");
    }

    #[test]
    fn normalize_leaves_six_field_expr_untouched() {
        assert_eq!(normalize_cron("0 0 * * * *"), "0 0 * * * *");
    }
}
