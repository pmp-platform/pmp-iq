//! SQLite implementation of [`PlatformQuery`]. SQLite lacks Postgres' `to_jsonb`
//! / `json_agg`, so rows are fetched with typed queries and JSON is assembled in
//! Rust.

use super::{ListQuery, Page, PlatformQuery, filter_clause, filter_fields, table_for};
use crate::db::{RepoError, RepoResult};
use crate::platform::catalog::{Catalog, CatalogEntry};
use crate::platform::linked::{LINKED, LinkedEntity, linked};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

pub struct SqlitePlatformQuery {
    pool: SqlitePool,
}

/// A nullable string column.
type Opt = Option<String>;
/// One `application_dependencies` row: target_name, kind, description, target
/// app id, component id, component name.
type DepRow = (String, Opt, Opt, Option<Uuid>, Option<Uuid>, Opt);

/// A filter as (allowlisted column, value).
type Filters<'a> = [(&'static str, &'a str)];

impl SqlitePlatformQuery {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn total(&self, table: &str, name_col: &str, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<i64> {
        let clause = filter_clause(filters, "", 3, true);
        let sql = format!(
            "SELECT COUNT(*) FROM {table} WHERE (?1 = '' OR LOWER({name_col}) LIKE LOWER(?2)){clause}"
        );
        let mut query = sqlx::query_as::<_, (i64,)>(&sql).bind(&q.search).bind(q.like());
        for (_, v) in filters {
            query = query.bind(*v);
        }
        let (n,) = query.fetch_one(&self.pool).await?;
        Ok(n)
    }

    async fn applications(&self, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<Page<Value>> {
        let clause = filter_clause(filters, "a.", 5, true);
        let sql = format!(
            "SELECT a.id, a.name, a.app_type, a.primary_language, \
             (SELECT COUNT(*) FROM application_libraries al WHERE al.application_id=a.id), \
             (SELECT COUNT(*) FROM application_infrastructure ai WHERE ai.application_id=a.id), \
             (SELECT COUNT(*) FROM application_dependencies d WHERE d.source_app_id=a.id) \
             FROM applications a WHERE (?1='' OR LOWER(a.name) LIKE LOWER(?2)){clause} \
             ORDER BY a.name LIMIT ?3 OFFSET ?4"
        );
        let mut query = sqlx::query_as::<_, (Uuid, String, Opt, Opt, i64, i64, i64)>(&sql)
            .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset());
        for (_, v) in filters {
            query = query.bind(*v);
        }
        let rows = query.fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, name, app_type, lang, libs, infra, deps)| {
                json!({ "id": id.to_string(), "name": name, "app_type": app_type,
                        "primary_language": lang, "libraries": libs,
                        "infrastructure": infra, "dependencies": deps })
            })
            .collect();
        Ok(Page::new(items, self.total("applications", "name", q, filters).await?, q))
    }

    /// List any linked entity (infrastructure / tools / dependency types).
    async fn linked_list(&self, e: &LinkedEntity, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<Page<Value>> {
        let clause = filter_clause(filters, "i.", 5, true);
        let sql = format!(
            "SELECT i.id, i.name, i.kind, i.version, i.metadata, \
             (SELECT COUNT(*) FROM {join} ai WHERE ai.{fk}=i.id) \
             FROM {table} i WHERE (?1='' OR LOWER(i.name) LIKE LOWER(?2)){clause} \
             ORDER BY i.name LIMIT ?3 OFFSET ?4",
            join = e.join_table,
            fk = e.fk_col,
            table = e.table
        );
        let mut query = sqlx::query_as::<_, (Uuid, String, String, Opt, Value, i64)>(&sql)
            .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset());
        for (_, v) in filters {
            query = query.bind(*v);
        }
        let rows = query.fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, name, kind, version, metadata, apps)| {
                json!({ "id": id.to_string(), "name": name, "kind": kind,
                        "version": version, "metadata": metadata, "applications": apps })
            })
            .collect();
        Ok(Page::new(items, self.total(e.table, "name", q, filters).await?, q))
    }

    async fn libraries(&self, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<Page<Value>> {
        let clause = filter_clause(filters, "l.", 5, true);
        let sql = format!(
            "SELECT l.id, l.name, l.ecosystem, l.metadata, \
             (SELECT COUNT(*) FROM library_versions v WHERE v.library_id=l.id), \
             (SELECT COUNT(DISTINCT al.application_id) FROM library_versions v \
                JOIN application_libraries al ON al.library_version_id=v.id WHERE v.library_id=l.id) \
             FROM libraries l WHERE (?1='' OR LOWER(l.name) LIKE LOWER(?2)){clause} \
             ORDER BY l.name LIMIT ?3 OFFSET ?4"
        );
        let mut query = sqlx::query_as::<_, (Uuid, String, String, Value, i64, i64)>(&sql)
            .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset());
        for (_, v) in filters {
            query = query.bind(*v);
        }
        let rows = query.fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, name, ecosystem, metadata, versions, apps)| {
                json!({ "id": id.to_string(), "name": name, "ecosystem": ecosystem,
                        "metadata": metadata, "versions": versions, "applications": apps })
            })
            .collect();
        Ok(Page::new(items, self.total("libraries", "name", q, filters).await?, q))
    }

    async fn users(&self, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<Page<Value>> {
        let rows: Vec<(Uuid, String, Opt, Value, i64, i64)> = sqlx::query_as(
            "SELECT u.id, u.username, u.email, u.metadata, \
             (SELECT COUNT(*) FROM group_memberships m WHERE m.user_id=u.id), \
             (SELECT COUNT(*) FROM access_grants g WHERE g.principal_type='user' AND g.principal_id=u.id) \
             FROM users u WHERE (?1='' OR LOWER(u.username) LIKE LOWER(?2)) \
             ORDER BY u.username LIMIT ?3 OFFSET ?4",
        )
        .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset())
        .fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, username, email, metadata, groups, apps)| {
                json!({ "id": id.to_string(), "username": username, "email": email,
                        "metadata": metadata, "groups": groups, "applications": apps })
            })
            .collect();
        Ok(Page::new(items, self.total("users", "username", q, filters).await?, q))
    }

    async fn groups(&self, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<Page<Value>> {
        let rows: Vec<(Uuid, String, Value, i64, i64)> = sqlx::query_as(
            "SELECT g.id, g.name, g.metadata, \
             (SELECT COUNT(*) FROM group_memberships m WHERE m.group_id=g.id), \
             (SELECT COUNT(*) FROM access_grants ag WHERE ag.principal_type='group' AND ag.principal_id=g.id) \
             FROM groups g WHERE (?1='' OR LOWER(g.name) LIKE LOWER(?2)) \
             ORDER BY g.name LIMIT ?3 OFFSET ?4",
        )
        .bind(&q.search).bind(q.like()).bind(q.page_size).bind(q.offset())
        .fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(|(id, name, metadata, members, apps)| {
                json!({ "id": id.to_string(), "name": name, "metadata": metadata,
                        "members": members, "applications": apps })
            })
            .collect();
        Ok(Page::new(items, self.total("groups", "name", q, filters).await?, q))
    }

    async fn app_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String, Opt, Opt, Opt, Value, Option<Uuid>)> = sqlx::query_as(
            "SELECT name, app_type, description, primary_language, metadata, repository_id FROM applications WHERE id=?1",
        )
        .bind(id).fetch_optional(&self.pool).await?;
        let (name, app_type, description, primary_language, metadata, repository_id) =
            base.ok_or(RepoError::NotFound)?;

        let languages: Vec<(String, Option<f64>)> = sqlx::query_as(
            "SELECT l.name, al.percentage FROM application_languages al \
             JOIN languages l ON l.id=al.language_id WHERE al.application_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let libraries: Vec<(Uuid, String, String, String, Opt, Value)> = sqlx::query_as(
            "SELECT lib.id, lib.name, lib.ecosystem, v.version, al.scope, lib.metadata FROM application_libraries al \
             JOIN library_versions v ON v.id=al.library_version_id \
             JOIN libraries lib ON lib.id=v.library_id WHERE al.application_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let dependencies: Vec<DepRow> = sqlx::query_as(
            "SELECT d.target_name, d.kind, d.description, ta.id, co.id, co.name FROM application_dependencies d \
             LEFT JOIN applications ta ON ta.name=d.target_name \
             LEFT JOIN components co ON co.id=d.component_id WHERE d.source_app_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let access: Vec<(String, Opt, String, Value, Opt)> = sqlx::query_as(
            "SELECT g.principal_type, g.access_level, g.association_type, g.permissions, \
             CASE WHEN g.principal_type='user' THEN u.username ELSE grp.name END \
             FROM access_grants g LEFT JOIN users u ON g.principal_type='user' AND u.id=g.principal_id \
             LEFT JOIN groups grp ON g.principal_type='group' AND grp.id=g.principal_id WHERE g.application_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;

        let components = self.app_components(id).await?;
        let use_cases = self.app_use_cases(id).await?;

        let mut out = json!({
            "id": id.to_string(), "name": name, "app_type": app_type, "description": description,
            "primary_language": primary_language, "metadata": metadata,
            "repository_id": repository_id.map(|r| r.to_string()),
            "languages": languages.into_iter().map(|(name, percentage)| json!({"name": name, "percentage": percentage})).collect::<Vec<_>>(),
            "libraries": libraries.into_iter().map(|(id, name, ecosystem, version, scope, metadata)| json!({"id": id.to_string(), "name": name, "ecosystem": ecosystem, "version": version, "scope": scope, "metadata": metadata})).collect::<Vec<_>>(),
            "dependencies": dependencies.into_iter().map(|(target_name, kind, description, target_app_id, component_id, component_name)| json!({"target_name": target_name, "kind": kind, "description": description, "target_app_id": target_app_id.map(|t| t.to_string()), "component_id": component_id.map(|c| c.to_string()), "component_name": component_name})).collect::<Vec<_>>(),
            "access": access.into_iter().map(|(principal_type, access_level, association_type, permissions, principal_name)| json!({"principal_type": principal_type, "access_level": access_level, "association_type": association_type, "permissions": permissions, "principal_name": principal_name})).collect::<Vec<_>>(),
            "components": components,
            "use_cases": use_cases,
        });
        if let Some(obj) = out.as_object_mut() {
            obj.extend(self.app_linked_relations(id).await?);
        }
        Ok(out)
    }

    /// An application's components, each with the observability signals it emits.
    async fn app_components(&self, app_id: Uuid) -> RepoResult<Vec<Value>> {
        let rows: Vec<(Uuid, String, String, Opt, Value)> = sqlx::query_as(
            "SELECT id, name, kind, description, metadata FROM components WHERE application_id=?1 ORDER BY name",
        ).bind(app_id).fetch_all(&self.pool).await?;
        let mut out = Vec::with_capacity(rows.len());
        for (component_id, name, kind, description, metadata) in rows {
            let signals: Vec<(Uuid, String, String, Opt)> = sqlx::query_as(
                "SELECT id, name, kind, description FROM observability_signals WHERE component_id=?1 ORDER BY name",
            ).bind(component_id).fetch_all(&self.pool).await?;
            let files: Vec<(String,)> = sqlx::query_as(
                "SELECT path FROM component_files WHERE component_id=?1 ORDER BY path",
            ).bind(component_id).fetch_all(&self.pool).await?;
            out.push(json!({"id": component_id.to_string(), "name": name, "kind": kind, "description": description, "metadata": metadata,
                "files": files.into_iter().map(|(p,)| p).collect::<Vec<_>>(),
                "observability_signals": signals.into_iter().map(|(sid, sname, skind, sdesc)| json!({"id": sid.to_string(), "name": sname, "kind": skind, "description": sdesc})).collect::<Vec<_>>()}));
        }
        Ok(out)
    }

    /// An application's use cases, each with referenced components and diagrams.
    async fn app_use_cases(&self, app_id: Uuid) -> RepoResult<Vec<Value>> {
        let rows: Vec<(Uuid, String, Opt, Value)> = sqlx::query_as(
            "SELECT id, name, description, metadata FROM use_cases WHERE application_id=?1 ORDER BY name",
        ).bind(app_id).fetch_all(&self.pool).await?;
        let mut out = Vec::with_capacity(rows.len());
        for (use_case_id, name, description, metadata) in rows {
            let comps: Vec<(Uuid, String)> = sqlx::query_as(
                "SELECT c.id, c.name FROM use_case_components ucc JOIN components c ON c.id=ucc.component_id WHERE ucc.use_case_id=?1 ORDER BY c.name",
            ).bind(use_case_id).fetch_all(&self.pool).await?;
            let diagrams: Vec<(Uuid, String, String, Opt, String)> = sqlx::query_as(
                "SELECT id, name, kind, description, content FROM diagrams WHERE use_case_id=?1 ORDER BY name",
            ).bind(use_case_id).fetch_all(&self.pool).await?;
            let files: Vec<(String,)> = sqlx::query_as(
                "SELECT path FROM use_case_files WHERE use_case_id=?1 ORDER BY path",
            ).bind(use_case_id).fetch_all(&self.pool).await?;
            out.push(json!({"id": use_case_id.to_string(), "name": name, "description": description, "metadata": metadata,
                "files": files.into_iter().map(|(p,)| p).collect::<Vec<_>>(),
                "components": comps.into_iter().map(|(cid, cname)| json!({"id": cid.to_string(), "name": cname})).collect::<Vec<_>>(),
                "diagrams": diagrams.into_iter().map(|(did, dname, dkind, ddesc, dcontent)| json!({"id": did.to_string(), "name": dname, "kind": dkind, "description": ddesc, "content": dcontent})).collect::<Vec<_>>()}));
        }
        Ok(out)
    }

    /// Detail for any linked entity (the entity plus the applications using it).
    async fn linked_detail(&self, e: &LinkedEntity, id: Uuid) -> RepoResult<Value> {
        let base_sql = format!("SELECT name, kind, version, metadata FROM {} WHERE id=?1", e.table);
        let base: Option<(String, String, Opt, Value)> =
            sqlx::query_as(&base_sql).bind(id).fetch_optional(&self.pool).await?;
        let (name, kind, version, metadata) = base.ok_or(RepoError::NotFound)?;
        let apps_sql = format!(
            "SELECT a.id, a.name, j.usage FROM {join} j \
             JOIN applications a ON a.id=j.application_id WHERE j.{fk}=?1",
            join = e.join_table,
            fk = e.fk_col
        );
        let apps: Vec<(Uuid, String, Opt)> =
            sqlx::query_as(&apps_sql).bind(id).fetch_all(&self.pool).await?;
        Ok(json!({ "id": id.to_string(), "name": name, "kind": kind, "version": version, "metadata": metadata,
            "applications": apps.into_iter().map(|(id, name, usage)| json!({"id": id.to_string(), "name": name, "usage": usage})).collect::<Vec<_>>() }))
    }

    /// Linked relations for an application, keyed by registry entity name.
    async fn app_linked_relations(&self, app_id: Uuid) -> RepoResult<serde_json::Map<String, Value>> {
        let mut map = serde_json::Map::new();
        for e in LINKED {
            let sql = format!(
                "SELECT x.id, x.name, x.kind, x.version, j.usage FROM {join} j \
                 JOIN {table} x ON x.id=j.{fk} WHERE j.application_id=?1",
                join = e.join_table,
                table = e.table,
                fk = e.fk_col
            );
            let rows: Vec<(Uuid, String, String, Opt, Opt)> =
                sqlx::query_as(&sql).bind(app_id).fetch_all(&self.pool).await?;
            let items: Vec<Value> = rows
                .into_iter()
                .map(|(id, name, kind, version, usage)| {
                    json!({"id": id.to_string(), "name": name, "kind": kind, "version": version, "usage": usage})
                })
                .collect();
            map.insert(e.name.to_string(), Value::Array(items));
        }
        Ok(map)
    }

    async fn lib_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String, String, Value)> = sqlx::query_as(
            "SELECT name, ecosystem, metadata FROM libraries WHERE id=?1",
        ).bind(id).fetch_optional(&self.pool).await?;
        let (name, ecosystem, metadata) = base.ok_or(RepoError::NotFound)?;
        let versions: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT version FROM library_versions WHERE library_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let apps: Vec<(Uuid, String, String, Opt)> = sqlx::query_as(
            "SELECT a.id, a.name, v.version, al.scope FROM library_versions v \
             JOIN application_libraries al ON al.library_version_id=v.id \
             JOIN applications a ON a.id=al.application_id WHERE v.library_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        Ok(json!({ "id": id.to_string(), "name": name, "ecosystem": ecosystem, "metadata": metadata,
            "versions": versions.into_iter().map(|(v,)| v).collect::<Vec<_>>(),
            "applications": apps.into_iter().map(|(id, name, version, scope)| json!({"id": id.to_string(), "name": name, "version": version, "scope": scope})).collect::<Vec<_>>() }))
    }

    async fn user_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String, Opt, Value)> = sqlx::query_as(
            "SELECT username, email, metadata FROM users WHERE id=?1",
        ).bind(id).fetch_optional(&self.pool).await?;
        let (username, email, metadata) = base.ok_or(RepoError::NotFound)?;
        let groups: Vec<(String,)> = sqlx::query_as(
            "SELECT g.name FROM group_memberships m JOIN groups g ON g.id=m.group_id WHERE m.user_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let apps: Vec<(Uuid, String, Opt, String, Value)> = sqlx::query_as(
            "SELECT a.id, a.name, ag.access_level, ag.association_type, ag.permissions FROM access_grants ag \
             JOIN applications a ON a.id=ag.application_id \
             WHERE ag.principal_type='user' AND ag.principal_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        Ok(json!({ "id": id.to_string(), "username": username, "email": email, "metadata": metadata,
            "groups": groups.into_iter().map(|(g,)| g).collect::<Vec<_>>(),
            "applications": apps.into_iter().map(|(id, name, access_level, association_type, permissions)| json!({"id": id.to_string(), "name": name, "access_level": access_level, "association_type": association_type, "permissions": permissions})).collect::<Vec<_>>() }))
    }

    async fn group_detail(&self, id: Uuid) -> RepoResult<Value> {
        let base: Option<(String, Value)> = sqlx::query_as(
            "SELECT name, metadata FROM groups WHERE id=?1",
        ).bind(id).fetch_optional(&self.pool).await?;
        let (name, metadata) = base.ok_or(RepoError::NotFound)?;
        let members: Vec<(String,)> = sqlx::query_as(
            "SELECT u.username FROM group_memberships m JOIN users u ON u.id=m.user_id WHERE m.group_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        let apps: Vec<(Uuid, String, Opt, String, Value)> = sqlx::query_as(
            "SELECT a.id, a.name, ag.access_level, ag.association_type, ag.permissions FROM access_grants ag \
             JOIN applications a ON a.id=ag.application_id \
             WHERE ag.principal_type='group' AND ag.principal_id=?1",
        ).bind(id).fetch_all(&self.pool).await?;
        Ok(json!({ "id": id.to_string(), "name": name, "metadata": metadata,
            "members": members.into_iter().map(|(m,)| m).collect::<Vec<_>>(),
            "applications": apps.into_iter().map(|(id, name, access_level, association_type, permissions)| json!({"id": id.to_string(), "name": name, "access_level": access_level, "association_type": association_type, "permissions": permissions})).collect::<Vec<_>>() }))
    }
}

