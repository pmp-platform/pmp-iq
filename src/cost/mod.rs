//! LLM cost & token budgeting (M39): turns recorded token usage into priced
//! cost (configurable per-model price map), aggregates spend, and enforces
//! budgets that warn or hard-stop work past a limit.

pub mod guard;
pub mod model;
pub mod pricing;
pub mod repository;

pub use guard::{BudgetDecision, BudgetGuard, ScopeRef};
pub use model::{
    Budget, BudgetInput, BudgetPeriod, BudgetScope, CostDimension, GroupModelTokens, LlmUsageInput,
    ModelTokens,
};
pub use pricing::{CostRow, ModelPrice, PriceTable, cost, price_rows};
pub use repository::{
    LlmBudgetRepository, LlmUsageRepository, PgLlmBudgetRepository, PgLlmUsageRepository,
    SqliteLlmBudgetRepository, SqliteLlmUsageRepository,
};
