//! Per-job workspace directory management over the [`FileSystem`] trait.

use crate::fs::{FileSystem, FsError};
use std::sync::Arc;

/// Allocates and prepares directories for cloned repositories.
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

    /// Directory for a repository within a job's workspace, creating parents.
    pub fn repo_dir(&self, job_marker: &str, full_name: &str) -> Result<String, FsError> {
        let path = format!(
            "{}/{}/{}",
            self.root.trim_end_matches('/'),
            Self::sanitize(job_marker),
            Self::sanitize(full_name)
        );
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
    fn repo_dir_builds_nested_path() {
        let mut fs = MockFileSystem::new();
        fs.expect_create_dir_all().returning(|_| Ok(()));
        let ws = Workspace::new(Arc::new(fs), "/work".into());
        let dir = ws.repo_dir("job1", "org/api").unwrap();
        assert_eq!(dir, "/work/job1/org_api");
    }
}
