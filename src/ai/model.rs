//! Shared request/response types for AI providers.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// A stored AI agent profile (secrets stay encrypted on the row).
#[derive(Debug, Clone)]
pub struct AiProfile {
    pub id: Uuid,
    pub name: String,
    pub provider_type: AiProviderType,
    pub config: Value,
    pub secrets_enc: Option<Vec<u8>>,
    pub enabled: bool,
}

/// Fields needed to create or update a profile (pre-encrypted secrets).
#[derive(Debug, Clone)]
pub struct AiProfileInput {
    pub name: String,
    pub provider_type: AiProviderType,
    pub config: Value,
    pub secrets_enc: Option<Vec<u8>>,
    pub enabled: bool,
}

/// A completion request. Bundled into one struct to bound parameter count.
#[derive(Debug, Clone)]
pub struct AiRequest {
    pub system: Option<String>,
    pub prompt: String,
}

impl AiRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            system: None,
            prompt: prompt.into(),
        }
    }

    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }
}

/// A completion response plus usage metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub text: String,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

/// Kind of AI provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderType {
    Anthropic,
    ClaudeCli,
}

impl AiProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AiProviderType::Anthropic => "anthropic",
            AiProviderType::ClaudeCli => "claude_cli",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "anthropic" => Ok(AiProviderType::Anthropic),
            "claude_cli" => Ok(AiProviderType::ClaudeCli),
            other => Err(format!("unknown ai provider '{other}'")),
        }
    }
}
