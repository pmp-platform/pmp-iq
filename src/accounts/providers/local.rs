//! Local filesystem repository provider: each git working copy under a base
//! directory is treated as a repository.

use super::{ProviderError, RepositoryProvider};
use crate::accounts::model::RemoteRepo;
use crate::fs::FileSystem;
use async_trait::async_trait;
use std::sync::Arc;

/// Discovers repositories under a base directory on the local filesystem.
pub struct LocalProvider {
    fs: Arc<dyn FileSystem>,
    base_path: String,
}

impl LocalProvider {
    pub fn new(fs: Arc<dyn FileSystem>, base_path: String) -> Self {
        Self { fs, base_path }
    }

    fn join(&self, child: &str) -> String {
        format!("{}/{}", self.base_path.trim_end_matches('/'), child)
    }
}

#[async_trait]
impl RepositoryProvider for LocalProvider {
    async fn validate(&self) -> Result<(), ProviderError> {
        if self.fs.exists(&self.base_path) {
            Ok(())
        } else {
            Err(ProviderError::Config(format!(
                "path does not exist: {}",
                self.base_path
            )))
        }
    }

    async fn list_repositories(&self) -> Result<Vec<RemoteRepo>, ProviderError> {
        let subdirs = self
            .fs
            .list_subdirs(&self.base_path)
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        let mut out = Vec::new();
        for dir in subdirs {
            let repo_path = self.join(&dir);
            if self.fs.exists(&format!("{repo_path}/.git")) {
                out.push(RemoteRepo {
                    name: dir.clone(),
                    full_name: dir,
                    clone_url: repo_path,
                    default_branch: None,
                    private: true,
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFileSystem;

    #[tokio::test]
    async fn lists_only_git_working_copies() {
        let mut fs = MockFileSystem::new();
        fs.expect_list_subdirs()
            .returning(|_| Ok(vec!["api".into(), "notes".into()]));
        fs.expect_exists().returning(|p: &str| p.ends_with("api/.git"));

        let provider = LocalProvider::new(Arc::new(fs), "/repos".into());
        let repos = provider.list_repositories().await.unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "api");
        assert_eq!(repos[0].clone_url, "/repos/api");
    }

    #[tokio::test]
    async fn default_get_repository_scans_the_listing() {
        let mut fs = MockFileSystem::new();
        fs.expect_list_subdirs()
            .returning(|_| Ok(vec!["api".into(), "notes".into()]));
        fs.expect_exists().returning(|p: &str| p.ends_with("api/.git"));

        let provider = LocalProvider::new(Arc::new(fs), "/repos".into());
        assert_eq!(provider.get_repository("api").await.unwrap().unwrap().name, "api");
        assert!(provider.get_repository("notes").await.unwrap().is_none());
    }
}