#[async_trait]
impl PlatformQuery for SqlitePlatformQuery {
    async fn list(&self, entity: &str, q: &ListQuery) -> RepoResult<Page<Value>> {
        let filters = q.effective_filters(entity);
        if let Some(e) = linked(entity) {
            return self.linked_list(e, q, &filters).await;
        }
        match entity {
            "applications" => self.applications(q, &filters).await,
            "libraries" => self.libraries(q, &filters).await,
            "users" => self.users(q, &filters).await,
            "groups" => self.groups(q, &filters).await,
            _ => Err(RepoError::Mapping(format!("unknown entity '{entity}'"))),
        }
    }

    async fn detail(&self, entity: &str, id: Uuid) -> RepoResult<Value> {
        if let Some(e) = linked(entity) {
            return self.linked_detail(e, id).await;
        }
        match entity {
            "applications" => self.app_detail(id).await,
            "libraries" => self.lib_detail(id).await,
            "users" => self.user_detail(id).await,
            "groups" => self.group_detail(id).await,
            _ => Err(RepoError::Mapping(format!("unknown entity '{entity}'"))),
        }
    }

    async fn embedding_sources(&self) -> RepoResult<Vec<super::EmbeddingSourceRow>> {
        let mut fetched = Vec::new();
        for (entity_type, sql) in super::EMBEDDING_SOURCE_QUERIES {
            let rows: Vec<(Uuid, String, String, String)> =
                sqlx::query_as(sql).fetch_all(&self.pool).await?;
            fetched.push((*entity_type, rows));
        }
        Ok(super::embedding_rows(fetched))
    }

