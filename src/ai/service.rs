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
        if form.provider_type == AiProviderType::Anthropic && secrets_enc.is_none() {
            return Err(AppError::BadRequest(
                "an API key is required for Anthropic API profiles".into(),
            ));
        }
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

    /// Pick the default profile id to analyse with: the first enabled profile,
    /// else the first configured one; `None` when no profiles exist.
    pub async fn default_profile_id(&self) -> Result<Option<Uuid>, AppError> {
        let profiles = self.list().await?;
        Ok(profiles
            .iter()
            .find(|p| p.enabled)
            .or_else(|| profiles.first())
            .map(|p| p.id))
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

    fn cli_profile() -> AiProfile {
        AiProfile {
            id: Uuid::new_v4(),
            name: "cli".into(),
            provider_type: AiProviderType::ClaudeCli,
            config: json!({ "binary_path": "claude" }),
            secrets_enc: None,
            enabled: true,
        }
    }

    /// AI deps whose command runner returns a fixed stdout.
    fn cli_deps(stdout: &'static str) -> AiProviderDeps {
        let mut runner = MockCommandRunner::new();
        runner.expect_run().returning(move |_| {
            Ok(crate::process::CommandOutput {
                status: 0,
                stdout: stdout.into(),
                stderr: String::new(),
            })
        });
        AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(runner),
            encryptor: Arc::new(MockEncryptor::new()),
        }
    }

    #[tokio::test]
    async fn get_list_delete_passthrough() {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_get().returning(|_| Ok(cli_profile()));
        repo.expect_list().returning(|| Ok(vec![cli_profile()]));
        repo.expect_delete().returning(|_| Ok(()));
        let svc = AiProfileService::new(Arc::new(repo), deps());
        assert!(svc.get(Uuid::new_v4()).await.is_ok());
        assert_eq!(svc.list().await.unwrap().len(), 1);
        assert!(svc.delete(Uuid::new_v4()).await.is_ok());
    }

    #[tokio::test]
    async fn update_reuses_existing_secret_when_key_blank() {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_get()
            .returning(|_| Ok(AiProfile { secrets_enc: Some(vec![7, 7]), ..cli_profile() }));
        repo.expect_update()
            .withf(|_, i: &AiProfileInput| i.secrets_enc.as_deref() == Some(&[7, 7][..]))
            .returning(|id, i| {
                Ok(AiProfile {
                    id,
                    name: i.name,
                    provider_type: i.provider_type,
                    config: i.config,
                    secrets_enc: i.secrets_enc,
                    enabled: i.enabled,
                })
            });
        let svc = AiProfileService::new(Arc::new(repo), deps());
        let form = ProfileForm {
            name: "x".into(),
            provider_type: AiProviderType::ClaudeCli,
            config: json!({ "binary_path": "claude" }),
            api_key: None,
            enabled: true,
        };
        assert!(svc.update(Uuid::new_v4(), form).await.is_ok());
    }

    #[tokio::test]
    async fn validate_runs_the_provider() {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_get().returning(|_| Ok(cli_profile()));
        let svc = AiProfileService::new(Arc::new(repo), cli_deps("claude 1.0\n"));
        assert!(svc.validate(Uuid::new_v4()).await.is_ok());
    }

    #[tokio::test]
    async fn test_prompt_returns_the_completion() {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_get().returning(|_| Ok(cli_profile()));
        let svc = AiProfileService::new(Arc::new(repo), cli_deps(r#"{"result":"hi there"}"#));
        let resp = svc.test_prompt(Uuid::new_v4(), "hello").await.unwrap();
        assert_eq!(resp.text, "hi there");
    }

    #[tokio::test]
    async fn create_anthropic_without_key_is_rejected() {
        // No repo call expected: validation fails before persistence.
        let svc = AiProfileService::new(Arc::new(MockAiProfileRepository::new()), deps());
        let form = ProfileForm {
            name: "a".into(),
            provider_type: AiProviderType::Anthropic,
            config: json!({}),
            api_key: None,
            enabled: true,
        };
        assert!(matches!(svc.create(form).await, Err(AppError::BadRequest(_))));
    }

    #[tokio::test]
    async fn update_anthropic_keeps_existing_key_when_blank() {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_get()
            .returning(|_| Ok(AiProfile { secrets_enc: Some(vec![9]), ..cli_profile() }));
        repo.expect_update().returning(|id, i| {
            Ok(AiProfile {
                id,
                name: i.name,
                provider_type: i.provider_type,
                config: i.config,
                secrets_enc: i.secrets_enc,
                enabled: i.enabled,
            })
        });
        let svc = AiProfileService::new(Arc::new(repo), deps());
        let form = ProfileForm {
            name: "a".into(),
            provider_type: AiProviderType::Anthropic,
            config: json!({}),
            api_key: None,
            enabled: true,
        };
        // Blank key is allowed because the stored profile already has a secret.
        assert!(svc.update(Uuid::new_v4(), form).await.is_ok());
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
