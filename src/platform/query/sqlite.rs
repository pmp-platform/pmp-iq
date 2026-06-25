//! SQLite implementation of [`PlatformQuery`]. SQLite lacks Postgres' `to_jsonb`
//! / `json_agg`, so rows are fetched with typed queries and JSON is assembled in
//! Rust.

use super::{ListQuery, Page, PlatformQuery};
use crate::db::{RepoError, RepoResult};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

pub struct SqlitePlatformQuery {
    pool: SqlitePool,
}

/// A nullable string column.
type Opt = Option<String>;

impl SqlitePlatformQuery {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn total(&self, table: &str, name_col: &str, q: &ListQuery) -> RepoResult<i64> {
        let sql = format!(
            "SELECT COUNT(*) FROM {table} WHERE (?1 = '' OR LOWER({name_col}) LIKE LOWER(?2))"
        );
        let (n,): (i64,) = sqlx::query_as(&sql)
            .bind(&q.search)
            .bind(q.like())
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    async fn applications(&self, q: &ListQuery) -> RepoResult<Page<Value>> {
        let rows: Vec<(Uuid, String, Opt, Opt, i64, i64, i64)> = sqlx::query_as(
            "SELECT a.id, a.name, a.app_type, a.primary_language, \
             (SELECT COUNT(*) FROM application_libraries al WHERE al.application_id=a.id), \
             (SELECT COUNT(*) FROM application_infrastructure ai WHERE ai.application_id=a.id), \
             (SELECT COUNT(*) FROM application_dependencies d WHERE d.source_app_id=a.id) \
             FROM applications a WHERE (?1='' OR LOWER(a.name) LIKE LOWER(?2)) \
             ORDER BY a.name LIMIT ?3 OFFSET ?4",
        )
        .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset())
        .fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, name, app_type, lang, libs, infra, deps)| {
                json!({ "id": id.to_string(), "name": name, "app_type": app_type,
                        "primary_language": lang, "libraries": libs,
                        "infrastructure": infra, "dependencies": deps })
            })
            .collect();
        Ok(Page::new(items, self.total("applications", "name", q).await?, q))
    }

    async fn infrastructure(&self, q: &ListQuery) -> RepoResult<Page<Value>> {
        let rows: Vec<(Uuid, String, String, Opt, i64)> = sqlx::query_as(
            "SELECT i.id, i.name, i.kind, i.version, \
             (SELECT COUNT(*) FROM application_infrastructure ai WHERE ai.infrastructure_id=i.id) \
             FROM infrastructure i WHERE (?1='' OR LOWER(i.name) LIKE LOWER(?2)) \
             ORDER BY i.name LIMIT ?3 OFFSET ?4",
        )
        .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset())
        .fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, name, kind, version, apps)| {
                json!({ "id": id.to_string(), "name": name, "kind": kind,
                        "version": version, "applications": apps })
            })
            .collect();
        Ok(Page::new(items, self.total("infrastructure", "name", q).await?, q))
    }

    async fn libraries(&self, q: &ListQuery) -> RepoResult<Page<Value>> {
        let rows: Vec<(Uuid, String, String, i64, i64)> = sqlx::query_as(
            "SELECT l.id, l.name, l.ecosystem, \
             (SELECT COUNT(*) FROM library_versions v WHERE v.library_id=l.id), \
             (SELECT COUNT(DISTINCT al.application_id) FROM library_versions v \
                JOIN application_libraries al ON al.library_version_id=v.id WHERE v.library_id=l.id) \
             FROM libraries l WHERE (?1='' OR LOWER(l.name) LIKE LOWER(?2)) \
             ORDER BY l.name LIMIT ?3 OFFSET ?4",
        )
        .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset())
        .fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, name, ecosystem, versions, apps)| {
                json!({ "id": id.to_string(), "name": name, "ecosystem": ecosystem,
                        "versions": versions, "applications": apps })
            })
            .collect();
        Ok(Page::new(items, self.total("libraries", "name", q).await?, q))
    }

    async fn users(&self, q: &ListQuery) -> RepoResult<Page<Value>> {
        let rows: Vec<(Uuid, String, Opt, i64, i64)> = sqlx::query_as(
            "SELECT u.id, u.username, u.email, \
             (SELECT COUNT(*) FROM group_memberships m WHERE m.user_id=u.id), \
             (SELECT COUNT(*) FROM access_grants g WHERE g.principal_type='user' AND g.principal_id=u.id) \
             FROM users u WHERE (?1='' OR LOWER(u.username) LIKE LOWER(?2)) \
             ORDER BY u.username LIMIT ?3 OFFSET ?4",
        )
        .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset())
        .fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, username, email, groups, apps)| {
                json!({ "id": id.to_string(), "username": username, "email": email,
                        "groups": groups, "applications": apps })
            })
            .collect();
        Ok(Page::new(items, self.total("users", "username", q).await?, q))
    }

    async fn groups(&self, q: &ListQuery) -> RepoResult<Page<Value>> {
        let rows: Vec<(Uuid, String, i64, i64)> = sqlx::query_as(
            "SELECT g.id, g.name, \
             (SELECT COUNT(*) FROM group_memberships m WHERE m.group_id=g.id), \
             (SELECT COUNT(*) FROM access_grants ag WHERE ag.principal_type='group' AND ag.principal_id=g.id) \
             FROM groups g WHERE (?1='' OR LOWER(g.name) LIKE LOWER(?2)) \
             ORDER BY g.name LIMIT ?3 OFFSET ?4",
        )
        .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset())
        .fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, name, members, apps)| {
                json!({ "id": id.to_string(), "name": name, "members": members, "applications": apps })
            })
            .collect();
        Ok(Page::new(items, self.total("groups", "name", q).await?, q))
    }

    async fn app_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String, Opt, Opt, Opt, Value)> = sqlx::query_as(
            "SELECT name, app_type, description, primary_language, metadata FROM applications WHERE id=?1",
        )
        .bind(id).fetch_optional(&self.pool).await?;
        let (name, app_type, description, primary_language, metadata) =
            base.ok_or(RepoError::NotFound)?;

        let languages: Vec<(String, Option<f64>)> = sqlx::query_as(
            "SELECT l.name, al.percentage FROM application_languages al \
             JOIN languages l ON l.id=al.language_id WHERE al.application_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let libraries: Vec<(String, String, String, Opt)> = sqlx::query_as(
            "SELECT lib.name, lib.ecosystem, v.version, al.scope FROM application_libraries al \
             JOIN library_versions v ON v.id=al.library_version_id \
             JOIN libraries lib ON lib.id=v.library_id WHERE al.application_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let infrastructure: Vec<(String, String, Opt, Opt)> = sqlx::query_as(
            "SELECT i.name, i.kind, i.version, ai.usage FROM application_infrastructure ai \
             JOIN infrastructure i ON i.id=ai.infrastructure_id WHERE ai.application_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let dependencies: Vec<(String, Opt, Opt)> = sqlx::query_as(
            "SELECT target_name, kind, description FROM application_dependencies WHERE source_app_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let access: Vec<(String, String, Opt)> = sqlx::query_as(
            "SELECT g.principal_type, g.access_level, \
             CASE WHEN g.principal_type='user' THEN u.username ELSE grp.name END \
             FROM access_grants g LEFT JOIN users u ON g.principal_type='user' AND u.id=g.principal_id \
             LEFT JOIN groups grp ON g.principal_type='group' AND grp.id=g.principal_id WHERE g.application_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;

        Ok(json!({
            "id": id.to_string(), "name": name, "app_type": app_type, "description": description,
            "primary_language": primary_language, "metadata": metadata,
            "languages": languages.into_iter().map(|(name, percentage)| json!({"name": name, "percentage": percentage})).collect::<Vec<_>>(),
            "libraries": libraries.into_iter().map(|(name, ecosystem, version, scope)| json!({"name": name, "ecosystem": ecosystem, "version": version, "scope": scope})).collect::<Vec<_>>(),
            "infrastructure": infrastructure.into_iter().map(|(name, kind, version, usage)| json!({"name": name, "kind": kind, "version": version, "usage": usage})).collect::<Vec<_>>(),
            "dependencies": dependencies.into_iter().map(|(target_name, kind, description)| json!({"target_name": target_name, "kind": kind, "description": description})).collect::<Vec<_>>(),
            "access": access.into_iter().map(|(principal_type, access_level, principal_name)| json!({"principal_type": principal_type, "access_level": access_level, "principal_name": principal_name})).collect::<Vec<_>>(),
        }))
    }

    async fn infra_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String, String, Opt)> = sqlx::query_as(
            "SELECT name, kind, version FROM infrastructure WHERE id=?1",
        ).bind(id).fetch_optional(&self.pool).await?;
        let (name, kind, version) = base.ok_or(RepoError::NotFound)?;
        let apps: Vec<(Uuid, String, Opt)> = sqlx::query_as(
            "SELECT a.id, a.name, ai.usage FROM application_infrastructure ai \
             JOIN applications a ON a.id=ai.application_id WHERE ai.infrastructure_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        Ok(json!({ "id": id.to_string(), "name": name, "kind": kind, "version": version,
            "applications": apps.into_iter().map(|(id, name, usage)| json!({"id": id.to_string(), "name": name, "usage": usage})).collect::<Vec<_>>() }))
    }

    async fn lib_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String, String)> = sqlx::query_as(
            "SELECT name, ecosystem FROM libraries WHERE id=?1",
        ).bind(id).fetch_optional(&self.pool).await?;
        let (name, ecosystem) = base.ok_or(RepoError::NotFound)?;
        let versions: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT version FROM library_versions WHERE library_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let apps: Vec<(Uuid, String, String, Opt)> = sqlx::query_as(
            "SELECT a.id, a.name, v.version, al.scope FROM library_versions v \
             JOIN application_libraries al ON al.library_version_id=v.id \
             JOIN applications a ON a.id=al.application_id WHERE v.library_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        Ok(json!({ "id": id.to_string(), "name": name, "ecosystem": ecosystem,
            "versions": versions.into_iter().map(|(v,)| v).collect::<Vec<_>>(),
            "applications": apps.into_iter().map(|(id, name, version, scope)| json!({"id": id.to_string(), "name": name, "version": version, "scope": scope})).collect::<Vec<_>>() }))
    }

    async fn user_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String, Opt)> = sqlx::query_as(
            "SELECT username, email FROM users WHERE id=?1",
        ).bind(id).fetch_optional(&self.pool).await?;
        let (username, email) = base.ok_or(RepoError::NotFound)?;
        let groups: Vec<(String,)> = sqlx::query_as(
            "SELECT g.name FROM group_memberships m JOIN groups g ON g.id=m.group_id WHERE m.user_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let apps: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT a.id, a.name, ag.access_level FROM access_grants ag \
             JOIN applications a ON a.id=ag.application_id \
             WHERE ag.principal_type='user' AND ag.principal_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        Ok(json!({ "id": id.to_string(), "username": username, "email": email,
            "groups": groups.into_iter().map(|(g,)| g).collect::<Vec<_>>(),
            "applications": apps.into_iter().map(|(id, name, access_level)| json!({"id": id.to_string(), "name": name, "access_level": access_level})).collect::<Vec<_>>() }))
    }

    async fn group_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM groups WHERE id=?1",
        ).bind(id).fetch_optional(&self.pool).await?;
        let (name,) = base.ok_or(RepoError::NotFound)?;
        let members: Vec<(String,)> = sqlx::query_as(
            "SELECT u.username FROM group_memberships m JOIN users u ON u.id=m.user_id WHERE m.group_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let apps: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT a.id, a.name, ag.access_level FROM access_grants ag \
             JOIN applications a ON a.id=ag.application_id \
             WHERE ag.principal_type='group' AND ag.principal_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        Ok(json!({ "id": id.to_string(), "name": name,
            "members": members.into_iter().map(|(m,)| m).collect::<Vec<_>>(),
            "applications": apps.into_iter().map(|(id, name, access_level)| json!({"id": id.to_string(), "name": name, "access_level": access_level})).collect::<Vec<_>>() }))
    }
}

#[async_trait]
impl PlatformQuery for SqlitePlatformQuery {
    async fn list(&self, entity: &str, q: &ListQuery) -> RepoResult<Page<Value>> {
        match entity {
            "applications" => self.applications(q).await,
            "infrastructure" => self.infrastructure(q).await,
            "libraries" => self.libraries(q).await,
            "users" => self.users(q).await,
            "groups" => self.groups(q).await,
            _ => Err(RepoError::Mapping(format!("unknown entity '{entity}'"))),
        }
    }

    async fn detail(&self, entity: &str, id: Uuid) -> RepoResult<Value> {
        match entity {
            "applications" => self.app_detail(id).await,
            "infrastructure" => self.infra_detail(id).await,
            "libraries" => self.lib_detail(id).await,
            "users" => self.user_detail(id).await,
            "groups" => self.group_detail(id).await,
            _ => Err(RepoError::Mapping(format!("unknown entity '{entity}'"))),
        }
    }
}
