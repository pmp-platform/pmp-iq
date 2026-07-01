//! DORA metrics (M47): capture deployment + incident events and derive the four
//! DORA measures (deployment frequency, lead time, change-failure rate, MTTR)
//! with a performance tier, per application / team / fleet.

pub mod compute;
pub mod model;
pub mod repository;

pub use compute::compute;
pub use model::{Deployment, DoraSummary, Incident, NewDeployment};
pub use repository::{DoraRepository, PgDoraRepository, SqliteDoraRepository};
