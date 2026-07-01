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

/// The platform default model when a profile does not pin one.
pub const DEFAULT_MODEL: &str = "claude-opus-4-8";

impl AiProfile {
    /// The model id this profile uses (from its `config.model`), defaulting to
    /// [`DEFAULT_MODEL`]. Used to attribute and price LLM usage (M39).
    pub fn model(&self) -> String {
        self.config
            .get("model")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_MODEL)
            .to_string()
    }
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
    /// Directory the provider should run in (agentic providers read it; the
    /// HTTP API provider ignores it).
    pub working_dir: Option<String>,
}

impl AiRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            system: None,
            prompt: prompt.into(),
            working_dir: None,
        }
    }

    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
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
