//! User-configured analysis vocabulary: allowed entity kinds and the properties
//! to extract per entity type. Backs the Settings "Entity kinds" / "Properties"
//! tabs and feeds the analyzer's prompt + kind normalization.

pub mod model;
pub mod prompt_repository;
pub mod repository;
pub mod service;

pub use model::{
    DataType, EntityKind, EntityKindInput, EntityProperty, EntityPropertyInput,
};
pub use prompt_repository::{
    ExtractionPromptRepository, PgExtractionPromptRepository, SqliteExtractionPromptRepository,
    StoredPrompt,
};
pub use repository::{
    EntityKindRepository, EntityPropertyRepository, PgEntityKindRepository,
    PgEntityPropertyRepository, SqliteEntityKindRepository, SqliteEntityPropertyRepository,
};
pub use service::AnalysisConfigService;
