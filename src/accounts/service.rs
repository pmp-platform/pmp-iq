//! Application service for repository accounts: encrypts credentials and drives
//! providers for validation and repository preview.

use super::model::{
    AccountInput, AuthType, ProviderType, RemoteRepo, RepositoryAccount, SelectionMode,
};
use super::providers::{
    ProviderDeps, PullRequest, PullRequestSpec, RepoMember, RepositoryProviderFactory,
};
use super::repository::RepositoryAccountRepository;
use super::selector::RepoSelector;
use crate::error::AppError;
use crate::strings::blank_to_none;
use std::sync::Arc;
use uuid::Uuid;

/// Operator-supplied account fields (token is plaintext, encrypted here).
#[derive(Clone)]
pub struct AccountForm {
    pub name: String,
    pub provider_type: ProviderType,
    pub auth_type: AuthType,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub selection_mode: SelectionMode,
    pub selection_value: Option<String>,
    pub enabled: bool,
}

/// Orchestrates account persistence and provider operations.
#[derive(Clone)]
pub struct AccountService {
    repo: Arc<dyn RepositoryAccountRepository>,
    deps: ProviderDeps,
}

impl AccountService {
    pub fn new(repo: Arc<dyn RepositoryAccountRepository>, deps: ProviderDeps) -> Self {
        Self { repo, deps }
    }

    fn encrypt_token(&self, token: &str) -> Result<Vec<u8>, AppError> {
        self.deps
            .encryptor
            .encrypt(token.as_bytes())
            .map_err(|e| AppError::internal(format!("encrypt credential: {e}")))
    }

    /// Build a persistence input, encrypting a fresh token or keeping existing
    /// credentials when the token field is left blank.
    fn to_input(
        &self,
        form: AccountForm,
        existing: Option<Vec<u8>>,
    ) -> Result<AccountInput, AppError> {
        let credentials_enc = match form.token.as_deref() {
            Some(token) if !token.is_empty() => Some(self.encrypt_token(token)?),
            _ => existing,
        };
        Ok(AccountInput {
            name: form.name,
            provider_type: form.provider_type,
            auth_type: form.auth_type,
            base_url: blank_to_none(form.base_url),
            credentials_enc,
            selection_mode: form.selection_mode,
            selection_value: blank_to_none(form.selection_value),
            enabled: form.enabled,
        })
    }

    pub async fn create(&self, form: AccountForm) -> Result<RepositoryAccount, AppError> {
        let input = self.to_input(form, None)?;
        Ok(self.repo.create(input).await?)
    }

    pub async fn update(
        &self,
        id: Uuid,
        form: AccountForm,
    ) -> Result<RepositoryAccount, AppError> {
        let existing = self.repo.get(id).await?;
        let input = self.to_input(form, existing.credentials_enc)?;
        Ok(self.repo.update(id, input).await?)
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), AppError> {
        Ok(self.repo.delete(id).await?)
    }

    pub async fn get(&self, id: Uuid) -> Result<RepositoryAccount, AppError> {
        Ok(self.repo.get(id).await?)
    }

    pub async fn list(&self) -> Result<Vec<RepositoryAccount>, AppError> {
        Ok(self.repo.list().await?)
    }

    /// Enabled accounts only (used by the review job).
    pub async fn list_enabled(&self) -> Result<Vec<RepositoryAccount>, AppError> {
        Ok(self.repo.list_enabled().await?)
    }

    /// Decrypt the clone credential (token) for an account, if present.
    pub fn clone_token(&self, account: &RepositoryAccount) -> Result<Option<String>, AppError> {
        match &account.credentials_enc {
            None => Ok(None),
            Some(enc) => {
                let bytes = self
                    .deps
                    .encryptor
                    .decrypt(enc)
                    .map_err(|e| AppError::internal(format!("decrypt credential: {e}")))?;
                let token =
                    String::from_utf8(bytes).map_err(|e| AppError::internal(e.to_string()))?;
                Ok(Some(token))
            }
        }
    }

    /// Validate an account's credentials/configuration via its provider.
    pub async fn validate(&self, id: Uuid) -> Result<(), AppError> {
        let account = self.repo.get(id).await?;
        let provider = RepositoryProviderFactory::build(&account, &self.deps)
            .map_err(AppError::from)?;
        provider.validate().await.map_err(AppError::from)
    }

