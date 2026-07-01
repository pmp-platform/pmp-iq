//! Platform change feed (M36): an append-only record of model mutations emitted
//! by the writer (entities created/updated/removed, keyed by natural keys), plus
//! the read layer (timeline + two-point diff).

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgPool, SqlitePool};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Kind of model change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Updated,
    Removed,
}

impl ChangeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChangeKind::Created => "created",
            ChangeKind::Updated => "updated",
            ChangeKind::Removed => "removed",
        }
    }
}

/// One emitted change (before persistence).
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub entity_type: String,
    pub entity_key: String,
    pub kind: ChangeKind,
    pub detail: Value,
}

impl Change {
    pub fn new(entity_type: &str, entity_key: &str, kind: ChangeKind, detail: Value) -> Self {
        Self { entity_type: entity_type.into(), entity_key: entity_key.into(), kind, detail }
    }
}

/// Created/removed changes between a prior and next set of natural keys (each
/// deduplicated). Deterministic order: creations first, then removals, sorted.
pub fn diff_keys(entity_type: &str, prior: &[String], next: &[String]) -> Vec<Change> {
    let prior_set: BTreeSet<&str> = prior.iter().map(String::as_str).collect();
    let next_set: BTreeSet<&str> = next.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for k in next_set.difference(&prior_set) {
        out.push(Change::new(entity_type, k, ChangeKind::Created, Value::Null));
    }
    for k in prior_set.difference(&next_set) {
        out.push(Change::new(entity_type, k, ChangeKind::Removed, Value::Null));
    }
    out
}

/// A persisted change row.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ChangeRow {
    pub id: Uuid,
    pub application_id: Option<Uuid>,
    pub entity_type: String,
    pub entity_key: String,
    pub change: String,
    pub detail: Option<Value>,
    pub occurred_at: DateTime<Utc>,
}

/// A net per-entity-type delta between two points in time (M36 `diff`).
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct DiffCounts {
    pub created: i64,
    pub updated: i64,
    pub removed: i64,
}

/// Aggregate change rows into per-entity-type net counts.
pub fn summarize(rows: &[ChangeRow]) -> std::collections::BTreeMap<String, DiffCounts> {
    let mut out: std::collections::BTreeMap<String, DiffCounts> = Default::default();
    for r in rows {
        let entry = out.entry(r.entity_type.clone()).or_default();
        match r.change.as_str() {
            "created" => entry.created += 1,
            "updated" => entry.updated += 1,
            "removed" => entry.removed += 1,
            _ => {}
        }
    }
    out
}

/// Read/write the change feed.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait PlatformChangeRepository: Send + Sync {
    async fn record(&self, app_id: Option<Uuid>, change: Change, job: Option<Uuid>) -> RepoResult<()>;
    /// Newest-first change rows for one application (or the whole platform).
    async fn timeline(&self, app_id: Option<Uuid>, limit: i64) -> RepoResult<Vec<ChangeRow>>;
    /// Change rows in a time window (for `diff(from, to)`), optionally app-scoped.
    async fn between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        app_id: Option<Uuid>,
    ) -> RepoResult<Vec<ChangeRow>>;
}

const COLS: &str = "id, application_id, entity_type, entity_key, change, detail, occurred_at";

