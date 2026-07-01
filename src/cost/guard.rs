//! Budget enforcement (M39): sum period-to-date cost for the scopes that apply
//! to a unit of work and decide whether to warn or hard-stop it.

use super::model::{Budget, BudgetScope};
use super::pricing::PriceTable;
use super::repository::LlmUsageRepository;
use crate::db::RepoResult;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

/// A scope to evaluate budgets against (e.g. `(Application, app_id)`).
#[derive(Debug, Clone, Copy)]
pub struct ScopeRef {
    pub scope: BudgetScope,
    pub scope_id: Option<Uuid>,
}

impl ScopeRef {
    pub fn new(scope: BudgetScope, scope_id: Option<Uuid>) -> Self {
        Self { scope, scope_id }
    }
}

/// Outcome of a budget check (most-severe wins).
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetDecision {
    Ok,
    /// Over a warn (soft) budget — annotate the execution but proceed.
    Warn { spent_usd: f64, limit_usd: f64 },
    /// Over a hard-stop budget — reschedule until the period resets.
    Stop { spent_usd: f64, limit_usd: f64, retry_at: DateTime<Utc> },
}

impl BudgetDecision {
    fn severity(&self) -> u8 {
        match self {
            BudgetDecision::Ok => 0,
            BudgetDecision::Warn { .. } => 1,
            BudgetDecision::Stop { .. } => 2,
        }
    }
}

/// Does a budget apply to any of the given scope refs? A global budget always
/// applies; a scoped budget applies when a ref shares its scope and id.
fn applies(budget: &Budget, scopes: &[ScopeRef]) -> bool {
    if matches!(budget.scope, BudgetScope::Global) {
        return true;
    }
    scopes
        .iter()
        .any(|s| s.scope == budget.scope && s.scope_id == budget.scope_id)
}

/// Sums period-to-date cost per budget scope and decides warn/stop.
pub struct BudgetGuard {
    usage: Arc<dyn LlmUsageRepository>,
    prices: PriceTable,
}

impl BudgetGuard {
    pub fn new(usage: Arc<dyn LlmUsageRepository>, prices: PriceTable) -> Self {
        Self { usage, prices }
    }

    /// Evaluate the budgets that apply to `scopes` as of `now`, returning the
    /// most severe decision. Over a hard-stop budget the retry time is the next
    /// period start; over a warn budget it annotates only.
    pub async fn evaluate(
        &self,
        budgets: &[Budget],
        scopes: &[ScopeRef],
        now: DateTime<Utc>,
    ) -> RepoResult<BudgetDecision> {
        let mut decision = BudgetDecision::Ok;
        for b in budgets {
            if !applies(b, scopes) {
                continue;
            }
            let since = b.period.start(now);
            let rows = self.usage.usage_since(b.scope, b.scope_id, since).await?;
            let spent = self.prices.total(&rows);
            if spent < b.limit_usd {
                continue;
            }
            let next = if b.hard_stop {
                BudgetDecision::Stop {
                    spent_usd: spent,
                    limit_usd: b.limit_usd,
                    retry_at: b.period.next_start(now),
                }
            } else {
                BudgetDecision::Warn { spent_usd: spent, limit_usd: b.limit_usd }
            };
            if next.severity() > decision.severity() {
                decision = next;
            }
        }
        Ok(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::model::{BudgetPeriod, ModelTokens};
    use crate::cost::repository::MockLlmUsageRepository;
    use chrono::TimeZone;

    fn budget(scope: BudgetScope, scope_id: Option<Uuid>, limit: f64, hard: bool) -> Budget {
        Budget {
            id: Uuid::new_v4(),
            scope,
            scope_id,
            period: BudgetPeriod::Monthly,
            limit_usd: limit,
            hard_stop: hard,
        }
    }

    /// A usage repo returning a fixed model/token total for any query.
    fn usage_returning(input: i64, output: i64) -> Arc<MockLlmUsageRepository> {
        let mut m = MockLlmUsageRepository::new();
        m.expect_usage_since().returning(move |_, _, _| {
            Ok(vec![ModelTokens { model: "claude-opus-4".into(), input_tokens: input, output_tokens: output }])
        });
        Arc::new(m)
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn under_limit_is_ok() {
        // 1M output @ $75 = $75 spent, limit $100 → Ok.
        let guard = BudgetGuard::new(usage_returning(0, 1_000_000), PriceTable::default());
        let b = [budget(BudgetScope::Global, None, 100.0, true)];
        assert_eq!(guard.evaluate(&b, &[], now()).await.unwrap(), BudgetDecision::Ok);
    }

    #[tokio::test]
    async fn over_soft_limit_warns() {
        // $75 spent, limit $50, not hard-stop → Warn.
        let guard = BudgetGuard::new(usage_returning(0, 1_000_000), PriceTable::default());
        let b = [budget(BudgetScope::Global, None, 50.0, false)];
        let d = guard.evaluate(&b, &[], now()).await.unwrap();
        assert!(matches!(d, BudgetDecision::Warn { .. }));
    }

    #[tokio::test]
    async fn over_hard_limit_stops_with_next_period() {
        let guard = BudgetGuard::new(usage_returning(0, 1_000_000), PriceTable::default());
        let b = [budget(BudgetScope::Global, None, 50.0, true)];
        let d = guard.evaluate(&b, &[], now()).await.unwrap();
        match d {
            BudgetDecision::Stop { retry_at, .. } => {
                assert_eq!(retry_at, Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap());
            }
            other => panic!("expected Stop, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn scoped_budget_ignored_when_scope_absent() {
        let app = Uuid::new_v4();
        let guard = BudgetGuard::new(usage_returning(0, 1_000_000), PriceTable::default());
        let b = [budget(BudgetScope::Application, Some(app), 1.0, true)];
        // No matching scope ref → budget does not apply → Ok (usage never summed).
        assert_eq!(guard.evaluate(&b, &[], now()).await.unwrap(), BudgetDecision::Ok);
        // With the matching scope ref it applies and stops.
        let d = guard
            .evaluate(&b, &[ScopeRef::new(BudgetScope::Application, Some(app))], now())
            .await
            .unwrap();
        assert!(matches!(d, BudgetDecision::Stop { .. }));
    }
}
