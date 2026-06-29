//! Batch-change campaigns (M30): a named change applied across many repositories
//! via one multi-repo agent task, with filter-based selection and tracking.

pub mod model;
pub mod repository;

pub use model::{Campaign, NewCampaign};
pub use repository::{CampaignRepository, PgCampaignRepository, SqliteCampaignRepository};
