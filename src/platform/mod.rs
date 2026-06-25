//! The platform model: analysis schema, analyzer, writer, queries and graph.

pub mod analysis;
pub mod analyzer;
pub mod graph;
pub mod query;
pub mod writer;

pub use analysis::AnalysisResult;
pub use analyzer::{
    AnalysisError, AnalysisInput, FileAnalyzer, RepositoryAnalyzer,
};
pub use graph::{GraphQuery, GraphScope, PgGraphQuery, SqliteGraphQuery};
pub use query::{
    ListQuery, Page, PgPlatformQuery, PlatformQuery, SqlitePlatformQuery, is_entity,
};
pub use writer::{PgPlatformWriter, PlatformWriter, SqlitePlatformWriter};
