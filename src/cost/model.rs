//! Domain types for LLM usage, budgets and cost rollups (M39).

use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One recorded LLM call's usage (appended per call by the recorder).
#[derive(Debug, Clone)]
pub struct LlmUsageInput {
    pub job_execution_id: Uuid,
    pub application_id: Option<Uuid>,
    pub ai_profile_id: Option<Uuid>,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// Token totals for one model (a usage aggregation result).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelTokens {
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// Token totals for one (group key, model) pair. Rollups group by a dimension
/// *and* model so cost can be priced per model before aggregating by key.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupModelTokens {
    pub key: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// The dimension a cost rollup groups by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostDimension {
    Application,
    Profile,
    JobType,
}

/// What a budget applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetScope {
    Global,
    Profile,
    Job,
    Application,
}

impl BudgetScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            BudgetScope::Global => "global",
            BudgetScope::Profile => "profile",
            BudgetScope::Job => "job",
            BudgetScope::Application => "application",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "global" => Ok(BudgetScope::Global),
            "profile" => Ok(BudgetScope::Profile),
            "job" => Ok(BudgetScope::Job),
            "application" => Ok(BudgetScope::Application),
            other => Err(format!("unknown budget scope '{other}'")),
        }
    }
}

/// The budget's reset cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPeriod {
    Daily,
    Monthly,
}

impl BudgetPeriod {
    pub fn as_str(&self) -> &'static str {
        match self {
            BudgetPeriod::Daily => "daily",
            BudgetPeriod::Monthly => "monthly",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "daily" => Ok(BudgetPeriod::Daily),
            "monthly" => Ok(BudgetPeriod::Monthly),
            other => Err(format!("unknown budget period '{other}'")),
        }
    }

    /// Start of the current period containing `now` (UTC midnight / month start).
    pub fn start(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            BudgetPeriod::Daily => Utc
                .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
                .single()
                .unwrap_or(now),
            BudgetPeriod::Monthly => Utc
                .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .single()
                .unwrap_or(now),
        }
    }

    /// Start of the next period (the budget reset time → `CannotRun` retry).
    pub fn next_start(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            BudgetPeriod::Daily => self.start(now) + chrono::Duration::days(1),
            BudgetPeriod::Monthly => {
                let (y, m) = if now.month() == 12 {
                    (now.year() + 1, 1)
                } else {
                    (now.year(), now.month() + 1)
                };
                Utc.with_ymd_and_hms(y, m, 1, 0, 0, 0).single().unwrap_or(now)
            }
        }
    }
}

/// A stored spend budget.
#[derive(Debug, Clone)]
pub struct Budget {
    pub id: Uuid,
    pub scope: BudgetScope,
    pub scope_id: Option<Uuid>,
    pub period: BudgetPeriod,
    pub limit_usd: f64,
    pub hard_stop: bool,
}

/// Fields needed to create a budget.
#[derive(Debug, Clone)]
pub struct BudgetInput {
    pub scope: BudgetScope,
    pub scope_id: Option<Uuid>,
    pub period: BudgetPeriod,
    pub limit_usd: f64,
    pub hard_stop: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_and_period_round_trip() {
        for s in ["global", "profile", "job", "application"] {
            assert_eq!(BudgetScope::parse(s).unwrap().as_str(), s);
        }
        for p in ["daily", "monthly"] {
            assert_eq!(BudgetPeriod::parse(p).unwrap().as_str(), p);
        }
        assert!(BudgetScope::parse("nope").is_err());
        assert!(BudgetPeriod::parse("yearly").is_err());
    }

    #[test]
    fn period_start_truncates() {
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 13, 45, 0).unwrap();
        assert_eq!(BudgetPeriod::Daily.start(now), Utc.with_ymd_and_hms(2026, 6, 15, 0, 0, 0).unwrap());
        assert_eq!(BudgetPeriod::Monthly.start(now), Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap());
    }

    #[test]
    fn next_start_rolls_over() {
        let dec = Utc.with_ymd_and_hms(2026, 12, 20, 0, 0, 0).unwrap();
        assert_eq!(BudgetPeriod::Monthly.next_start(dec), Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap());
        let day = Utc.with_ymd_and_hms(2026, 6, 15, 9, 0, 0).unwrap();
        assert_eq!(BudgetPeriod::Daily.next_start(day), Utc.with_ymd_and_hms(2026, 6, 16, 0, 0, 0).unwrap());
    }
}