    /// List the repositories an account would select.
    pub async fn preview(&self, id: Uuid) -> Result<Vec<RemoteRepo>, AppError> {
        let account = self.repo.get(id).await?;
        self.select_for(&account).await
    }

    /// List the members/collaborators of a repository via the account's
    /// provider (empty for providers without a member concept).
    pub async fn members_for(
        &self,
        account: &RepositoryAccount,
        repo_full_name: &str,
    ) -> Result<Vec<RepoMember>, AppError> {
        let provider =
            RepositoryProviderFactory::build(account, &self.deps).map_err(AppError::from)?;
        provider
            .list_members(repo_full_name)
            .await
            .map_err(AppError::from)
    }

    /// Open (or reuse) a pull request via the account's provider. Returns a
    /// clear error for providers that cannot open PRs.
    pub async fn open_pull_request(
        &self,
        account: &RepositoryAccount,
        spec: PullRequestSpec,
    ) -> Result<PullRequest, AppError> {
        let provider =
            RepositoryProviderFactory::build(account, &self.deps).map_err(AppError::from)?;
        provider.open_pull_request(spec).await.map_err(AppError::from)
    }

    /// Resolve the selected repositories for a given account (reused by jobs).
    pub async fn select_for(
        &self,
        account: &RepositoryAccount,
    ) -> Result<Vec<RemoteRepo>, AppError> {
        let provider = RepositoryProviderFactory::build(account, &self.deps)
            .map_err(AppError::from)?;
        let all = provider.list_repositories().await.map_err(AppError::from)?;
        RepoSelector::select(
            account.selection_mode,
            account.selection_value.as_deref(),
            all,
        )
        .map_err(|e| AppError::BadRequest(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::repository::MockRepositoryAccountRepository;
    use crate::crypto::MockEncryptor;
    use crate::fs::MockFileSystem;
    use crate::httpclient::MockHttpClient;

    fn deps_with_encrypt() -> ProviderDeps {
        let mut enc = MockEncryptor::new();
        enc.expect_encrypt().returning(|_| Ok(vec![9, 9, 9]));
        ProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            fs: Arc::new(MockFileSystem::new()),
            encryptor: Arc::new(enc),
        }
    }

    fn form() -> AccountForm {
        AccountForm {
            name: "gh".into(),
            provider_type: ProviderType::Github,
            auth_type: AuthType::Token,
            base_url: None,
            token: Some("ghp_x".into()),
            selection_mode: SelectionMode::All,
            selection_value: None,
            enabled: true,
        }
    }

    use crate::httpclient::HttpResponse;

    /// Provider deps with a configurable HTTP client and a token-decrypting
    /// encryptor.
    fn deps_with_http(http: MockHttpClient) -> ProviderDeps {
        let mut enc = MockEncryptor::new();
        enc.expect_decrypt().returning(|_| Ok(b"ghp_token".to_vec()));
        ProviderDeps {
            http: Arc::new(http),
            fs: Arc::new(MockFileSystem::new()),
            encryptor: Arc::new(enc),
        }
    }

    fn github_account() -> RepositoryAccount {
        RepositoryAccount {
            id: Uuid::new_v4(),
            name: "gh".into(),
            provider_type: ProviderType::Github,
            auth_type: AuthType::Token,
            base_url: None,
            credentials_enc: Some(vec![1]),
            selection_mode: SelectionMode::All,
            selection_value: None,
            enabled: true,
        }
    }

    #[tokio::test]
    async fn validate_calls_the_provider() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| Ok(HttpResponse::new(200, "{}")));
        let mut repo = MockRepositoryAccountRepository::new();
        repo.expect_get().returning(|_| Ok(github_account()));
        let svc = AccountService::new(Arc::new(repo), deps_with_http(http));
        assert!(svc.validate(Uuid::new_v4()).await.is_ok());
    }

