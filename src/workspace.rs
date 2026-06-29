//! Per-job workspace directory management over the [`FileSystem`] trait.

use crate::fs::{FileSystem, FsError};
use std::sync::Arc;
use uuid::Uuid;

/// Identifies the owning job for a workspace path (bundles to bound params).
pub struct JobLocator<'a> {
    pub name: &'a str,
    pub id: Uuid,
}

impl<'a> JobLocator<'a> {
    pub fn new(name: &'a str, id: Uuid) -> Self {
        Self { name, id }
    }
}

/// Allocates and prepares directories for cloned repositories. Each job owns a
/// stable directory `{root}/jobs/{job-name}/{job-id}` that persists across
/// executions, so re-runs reuse an existing clone.
#[derive(Clone)]
pub struct Workspace {
    fs: Arc<dyn FileSystem>,
    root: String,
}

impl Workspace {
    pub fn new(fs: Arc<dyn FileSystem>, root: String) -> Self {
        Self { fs, root }
    }

    /// Sanitise a repository full name into a directory-safe segment.
    pub fn sanitize(full_name: &str) -> String {
        full_name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect()
    }

    /// The stable root directory for a job: `{root}/jobs/{name}/{id}`.
    pub fn job_dir(&self, job: &JobLocator<'_>) -> String {
        format!(
            "{}/jobs/{}/{}",
            self.root.trim_end_matches('/'),
            Self::sanitize(job.name),
            job.id
        )
    }

    /// Directory for a repository within a job's workspace, creating parents.
    pub fn repo_dir(&self, job: &JobLocator<'_>, full_name: &str) -> Result<String, FsError> {
        let path = format!("{}/{}", self.job_dir(job), Self::sanitize(full_name));
        if let Some(parent) = path.rsplit_once('/').map(|(p, _)| p) {
            self.fs.create_dir_all(parent)?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFileSystem;

    #[test]
    fn sanitize_replaces_unsafe_chars() {
        assert_eq!(Workspace::sanitize("org/api-gateway"), "org_api-gateway");
    }

    #[test]
    fn repo_dir_builds_nested_per_job_path() {
        let mut fs = MockFileSystem::new();
        fs.expect_create_dir_all().returning(|_| Ok(()));
        let ws = Workspace::new(Arc::new(fs), "/work".into());
        let id = Uuid::nil();
        let dir = ws.repo_dir(&JobLocator::new("sync repos", id), "org/api").unwrap();
        assert_eq!(dir, format!("/work/jobs/sync_repos/{id}/org_api"));
    }
}
