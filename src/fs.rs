//! Filesystem abstraction so directory/file access can be mocked in tests.

use std::path::Path;

/// Errors from filesystem operations.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("io error: {0}")]
    Io(String),
}

/// A narrow filesystem interface covering what the app needs.
#[cfg_attr(test, mockall::automock)]
pub trait FileSystem: Send + Sync {
    /// Names of immediate subdirectories of `path`.
    fn list_subdirs(&self, path: &str) -> Result<Vec<String>, FsError>;

    /// Names of immediate (non-directory) files of `path`.
    fn list_files(&self, path: &str) -> Result<Vec<String>, FsError>;

    /// Whether a path exists.
    fn exists(&self, path: &str) -> bool;

    /// Whether a path exists and is a regular file (not a directory).
    fn is_file(&self, path: &str) -> bool;

    /// Read a file to a string, returning `None` if it does not exist.
    fn read_to_string(&self, path: &str) -> Result<Option<String>, FsError>;

    /// Recursively create a directory.
    fn create_dir_all(&self, path: &str) -> Result<(), FsError>;
}

/// Real filesystem implementation backed by `std::fs`.
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn list_subdirs(&self, path: &str) -> Result<Vec<String>, FsError> {
        let mut out = Vec::new();
        let entries = std::fs::read_dir(path).map_err(|e| FsError::Io(e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| FsError::Io(e.to_string()))?;
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
        Ok(out)
    }

    fn list_files(&self, path: &str) -> Result<Vec<String>, FsError> {
        let mut out = Vec::new();
        let entries = std::fs::read_dir(path).map_err(|e| FsError::Io(e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| FsError::Io(e.to_string()))?;
            if entry.path().is_file() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
        Ok(out)
    }

    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn is_file(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn read_to_string(&self, path: &str) -> Result<Option<String>, FsError> {
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(FsError::Io(e.to_string())),
        }
    }

    fn create_dir_all(&self, path: &str) -> Result<(), FsError> {
        std::fs::create_dir_all(path).map_err(|e| FsError::Io(e.to_string()))
    }
}
