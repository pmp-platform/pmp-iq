//! Discovered/cloned repository records.

pub mod model;
pub mod repository;

pub use model::{RepoRecord, RepoRecordInput};
pub use repository::{
    PgRepoRecordRepository, RepoRecordRepository, SqliteRepoRecordRepository,
};
