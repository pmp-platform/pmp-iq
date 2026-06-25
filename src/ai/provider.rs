//! The AI provider strategy trait.

use super::model::{AiRequest, AiResponse};
use async_trait::async_trait;

/// Errors raised by AI providers.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("authentication failed")]
    Auth,
    #[error("provider request failed: {0}")]
    Request(String),
    #[error("could not parse provider response: {0}")]
    Parse(String),
    #[error("misconfigured profile: {0}")]
    Config(String),
}

/// A strategy that turns prompts into completions.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Run a completion.
    async fn complete(&self, request: AiRequest) -> Result<AiResponse, AiError>;

    /// Check that the provider is usable (credentials/binary present).
    async fn validate(&self) -> Result<(), AiError>;
}
