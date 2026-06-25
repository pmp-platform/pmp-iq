//! Read-side queries over the platform model: paginated, searchable lists and
//! detail views. The trait has a Postgres implementation (using `to_jsonb` /
//! `json_agg`) and a SQLite implementation (assembling JSON in Rust).

mod pg;
mod sqlite;

pub use pg::PgPlatformQuery;
pub use sqlite::SqlitePlatformQuery;

use crate::db::RepoResult;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// Pagination + search parameters for a list query.
#[derive(Debug, Clone)]
pub struct ListQuery {
    pub search: String,
    pub page: i64,
    pub page_size: i64,
}

impl ListQuery {
    pub fn new(search: Option<String>, page: Option<i64>, page_size: Option<i64>) -> Self {
        let page = page.unwrap_or(1).max(1);
        let page_size = page_size.unwrap_or(25).clamp(1, 200);
        Self {
            search: search.unwrap_or_default(),
            page,
            page_size,
        }
    }

    pub fn offset(&self) -> i64 {
        (self.page - 1) * self.page_size
    }

    /// `%search%` pattern for `LIKE`.
    pub fn like(&self) -> String {
        format!("%{}%", self.search)
    }
}

/// A page of results plus the total count.
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, total: i64, q: &ListQuery) -> Self {
        Page {
            items,
            total,
            page: q.page,
            page_size: q.page_size,
        }
    }
}

/// Read access to the platform model.
#[async_trait]
pub trait PlatformQuery: Send + Sync {
    async fn list(&self, entity: &str, q: &ListQuery) -> RepoResult<Page<Value>>;
    async fn detail(&self, entity: &str, id: Uuid) -> RepoResult<Value>;
}

/// Recognised platform entity names (for routing/validation).
pub fn is_entity(name: &str) -> bool {
    matches!(
        name,
        "applications" | "infrastructure" | "libraries" | "users" | "groups"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_query_clamps_and_offsets() {
        let q = ListQuery::new(None, Some(3), Some(1000));
        assert_eq!(q.page_size, 200);
        assert_eq!(q.page, 3);
        assert_eq!(q.offset(), 400);
        let d = ListQuery::new(None, Some(0), None);
        assert_eq!(d.page, 1);
        assert_eq!(d.page_size, 25);
    }

    #[test]
    fn entity_recognition() {
        assert!(is_entity("applications"));
        assert!(!is_entity("widgets"));
    }
}
