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
use std::collections::BTreeMap;
use uuid::Uuid;

/// Pagination + search + filter parameters for a list query.
#[derive(Debug, Clone)]
pub struct ListQuery {
    pub search: String,
    pub page: i64,
    pub page_size: i64,
    /// Equality filters keyed by column name (validated against the entity's
    /// [`filter_fields`] allowlist by [`ListQuery::effective_filters`]).
    pub filters: BTreeMap<String, String>,
}

impl ListQuery {
    pub fn new(
        search: Option<String>,
        page: Option<i64>,
        page_size: Option<i64>,
        filters: BTreeMap<String, String>,
    ) -> Self {
        let page = page.unwrap_or(1).max(1);
        let page_size = page_size.unwrap_or(25).clamp(1, 200);
        Self {
            search: search.unwrap_or_default(),
            page,
            page_size,
            filters,
        }
    }

    pub fn offset(&self) -> i64 {
        (self.page - 1) * self.page_size
    }

    /// `%search%` pattern for `LIKE`.
    pub fn like(&self) -> String {
        format!("%{}%", self.search)
    }

    /// Requested filters limited to the allowlisted columns for `entity`, in a
    /// stable order (the `filter_fields` order). Empty values are dropped.
    pub fn effective_filters(&self, entity: &str) -> Vec<(&'static str, &str)> {
        filter_fields(entity)
            .iter()
            .filter_map(|&col| self.filters.get(col).map(|v| (col, v.as_str())))
            .filter(|(_, v)| !v.is_empty())
            .collect()
    }
}

/// Allowlisted equality-filter columns per entity. Bounds both the filter
/// WHERE clauses and the facet (dropdown) value sets, and prevents arbitrary
/// column names from being interpolated into SQL.
pub fn filter_fields(entity: &str) -> &'static [&'static str] {
    if crate::platform::linked::linked(entity).is_some() {
        return &["kind", "version"];
    }
    match entity {
        "applications" => &["app_type", "primary_language"],
        "libraries" => &["ecosystem"],
        _ => &[],
    }
}

/// The primary table backing an entity (for facet `SELECT DISTINCT` queries).
pub fn table_for(entity: &str) -> Option<&'static str> {
    if let Some(e) = crate::platform::linked::linked(entity) {
        return Some(e.table);
    }
    match entity {
        "applications" => Some("applications"),
        "libraries" => Some("libraries"),
        "users" => Some("users"),
        "groups" => Some("groups"),
        _ => None,
    }
}

/// Build ` AND {alias}{col}=<placeholder>` for each filter, numbering
/// placeholders from `start` (`$N` for Postgres, `?N` for SQLite). Column names
/// come from the [`filter_fields`] allowlist, so interpolation is safe.
pub fn filter_clause(filters: &[(&'static str, &str)], alias: &str, start: usize, sqlite: bool) -> String {
    let mut out = String::new();
    for (i, (col, _)) in filters.iter().enumerate() {
        let n = start + i;
        let placeholder = if sqlite { format!("?{n}") } else { format!("${n}") };
        out.push_str(&format!(" AND {alias}{col}={placeholder}"));
    }
    out
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
    /// Distinct values for each of the entity's filterable fields (for filter
    /// dropdowns): `{ field: [value, …] }`.
    async fn facets(&self, entity: &str) -> RepoResult<Value>;
    /// Snapshot of known entity names a dependency can target (applications +
    /// every linked entity), for canonicalizing dependency targets.
    async fn catalog(&self) -> RepoResult<crate::platform::catalog::Catalog>;
}

/// Recognised platform entity names (for routing/validation).
pub fn is_entity(name: &str) -> bool {
    crate::platform::linked::linked(name).is_some()
        || matches!(name, "applications" | "libraries" | "users" | "groups")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_query_clamps_and_offsets() {
        let q = ListQuery::new(None, Some(3), Some(1000), BTreeMap::new());
        assert_eq!(q.page_size, 200);
        assert_eq!(q.page, 3);
        assert_eq!(q.offset(), 400);
        let d = ListQuery::new(None, Some(0), None, BTreeMap::new());
        assert_eq!(d.page, 1);
        assert_eq!(d.page_size, 25);
    }

    #[test]
    fn effective_filters_allowlists_and_orders() {
        let mut filters = BTreeMap::new();
        filters.insert("primary_language".into(), "Rust".into());
        filters.insert("app_type".into(), "api".into());
        filters.insert("evil; DROP".into(), "x".into()); // not allowlisted → dropped
        let q = ListQuery::new(None, None, None, filters);
        let eff = q.effective_filters("applications");
        assert_eq!(eff, vec![("app_type", "api"), ("primary_language", "Rust")]);
        assert_eq!(
            filter_clause(&eff, "a.", 4, false),
            " AND a.app_type=$4 AND a.primary_language=$5"
        );
        assert_eq!(
            filter_clause(&eff, "a.", 5, true),
            " AND a.app_type=?5 AND a.primary_language=?6"
        );
    }

    #[test]
    fn entity_recognition() {
        assert!(is_entity("applications"));
        assert!(is_entity("tools"));
        assert!(is_entity("cloud-providers"));
        assert!(is_entity("external"));
        assert!(!is_entity("widgets"));
    }
}
