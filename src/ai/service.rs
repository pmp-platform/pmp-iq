//! Application service for AI agent profiles: encrypts secrets and drives
//! providers for validation and test prompts.

use super::factory::{AiProviderDeps, AiProviderFactory};
use super::model::{AiProfile, AiProfileInput, AiProviderType, AiRequest, AiResponse};
use super::repository::AiProfileRepository;
use crate::error::AppError;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

/// Operator-supplied profile fields (api_key plaintext, encrypted here).
#[derive(Clone)]
pub struct ProfileForm {
    pub name: String,
    pub provider_type: AiProviderType,
    pub config: Value,
    pub api_key: Option<String>,
    pub enabled: bool,
}

/// Orchestrates profile persistence and provider operations.
#[derive(Clone)]
pub struct AiProfileService {
    repo: Arc<dyn AiProfileRepository>,
    deps: AiProviderDeps,
}

impl AiProfileService {
    pub fn new(repo: Arc<dyn AiProfileRepository>, deps: AiProviderDeps) -> Self {
        Self { repo, deps }
    }

    fn encrypt(&self, secret: &str) -> Result<Vec<u8>, AppError> {
        self.deps
            .encryptor
            .encrypt(secret.as_bytes())
            .map_err(|e| AppError::internal(format!("encrypt secret: {e}")))
    }

    fn to_input(
        &self,
        form: ProfileForm,
        existing: Option<Vec<u8>>,
    ) -> Result<AiProfileInput, AppError> {
        let secrets_enc = match form.api_key.as_deref() {
            Some(key) if !key.is_empty() => Some(self.encrypt(key)?),
            _ => existing,
        };
        Ok(AiProfileInput {
            name: form.name,
            provider_type: form.provider_type,
            config: form.config,
            secrets_enc,
            enabled: form.enabled,
        })
    }

    pub async fn create(&self, form: ProfileForm) -> Result<AiProfile, AppError> {
        let input = self.to_input(form, None)?;
        Ok(self.repo.create(input).await?)
    }

    pub async fn update(&self, id: Uuid, form: ProfileForm) -> Result<AiProfile, AppError> {
        let existing = self.repo.get(id).await?;
        let input = self.to_input(form, existing.secrets_enc)?;
        Ok(self.repo.update(id, input).await?)
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), AppError> {
        Ok(self.repo.delete(id).await?)
    }

    pub async fn get(&self, id: Uuid) -> Result<AiProfile, AppError> {
        Ok(self.repo.get(id).await?)
    }

    pub async fn list(&self) -> Result<Vec<AiProfile>, AppError> {
        Ok(self.repo.list().await?)
    }

    /// Validate a profile's credentials/binary via its provider.
    pub async fn validate(&self, id: Uuid) -> Result<(), AppError> {
        let profile = self.repo.get(id).await?;
        let provider = AiProviderFactory::build(&profile, &self.deps)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        provider
            .validate()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))
    }

    /// Run a test prompt through a profile.
    pub async fn test_prompt(&self, id: Uuid, prompt: &str) -> Result<AiResponse, AppError> {
        let profile = self.repo.get(id).await?;
        self.complete(&profile, AiRequest::new(prompt)).await
    }

    /// Run a completion against a given profile (reused by jobs).
    pub async fn complete(
        &self,
        profile: &AiProfile,
        request: AiRequest,
    ) -> Result<AiResponse, AppError> {
        let provider = AiProviderFactory::build(profile, &self.deps)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        provider
            .complete(request)
            .await
            .map_err(AppError::internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::repository::MockAiProfileRepository;
    use crate::crypto::MockEncryptor;
    use crate::httpclient::MockHttpClient;
    use crate::process::MockCommandRunner;
    use serde_json::json;

    fn deps() -> AiProviderDeps {
        let mut enc = MockEncryptor::new();
        enc.expect_encrypt().returning(|_| Ok(vec![1, 2, 3]));
        AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(MockCommandRunner::new()),
            encryptor: Arc::new(enc),
        }
    }

    #[tokio::test]
    async fn create_encrypts_api_key() {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_create()
            .withf(|i: &AiProfileInput| i.secrets_enc.as_deref() == Some(&[1, 2, 3][..]))
            .returning(|i| {
                Ok(AiProfile {
                    id: Uuid::new_v4(),
                    name: i.name,
                    provider_type: i.provider_type,
                    config: i.config,
                    secrets_enc: i.secrets_enc,
                    enabled: i.enabled,
                })
            });
        let service = AiProfileService::new(Arc::new(repo), deps());
        let form = ProfileForm {
            name: "a".into(),
            provider_type: AiProviderType::Anthropic,
            config: json!({}),
            api_key: Some("sk-test".into()),
            enabled: true,
        };
        let created = service.create(form).await.unwrap();
        assert_eq!(created.secrets_enc, Some(vec![1, 2, 3]));
    }
}
