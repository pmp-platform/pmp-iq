//! Read-only browsing of a cloned repository checkout: a lazy one-level file
//! tree and size-capped file contents, sandboxed to the checkout root.

use crate::fs::FileSystem;
use serde::Serialize;
use std::sync::Arc;

/// Directories never shown in the tree.
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".idea", ".vscode"];
/// Largest file returned to the viewer.
const MAX_FILE_BYTES: usize = 512 * 1024;

/// Errors from browsing the checkout.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("path escapes the repository root")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("file is too large to display")]
    TooLarge,
    #[error("file is not text")]
    Binary,
    #[error("io error: {0}")]
    Io(String),
}

/// One entry in a directory listing.
#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Contents of a file.
#[derive(Debug, Clone, Serialize)]
pub struct FileContent {
    pub path: String,
    pub content: String,
}

/// Join a user-supplied relative path under `root`, rejecting any escape
/// (absolute paths, `..` segments, drive letters). Returns the absolute path.
pub fn safe_join(root: &str, rel: &str) -> Result<String, FileError> {
    let rel = rel.trim_start_matches(['/', '\\']);
    if std::path::Path::new(rel).is_absolute() {
        return Err(FileError::Forbidden);
    }
    for segment in rel.split(['/', '\\']) {
        if segment == ".." || segment.contains(':') {
            return Err(FileError::Forbidden);
        }
    }
    if rel.is_empty() {
        Ok(root.trim_end_matches(['/', '\\']).to_string())
    } else {
        Ok(format!("{}/{}", root.trim_end_matches(['/', '\\']), rel))
    }
}

/// Lists and reads files within a checkout over the [`FileSystem`] trait.
pub struct FileBrowser {
    fs: Arc<dyn FileSystem>,
}

impl FileBrowser {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }

    /// One directory level under `root`/`rel`: subdirectories then files, sorted.
    pub fn list(&self, root: &str, rel: &str) -> Result<Vec<DirEntry>, FileError> {
        let dir = safe_join(root, rel)?;
        let mut dirs = self
            .fs
            .list_subdirs(&dir)
            .map_err(|e| FileError::Io(e.to_string()))?;
        dirs.retain(|d| !SKIP_DIRS.contains(&d.as_str()));
        dirs.sort();
        let mut files = self
            .fs
            .list_files(&dir)
            .map_err(|e| FileError::Io(e.to_string()))?;
        files.sort();
        let mut out: Vec<DirEntry> =
            dirs.into_iter().map(|name| DirEntry { name, is_dir: true }).collect();
        out.extend(files.into_iter().map(|name| DirEntry { name, is_dir: false }));
        Ok(out)
    }

    /// The text content of a file, capped in size and rejecting binary files.
    pub fn read(&self, root: &str, rel: &str) -> Result<FileContent, FileError> {
        let path = safe_join(root, rel)?;
        let content = self
            .fs
            .read_to_string(&path)
            .map_err(|e| FileError::Io(e.to_string()))?
            .ok_or(FileError::NotFound)?;
        if content.len() > MAX_FILE_BYTES {
            return Err(FileError::TooLarge);
        }
        if content.contains('\u{0}') {
            return Err(FileError::Binary);
        }
        Ok(FileContent { path: rel.to_string(), content })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFileSystem;

    #[test]
    fn safe_join_blocks_escapes() {
        assert!(matches!(safe_join("/repo", "../etc/passwd"), Err(FileError::Forbidden)));
        assert!(matches!(safe_join("/repo", "a/../../b"), Err(FileError::Forbidden)));
        assert!(matches!(safe_join("/repo", "C:\\x"), Err(FileError::Forbidden)));
    }

    #[test]
    fn safe_join_clamps_and_allows_relative_paths() {
        assert_eq!(safe_join("/repo", "").unwrap(), "/repo");
        assert_eq!(safe_join("/repo/", "src/main.rs").unwrap(), "/repo/src/main.rs");
        // A leading slash is clamped under the root, not an escape.
        assert_eq!(safe_join("/repo", "/etc/passwd").unwrap(), "/repo/etc/passwd");
    }

    #[test]
    fn list_sorts_dirs_before_files_and_skips_git() {
        let mut fs = MockFileSystem::new();
        fs.expect_list_subdirs().returning(|_| Ok(vec!["src".into(), ".git".into()]));
        fs.expect_list_files().returning(|_| Ok(vec!["README.md".into(), "Cargo.toml".into()]));
        let browser = FileBrowser::new(Arc::new(fs));
        let entries = browser.list("/repo", "").unwrap();
        let names: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
        assert_eq!(names, vec![("src", true), ("Cargo.toml", false), ("README.md", false)]);
    }

    #[test]
    fn read_rejects_binary_and_missing() {
        let mut fs = MockFileSystem::new();
        fs.expect_read_to_string().returning(|p: &str| {
            if p.ends_with("bin") {
                Ok(Some("a\u{0}b".to_string()))
            } else {
                Ok(None)
            }
        });
        let browser = FileBrowser::new(Arc::new(fs));
        assert!(matches!(browser.read("/repo", "bin"), Err(FileError::Binary)));
        assert!(matches!(browser.read("/repo", "missing"), Err(FileError::NotFound)));
    }
}
