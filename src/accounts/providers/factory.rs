//! Builds the right [`RepositoryProvider`] from a stored account.

use super::{GitHubProvider, GitLabProvider, LocalProvider, ProviderError, RepositoryProvider};
use crate::accounts::model::{ProviderType, RepositoryAccount};
use crate::crypto::Encryptor;
use crate::fs::FileSystem;
use crate::httpclient::HttpClient;
use std::sync::Arc;

/// Shared dependencies needed to construct providers.
#[derive(Clone)]
pub struct ProviderDeps {
    pub http: Arc<dyn HttpClient>,
    pub fs: Arc<dyn FileSystem>,
    pub encryptor: Arc<dyn Encryptor>,
}

/// Constructs providers, decrypting stored credentials as needed.
pub struct RepositoryProviderFactory;

impl RepositoryProviderFactory {
    pub fn build(
        account: &RepositoryAccount,
        deps: &ProviderDeps,
    ) -> Result<Box<dyn RepositoryProvider>, ProviderError> {
        let token = Self::decrypt_token(account, deps)?;
        match account.provider_type {
            ProviderType::Github => Ok(Box::new(GitHubProvider::new(
                deps.http.clone(),
                token,
                account.base_url.clone(),
            ))),
            ProviderType::Gitlab => Ok(Box::new(GitLabProvider::new(
                deps.http.clone(),
                token,
                account.base_url.clone(),
            ))),
            ProviderType::Local => {
                let path = account.base_url.clone().ok_or_else(|| {
                    ProviderError::Config("local account requires a base_url path".into())
                })?;
                Ok(Box::new(LocalProvider::new(deps.fs.clone(), path)))
            }
        }
    }

    fn decrypt_token(
        account: &RepositoryAccount,
        deps: &ProviderDeps,
    ) -> Result<Option<String>, ProviderError> {
        match &account.credentials_enc {
            None => Ok(None),
            Some(enc) => {
                let bytes = deps
                    .encryptor
                    .decrypt(enc)
                    .map_err(|e| ProviderError::Config(format!("credential decrypt failed: {e}")))?;
                let token =
                    String::from_utf8(bytes).map_err(|e| ProviderError::Config(e.to_string()))?;
                Ok(Some(token))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::model::{AuthType, SelectionMode};
    use crate::crypto::MockEncryptor;
    use crate::fs::MockFileSystem;
    use crate::httpclient::MockHttpClient;
    use uuid::Uuid;

    fn deps_with_token(token: &'static [u8]) -> ProviderDeps {
        let mut enc = MockEncryptor::new();
        enc.expect_decrypt().returning(move |_| Ok(token.to_vec()));
        ProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            fs: Arc::new(MockFileSystem::new()),
            encryptor: Arc::new(enc),
        }
    }

    fn account(provider: ProviderType, base_url: Option<&str>) -> RepositoryAccount {
        RepositoryAccount {
            id: Uuid::new_v4(),
            name: "acc".into(),
            provider_type: provider,
            auth_type: AuthType::Token,
            base_url: base_url.map(String::from),
            credentials_enc: Some(vec![1, 2, 3]),
            selection_mode: SelectionMode::All,
            selection_value: None,
            enabled: true,
        }
    }

    #[test]
    fn builds_github_provider() {
        let deps = deps_with_token(b"ghp_x");
        let provider = RepositoryProviderFactory::build(&account(ProviderType::Github, None), &deps);
        assert!(provider.is_ok());
    }

    #[test]
    fn local_without_path_is_config_error() {
        let deps = deps_with_token(b"");
        let result = RepositoryProviderFactory::build(&account(ProviderType::Local, None), &deps);
        assert!(matches!(result, Err(ProviderError::Config(_))));
    }
}
