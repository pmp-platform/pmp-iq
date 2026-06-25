//! AI agent profiles: providers (Anthropic API, Claude CLI), persistence and
//! the orchestrating service.

pub mod anthropic;
pub mod claude_cli;
pub mod factory;
pub mod model;
pub mod provider;
pub mod repository;
pub mod service;

pub use factory::{AiProviderDeps, AiProviderFactory};
pub use model::{AiProfile, AiProfileInput, AiProviderType, AiRequest, AiResponse};
pub use provider::{AiError, AiProvider};
pub use repository::{AiProfileRepository, PgAiProfileRepository, SqliteAiProfileRepository};
pub use service::{AiProfileService, ProfileForm};
