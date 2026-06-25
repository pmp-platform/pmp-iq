//! Builds the right [`AiProvider`] from a stored profile.

use super::anthropic::{AnthropicConfig, AnthropicProvider};
use super::claude_cli::{ClaudeCliConfig, ClaudeCliProvider};
use super::model::{AiProfile, AiProviderType};
use super::provider::{AiError, AiProvider};
use crate::crypto::Encryptor;
use crate::httpclient::HttpClient;
use crate::process::CommandRunner;
use std::sync::Arc;

/// Shared dependencies needed to construct AI providers.
#[derive(Clone)]
pub struct AiProviderDeps {
    pub http: Arc<dyn HttpClient>,
    pub runner: Arc<dyn CommandRunner>,
    pub encryptor: Arc<dyn Encryptor>,
}

/// Constructs AI providers, decrypting stored secrets as needed.
pub struct AiProviderFactory;

impl AiProviderFactory {
    pub fn build(
        profile: &AiProfile,
        deps: &AiProviderDeps,
    ) -> Result<Box<dyn AiProvider>, AiError> {
        match profile.provider_type {
            AiProviderType::Anthropic => {
                let config: AnthropicConfig = serde_json::from_value(profile.config.clone())
                    .map_err(|e| AiError::Config(e.to_string()))?;
                let api_key = Self::decrypt_secret(profile, deps)?
                    .ok_or_else(|| AiError::Config("anthropic profile requires an API key".into()))?;
                Ok(Box::new(AnthropicProvider::new(deps.http.clone(), api_key, config)))
            }
            AiProviderType::ClaudeCli => {
                let config: ClaudeCliConfig = serde_json::from_value(profile.config.clone())
                    .map_err(|e| AiError::Config(e.to_string()))?;
                Ok(Box::new(ClaudeCliProvider::new(deps.runner.clone(), config)))
            }
        }
    }

    fn decrypt_secret(
        profile: &AiProfile,
        deps: &AiProviderDeps,
    ) -> Result<Option<String>, AiError> {
        match &profile.secrets_enc {
            None => Ok(None),
            Some(enc) => {
                let bytes = deps
                    .encryptor
                    .decrypt(enc)
                    .map_err(|e| AiError::Config(format!("secret decrypt failed: {e}")))?;
                let secret =
                    String::from_utf8(bytes).map_err(|e| AiError::Config(e.to_string()))?;
                Ok(Some(secret))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::MockEncryptor;
    use crate::httpclient::MockHttpClient;
    use crate::process::MockCommandRunner;
    use serde_json::json;
    use uuid::Uuid;

    fn deps(secret: &'static [u8]) -> AiProviderDeps {
        let mut enc = MockEncryptor::new();
        enc.expect_decrypt().returning(move |_| Ok(secret.to_vec()));
        AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(MockCommandRunner::new()),
            encryptor: Arc::new(enc),
        }
    }

    #[test]
    fn builds_claude_cli_without_secret() {
        let profile = AiProfile {
            id: Uuid::new_v4(),
            name: "cli".into(),
            provider_type: AiProviderType::ClaudeCli,
            config: json!({ "binary_path": "claude" }),
            secrets_enc: None,
            enabled: true,
        };
        assert!(AiProviderFactory::build(&profile, &deps(b"")).is_ok());
    }

    #[test]
    fn anthropic_without_secret_is_config_error() {
        let profile = AiProfile {
            id: Uuid::new_v4(),
            name: "a".into(),
            provider_type: AiProviderType::Anthropic,
            config: json!({}),
            secrets_enc: None,
            enabled: true,
        };
        let deps = AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(MockCommandRunner::new()),
            encryptor: Arc::new(MockEncryptor::new()),
        };
        assert!(matches!(
            AiProviderFactory::build(&profile, &deps),
            Err(AiError::Config(_))
        ));
    }
}