    #[tokio::test]
    async fn preview_lists_selected_repositories() {
        let mut http = MockHttpClient::new();
        let mut call = 0;
        http.expect_send().returning(move |_| {
            call += 1;
            Ok(if call == 1 {
                HttpResponse::new(
                    200,
                    r#"[{"name":"api","full_name":"org/api","clone_url":"u","default_branch":"main","private":false}]"#,
                )
            } else {
                HttpResponse::new(200, "[]")
            })
        });
        let mut repo = MockRepositoryAccountRepository::new();
        repo.expect_get().returning(|_| Ok(github_account()));
        let svc = AccountService::new(Arc::new(repo), deps_with_http(http));
        assert_eq!(svc.preview(Uuid::new_v4()).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn members_for_lists_collaborators() {
        let mut http = MockHttpClient::new();
        let mut call = 0;
        http.expect_send().returning(move |_| {
            call += 1;
            Ok(if call == 1 {
                HttpResponse::new(200, r#"[{"login":"alice","role_name":"admin"}]"#)
            } else {
                HttpResponse::new(200, "[]")
            })
        });
        let svc = AccountService::new(
            Arc::new(MockRepositoryAccountRepository::new()),
            deps_with_http(http),
        );
        let members = svc.members_for(&github_account(), "org/api").await.unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].username, "alice");
    }

    #[tokio::test]
    async fn clone_token_decrypts_stored_credential() {
        let svc = AccountService::new(
            Arc::new(MockRepositoryAccountRepository::new()),
            deps_with_http(MockHttpClient::new()),
        );
        assert_eq!(svc.clone_token(&github_account()).unwrap().as_deref(), Some("ghp_token"));
    }

    #[tokio::test]
    async fn get_list_delete_passthrough() {
        let mut repo = MockRepositoryAccountRepository::new();
        repo.expect_get().returning(|_| Ok(github_account()));
        repo.expect_list().returning(|| Ok(vec![github_account()]));
        repo.expect_list_enabled().returning(|| Ok(vec![]));
        repo.expect_delete().returning(|_| Ok(()));
        let svc = AccountService::new(Arc::new(repo), deps_with_http(MockHttpClient::new()));
        assert!(svc.get(Uuid::new_v4()).await.is_ok());
        assert_eq!(svc.list().await.unwrap().len(), 1);
        assert!(svc.list_enabled().await.unwrap().is_empty());
        assert!(svc.delete(Uuid::new_v4()).await.is_ok());
    }

    #[tokio::test]
    async fn create_encrypts_token() {
        let mut repo = MockRepositoryAccountRepository::new();
        repo.expect_create().withf(|input: &AccountInput| {
            input.credentials_enc.as_deref() == Some(&[9, 9, 9][..])
        }).returning(|input| {
            Ok(RepositoryAccount {
                id: Uuid::new_v4(),
                name: input.name,
                provider_type: input.provider_type,
                auth_type: input.auth_type,
                base_url: input.base_url,
                credentials_enc: input.credentials_enc,
                selection_mode: input.selection_mode,
                selection_value: input.selection_value,
                enabled: input.enabled,
            })
        });
        let service = AccountService::new(Arc::new(repo), deps_with_encrypt());
        let created = service.create(form()).await.unwrap();
        assert_eq!(created.credentials_enc, Some(vec![9, 9, 9]));
    }

    #[tokio::test]
    async fn update_keeps_existing_credentials_when_token_blank() {
        let id = Uuid::new_v4();
        let mut repo = MockRepositoryAccountRepository::new();
        repo.expect_get().returning(move |_| {
            Ok(RepositoryAccount {
                id,
                name: "gh".into(),
                provider_type: ProviderType::Github,
                auth_type: AuthType::Token,
                base_url: None,
                credentials_enc: Some(vec![1, 2, 3]),
                selection_mode: SelectionMode::All,
                selection_value: None,
                enabled: true,
            })
        });
        repo.expect_update()
            .withf(|_, input: &AccountInput| input.credentials_enc.as_deref() == Some(&[1, 2, 3][..]))
            .returning(|id, input| {
                Ok(RepositoryAccount {
                    id,
                    name: input.name,
                    provider_type: input.provider_type,
                    auth_type: input.auth_type,
                    base_url: input.base_url,
                    credentials_enc: input.credentials_enc,
                    selection_mode: input.selection_mode,
                    selection_value: input.selection_value,
                    enabled: input.enabled,
                })
            });

        let mut blank = form();
        blank.token = None;
        let service = AccountService::new(Arc::new(repo), deps_with_encrypt());
        let updated = service.update(id, blank).await.unwrap();
        assert_eq!(updated.credentials_enc, Some(vec![1, 2, 3]));
    }
}
