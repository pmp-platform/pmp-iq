//! The platform model: analysis schema, analyzer, writer, queries and graph.

pub mod analysis;
pub mod analyzer;
pub mod catalog;
pub mod graph;
pub mod linked;
pub mod query;
pub mod writer;

pub use analysis::{AnalysisConfig, AnalysisResult, KindDef, MemberInfo, PropertyDef};
pub use catalog::{Catalog, CatalogEntry};
pub use analyzer::{
    AnalysisError, AnalysisInput, FileAnalyzer, RepositoryAnalyzer,
};
pub use graph::{GraphQuery, GraphScope, PgGraphQuery, SqliteGraphQuery};
pub use linked::{LINKED, LinkedEntity, linked};
pub use query::{
    ListQuery, Page, PgPlatformQuery, PlatformQuery, SqlitePlatformQuery, filter_fields, is_entity,
};
pub use writer::{PgPlatformWriter, PlatformWriter, SqlitePlatformWriter};