    async fn facets(&self, entity: &str) -> RepoResult<Value> {
        let Some(table) = table_for(entity) else {
            return Ok(json!({}));
        };
        let mut out = serde_json::Map::new();
        for &field in filter_fields(entity) {
            let sql = format!(
                "SELECT DISTINCT {field} AS v FROM {table} \
                 WHERE {field} IS NOT NULL AND {field} <> '' ORDER BY v"
            );
            let rows: Vec<(String,)> = sqlx::query_as(&sql).fetch_all(&self.pool).await?;
            out.insert(field.to_string(), json!(rows.into_iter().map(|(v,)| v).collect::<Vec<_>>()));
        }
        Ok(Value::Object(out))
    }

    async fn catalog(&self) -> RepoResult<Catalog> {
        let mut sql = String::from("SELECT name, 'application' AS kind FROM applications");
        for e in LINKED {
            sql.push_str(&format!(" UNION ALL SELECT name, '{}' AS kind FROM {}", e.name, e.table));
        }
        let rows: Vec<(String, String)> = sqlx::query_as(&sql).fetch_all(&self.pool).await?;
        Ok(Catalog::new(
            rows.into_iter().map(|(name, kind)| CatalogEntry { name, kind }).collect(),
        ))
    }

    async fn application_repository(&self, app_id: Uuid) -> RepoResult<Option<Uuid>> {
        let row: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT repository_id FROM applications WHERE id = ?1")
                .bind(app_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(repo,)| repo))
    }

    async fn repository_application(&self, repository_id: Uuid) -> RepoResult<Option<Uuid>> {
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM applications WHERE repository_id = ?1")
                .bind(repository_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(id,)| id))
    }
}
