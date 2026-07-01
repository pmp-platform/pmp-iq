//! Shared budget enforcement for LLM-using jobs (M39): evaluate the active
//! budgets for a unit of work and translate the decision into the job control
//! flow — warn-annotate, or hard-stop via `CannotRun` so the controller
//! reschedules past the limit rather than failing.

use super::job_type::JobContext;
use super::model::JobError;
use crate::cost::{BudgetDecision, BudgetGuard, LlmBudgetRepository, ScopeRef};
use serde_json::json;

/// Evaluate budgets for `scopes` before LLM work. A read error never blocks
/// work (logged and ignored); a warn annotates the execution; a hard-stop
/// returns `CannotRun{retry_at}` at the next period boundary.
pub async fn enforce_budget(
    guard: &BudgetGuard,
    budgets: &dyn LlmBudgetRepository,
    scopes: &[ScopeRef],
    ctx: &JobContext,
) -> Result<(), JobError> {
    let list = match budgets.list().await {
        Ok(list) if !list.is_empty() => list,
        Ok(_) => return Ok(()),
        Err(e) => {
            ctx.log(&format!("budget check skipped (read error): {e}")).await;
            return Ok(());
        }
    };
    match guard.evaluate(&list, scopes, ctx.clock.now()).await {
        Ok(BudgetDecision::Ok) => Ok(()),
        Ok(BudgetDecision::Warn { spent_usd, limit_usd }) => {
            ctx.log(&format!(
                "⚠ LLM budget warning: ${spent_usd:.2} of ${limit_usd:.2} spent this period"
            ))
            .await;
            ctx.merge_metadata(&json!({
                "budget": { "warn": true, "spent_usd": spent_usd, "limit_usd": limit_usd }
            }))
            .await;
            Ok(())
        }
        Ok(BudgetDecision::Stop { spent_usd, limit_usd, retry_at }) => {
            ctx.log(&format!(
                "⛔ LLM budget exceeded: ${spent_usd:.2} of ${limit_usd:.2} — rescheduling to {retry_at}"
            ))
            .await;
            Err(JobError::CannotRun { retry_at: Some(retry_at) })
        }
        Err(e) => {
            ctx.log(&format!("budget check failed (continuing): {e}")).await;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::model::{Budget, BudgetPeriod, ModelTokens};
    use crate::cost::repository::{MockLlmBudgetRepository, MockLlmUsageRepository};
    use crate::cost::{BudgetScope, PriceTable};
    use crate::jobs::clock::MockClock;
    use crate::jobs::job_type::JobContext;
    use crate::jobs::log_sink::MockLogSink;
    use crate::jobs::repository::MockJobExecutionRepository;
    use serde_json::Value;
    use std::sync::Arc;
    use uuid::Uuid;

    fn ctx() -> JobContext {
        let mut log = MockLogSink::new();
        log.expect_append().returning(|_, _| Ok(()));
        let mut execs = MockJobExecutionRepository::new();
        execs.expect_merge_metadata().returning(|_, _| Ok(()));
        let mut clock = MockClock::new();
        clock.expect_now().returning(chrono::Utc::now);
        JobContext {
            execution_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            job_name: "j".into(),
            config: Value::Null,
            params: Value::Null,
            state: Value::Null,
            log: Arc::new(log),
            executions: Arc::new(execs),
            clock: Arc::new(clock),
        }
    }

    /// A guard whose usage repo always reports 1M opus output tokens ($75).
    fn guard_75() -> BudgetGuard {
        let mut usage = MockLlmUsageRepository::new();
        usage.expect_usage_since().returning(|_, _, _| {
            Ok(vec![ModelTokens { model: "claude-opus-4".into(), input_tokens: 0, output_tokens: 1_000_000 }])
        });
        BudgetGuard::new(Arc::new(usage), PriceTable::default())
    }

    fn budgets_repo(budget: Option<Budget>) -> MockLlmBudgetRepository {
        let mut repo = MockLlmBudgetRepository::new();
        repo.expect_list().returning(move || Ok(budget.clone().into_iter().collect()));
        repo
    }

    fn global(limit: f64, hard: bool) -> Budget {
        Budget {
            id: Uuid::new_v4(),
            scope: BudgetScope::Global,
            scope_id: None,
            period: BudgetPeriod::Monthly,
            limit_usd: limit,
            hard_stop: hard,
        }
    }

    #[tokio::test]
    async fn no_budgets_is_ok() {
        let repo = budgets_repo(None);
        assert!(enforce_budget(&guard_75(), &repo, &[], &ctx()).await.is_ok());
    }

    #[tokio::test]
    async fn soft_over_budget_warns_but_proceeds() {
        let repo = budgets_repo(Some(global(50.0, false)));
        assert!(enforce_budget(&guard_75(), &repo, &[], &ctx()).await.is_ok());
    }

    #[tokio::test]
    async fn hard_over_budget_reschedules() {
        let repo = budgets_repo(Some(global(50.0, true)));
        let err = enforce_budget(&guard_75(), &repo, &[], &ctx()).await.unwrap_err();
        assert!(matches!(err, JobError::CannotRun { retry_at: Some(_) }));
    }

    #[tokio::test]
    async fn list_error_does_not_block() {
        let mut repo = MockLlmBudgetRepository::new();
        repo.expect_list().returning(|| Err(crate::db::RepoError::NotFound));
        assert!(enforce_budget(&guard_75(), &repo, &[], &ctx()).await.is_ok());
    }
}
