//! Domain-level repository errors that hide the underlying driver type.

/// Errors surfaced by repository implementations.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("entity not found")]
    NotFound,

    #[error("unique constraint violated: {0}")]
    Conflict(String),

    #[error("database error: {0}")]
    Backend(String),

    #[error("data mapping error: {0}")]
    Mapping(String),
}

pub type RepoResult<T> = Result<T, RepoError>;

impl From<sqlx::Error> for RepoError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => RepoError::NotFound,
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                RepoError::Conflict(db.message().to_string())
            }
            other => RepoError::Backend(other.to_string()),
        }
    }
}

impl From<RepoError> for crate::error::AppError {
    fn from(err: RepoError) -> Self {
        use crate::error::AppError;
        match err {
            RepoError::NotFound => AppError::NotFound("resource not found".into()),
            RepoError::Conflict(msg) => AppError::Conflict(msg),
            other => AppError::internal(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_not_found_maps_to_not_found() {
        let mapped: RepoError = sqlx::Error::RowNotFound.into();
        assert!(matches!(mapped, RepoError::NotFound));
    }

    #[test]
    fn repo_error_maps_to_app_error() {
        let app: crate::error::AppError = RepoError::NotFound.into();
        assert!(matches!(app, crate::error::AppError::NotFound(_)));
        let app: crate::error::AppError = RepoError::Conflict("dup".into()).into();
        assert!(matches!(app, crate::error::AppError::Conflict(_)));
    }
}
