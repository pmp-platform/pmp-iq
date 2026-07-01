//! Auto-remediation (M46): turn findings (metrics, scorecards, currency, …) into
//! agent-task / campaign change requests, gated by rules + an approval step.

pub mod repository;
pub mod service;
pub mod trigger;

pub use repository::{
    PgRemediationRepository, Remediation, RemediationRepository, RemediationRule, RuleInput,
    SqliteRemediationRepository,
};
pub use service::RemediationService;
pub use trigger::{AppSignals, trigger_matches};
