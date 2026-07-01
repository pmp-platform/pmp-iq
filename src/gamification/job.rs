//! The `gamification` job (M44): periodically replay recorded operator actions
//! into XP awards + badges (idempotent).

use super::service::GamificationService;
use crate::error::AppError;
use crate::jobs::model::{JobInput, TriggerType};
use crate::jobs::repository::JobRepository;
use crate::jobs::{JobContext, JobError, JobOutcome, JobType};
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

pub const JOB_TYPE: &str = "gamification";

pub struct GamificationJob {
    service: GamificationService,
}

impl GamificationJob {
    pub fn new(service: GamificationService) -> Self {
        Self { service }
    }
}

#[async_trait]
impl JobType for GamificationJob {
    fn id(&self) -> &str {
        JOB_TYPE
    }

    fn description(&self) -> &str {
        "Award operator XP/badges from recorded actions"
    }

    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        let awarded = self.service.replay().await.map_err(|e| JobError::Failed(e.to_string()))?;
        ctx.log(&format!("awarded {awarded} new action(s)")).await;
        Ok(JobOutcome::completed(json!({ "awarded": awarded })))
    }
}

/// Seed the hourly `gamification` cron job at boot.
pub async fn ensure_job(jobs: &dyn JobRepository) -> Result<Uuid, AppError> {
    let existing = jobs.list().await?;
    if let Some(job) = existing.iter().find(|j| j.job_type == JOB_TYPE) {
        return Ok(job.id);
    }
    let created = jobs
        .create(JobInput {
            job_type: JOB_TYPE.to_string(),
            name: "Gamification".to_string(),
            trigger_type: TriggerType::Cron,
            cron_expr: Some("17 * * * *".to_string()),
            config: json!({}),
            enabled: true,
            next_run_at: None,
        })
        .await?;
    Ok(created.id)
}