macro_rules! change_impl {
    ($name:ident, $pool:ty, $xform:path) => {
        pub struct $name {
            pool: $pool,
        }
        impl $name {
            pub fn new(pool: $pool) -> Self {
                Self { pool }
            }
        }
        #[async_trait]
        impl PlatformChangeRepository for $name {
            async fn record(&self, app_id: Option<Uuid>, change: Change, job: Option<Uuid>) -> RepoResult<()> {
                let detail = if change.detail.is_null() { None } else { Some(change.detail.clone()) };
                // Bind `occurred_at` explicitly so the write uses sqlx's own
                // timestamp encoding, matching the `occurred_at >= $1 AND <= $2`
                // diff-window filter. SQLite's CURRENT_TIMESTAMP default stores a
                // space-separated, offset-less string that sorts before sqlx's
                // RFC3339 bound value, dropping same-period changes at boundaries.
                sqlx::query(&$xform(
                    "INSERT INTO platform_changes \
                       (id, application_id, entity_type, entity_key, change, detail, job_execution_id, occurred_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                ))
                .bind(Uuid::new_v4())
                .bind(app_id)
                .bind(&change.entity_type)
                .bind(&change.entity_key)
                .bind(change.kind.as_str())
                .bind(detail)
                .bind(job)
                .bind(Utc::now())
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn timeline(&self, app_id: Option<Uuid>, limit: i64) -> RepoResult<Vec<ChangeRow>> {
                let rows: Vec<ChangeRow> = match app_id {
                    Some(id) => {
                        sqlx::query_as(&$xform(&format!(
                            "SELECT {COLS} FROM platform_changes WHERE application_id = $1 \
                             ORDER BY occurred_at DESC LIMIT $2"
                        )))
                        .bind(id)
                        .bind(limit)
                        .fetch_all(&self.pool)
                        .await?
                    }
                    None => {
                        sqlx::query_as(&$xform(&format!(
                            "SELECT {COLS} FROM platform_changes ORDER BY occurred_at DESC LIMIT $1"
                        )))
                        .bind(limit)
                        .fetch_all(&self.pool)
                        .await?
                    }
                };
                Ok(rows)
            }

            async fn between(
                &self,
                from: DateTime<Utc>,
                to: DateTime<Utc>,
                app_id: Option<Uuid>,
            ) -> RepoResult<Vec<ChangeRow>> {
                let rows: Vec<ChangeRow> = match app_id {
                    Some(id) => {
                        sqlx::query_as(&$xform(&format!(
                            "SELECT {COLS} FROM platform_changes \
                             WHERE occurred_at >= $1 AND occurred_at <= $2 AND application_id = $3 \
                             ORDER BY occurred_at DESC"
                        )))
                        .bind(from)
                        .bind(to)
                        .bind(id)
                        .fetch_all(&self.pool)
                        .await?
                    }
                    None => {
                        sqlx::query_as(&$xform(&format!(
                            "SELECT {COLS} FROM platform_changes \
                             WHERE occurred_at >= $1 AND occurred_at <= $2 ORDER BY occurred_at DESC"
                        )))
                        .bind(from)
                        .bind(to)
                        .fetch_all(&self.pool)
                        .await?
                    }
                };
                Ok(rows)
            }
        }
    };
}

change_impl!(PgPlatformChangeRepository, PgPool, identity);
change_impl!(SqlitePlatformChangeRepository, SqlitePool, to_sqlite);

/// Build the `application` change for a sync: `created` on first analysis, or
/// `updated` when its type/description changed (else `None`).
pub fn application_change(
    name: &str,
    prior: Option<(&str, &str)>,
    next_type: &str,
    next_description: &str,
) -> Option<Change> {
    match prior {
        None => Some(Change::new("application", name, ChangeKind::Created, Value::Null)),
        Some((ptype, pdesc)) if ptype != next_type || pdesc != next_description => Some(Change::new(
            "application",
            name,
            ChangeKind::Updated,
            json!({
                "before": { "app_type": ptype, "description": pdesc },
                "after": { "app_type": next_type, "description": next_description }
            }),
        )),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_keys_emits_created_and_removed() {
        let prior = vec!["a".to_string(), "b".to_string()];
        let next = vec!["b".to_string(), "c".to_string()];
        let changes = diff_keys("dependency", &prior, &next);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().any(|c| c.entity_key == "c" && c.kind == ChangeKind::Created));
        assert!(changes.iter().any(|c| c.entity_key == "a" && c.kind == ChangeKind::Removed));
        assert!(!changes.iter().any(|c| c.entity_key == "b")); // unchanged
    }

    #[test]
    fn diff_keys_dedups_and_is_empty_when_equal() {
        let prior = vec!["x".to_string(), "x".to_string()];
        let next = vec!["x".to_string()];
        assert!(diff_keys("library", &prior, &next).is_empty());
    }

    #[test]
    fn application_change_detects_create_and_update() {
        assert_eq!(
            application_change("api", None, "service", "d").unwrap().kind,
            ChangeKind::Created
        );
        assert!(application_change("api", Some(("service", "d")), "service", "d").is_none());
        let updated = application_change("api", Some(("api", "old")), "service", "new").unwrap();
        assert_eq!(updated.kind, ChangeKind::Updated);
        assert_eq!(updated.detail["after"]["app_type"], "service");
    }

    #[test]
    fn summarize_counts_per_type() {
        let rows = vec![
            ChangeRow { id: Uuid::new_v4(), application_id: None, entity_type: "dependency".into(), entity_key: "a".into(), change: "created".into(), detail: None, occurred_at: Utc::now() },
            ChangeRow { id: Uuid::new_v4(), application_id: None, entity_type: "dependency".into(), entity_key: "b".into(), change: "removed".into(), detail: None, occurred_at: Utc::now() },
            ChangeRow { id: Uuid::new_v4(), application_id: None, entity_type: "application".into(), entity_key: "x".into(), change: "updated".into(), detail: None, occurred_at: Utc::now() },
        ];
        let s = summarize(&rows);
        assert_eq!(s["dependency"], DiffCounts { created: 1, updated: 0, removed: 1 });
        assert_eq!(s["application"], DiffCounts { created: 0, updated: 1, removed: 0 });
    }
}
