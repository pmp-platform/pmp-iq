//! The platform model: analysis schema, analyzer, writer, queries and graph.

pub mod analysis;
pub mod analyzer;
pub mod api_endpoints;
pub mod catalog;
pub mod changes;
pub mod graph;
pub mod linked;
pub mod prompts;
pub mod query;
pub mod writer;

pub use api_endpoints::{
    ApiEndpointRepository, Consumer, Endpoint, PgApiEndpointRepository, SqliteApiEndpointRepository,
};
pub use changes::{
    Change, ChangeKind, ChangeRow, PgPlatformChangeRepository, PlatformChangeRepository,
    SqlitePlatformChangeRepository,
};

pub use analysis::{AnalysisConfig, AnalysisResult, KindDef, MemberInfo, PropertyDef};
pub use prompts::{PromptConfig, PromptSection};
pub use catalog::{Catalog, CatalogEntry};
pub use analyzer::{
    AnalysisError, AnalysisInput, FileAnalyzer, RepositoryAnalyzer,
};
pub use graph::{GraphQuery, GraphScope, PgGraphQuery, SqliteGraphQuery};
pub use linked::{LINKED, LinkedEntity, linked};
pub use query::{
    EmbeddingSourceRow, ListQuery, Page, PgPlatformQuery, PlatformQuery, SqlitePlatformQuery,
    filter_fields, is_entity,
};
pub use writer::{PgPlatformWriter, PlatformWriter, SqlitePlatformWriter};
