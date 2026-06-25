//! PostgreSQL implementation of [`PlatformQuery`] using `to_jsonb`/`json_agg`.

use super::{ListQuery, Page, PlatformQuery};
use crate::db::{RepoError, RepoResult};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PgPlatformQuery {
    pool: PgPool,
}

impl PgPlatformQuery {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn total(&self, table: &str, name_col: &str, search: &str) -> RepoResult<i64> {
        let sql = format!(
            "SELECT COUNT(*) FROM {table} WHERE ($1 = '' OR {name_col} ILIKE '%'||$1||'%')"
        );
        let (n,): (i64,) = sqlx::query_as(&sql).bind(search).fetch_one(&self.pool).await?;
        Ok(n)
    }

    async fn list_json(&self, sql: &str, q: &ListQuery) -> RepoResult<Vec<Value>> {
        let rows: Vec<(Value,)> = sqlx::query_as(sql)
            .bind(&q.search)
            .bind(q.page_size)
            .bind(q.offset())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(v,)| v).collect())
    }

    async fn fetch_one_json(&self, sql: &str, id: Uuid) -> RepoResult<Value> {
        let row: Option<(Value,)> = sqlx::query_as(sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|(v,)| v).ok_or(RepoError::NotFound)
    }

    async fn list_for(
        &self,
        sql: &str,
        table: &str,
        name_col: &str,
        q: &ListQuery,
    ) -> RepoResult<Page<Value>> {
        let items = self.list_json(sql, q).await?;
        let total = self.total(table, name_col, &q.search).await?;
        Ok(Page::new(items, total, q))
    }
}

const APPS_LIST: &str = "SELECT to_jsonb(t) FROM (\
    SELECT a.id, a.name, a.app_type, a.primary_language, \
      (SELECT COUNT(*) FROM application_libraries al WHERE al.application_id=a.id) AS libraries, \
      (SELECT COUNT(*) FROM application_infrastructure ai WHERE ai.application_id=a.id) AS infrastructure, \
      (SELECT COUNT(*) FROM application_dependencies d WHERE d.source_app_id=a.id) AS dependencies \
    FROM applications a WHERE ($1 = '' OR a.name ILIKE '%'||$1||'%') \
    ORDER BY a.name LIMIT $2 OFFSET $3) t";

const INFRA_LIST: &str = "SELECT to_jsonb(t) FROM (\
    SELECT i.id, i.name, i.kind, i.version, \
      (SELECT COUNT(*) FROM application_infrastructure ai WHERE ai.infrastructure_id=i.id) AS applications \
    FROM infrastructure i WHERE ($1 = '' OR i.name ILIKE '%'||$1||'%') \
    ORDER BY i.name LIMIT $2 OFFSET $3) t";

const LIBS_LIST: &str = "SELECT to_jsonb(t) FROM (\
    SELECT l.id, l.name, l.ecosystem, \
      (SELECT COUNT(*) FROM library_versions v WHERE v.library_id=l.id) AS versions, \
      (SELECT COUNT(DISTINCT al.application_id) FROM library_versions v \
         JOIN application_libraries al ON al.library_version_id=v.id WHERE v.library_id=l.id) AS applications \
    FROM libraries l WHERE ($1 = '' OR l.name ILIKE '%'||$1||'%') \
    ORDER BY l.name LIMIT $2 OFFSET $3) t";

const USERS_LIST: &str = "SELECT to_jsonb(t) FROM (\
    SELECT u.id, u.username, u.email, \
      (SELECT COUNT(*) FROM group_memberships m WHERE m.user_id=u.id) AS groups, \
      (SELECT COUNT(*) FROM access_grants g WHERE g.principal_type='user' AND g.principal_id=u.id) AS applications \
    FROM users u WHERE ($1 = '' OR u.username ILIKE '%'||$1||'%') \
    ORDER BY u.username LIMIT $2 OFFSET $3) t";

const GROUPS_LIST: &str = "SELECT to_jsonb(t) FROM (\
    SELECT g.id, g.name, \
      (SELECT COUNT(*) FROM group_memberships m WHERE m.group_id=g.id) AS members, \
      (SELECT COUNT(*) FROM access_grants ag WHERE ag.principal_type='group' AND ag.principal_id=g.id) AS applications \
    FROM groups g WHERE ($1 = '' OR g.name ILIKE '%'||$1||'%') \
    ORDER BY g.name LIMIT $2 OFFSET $3) t";

const APP_DETAIL: &str = "SELECT to_jsonb(t) FROM (SELECT \
    a.id, a.name, a.app_type, a.description, a.primary_language, a.metadata, \
    (SELECT json_agg(json_build_object('name', l.name, 'percentage', al.percentage)) \
       FROM application_languages al JOIN languages l ON l.id=al.language_id WHERE al.application_id=a.id) AS languages, \
    (SELECT json_agg(json_build_object('name', lib.name, 'ecosystem', lib.ecosystem, 'version', v.version, 'scope', al.scope)) \
       FROM application_libraries al JOIN library_versions v ON v.id=al.library_version_id \
       JOIN libraries lib ON lib.id=v.library_id WHERE al.application_id=a.id) AS libraries, \
    (SELECT json_agg(json_build_object('name', i.name, 'kind', i.kind, 'version', i.version, 'usage', ai.usage)) \
       FROM application_infrastructure ai JOIN infrastructure i ON i.id=ai.infrastructure_id WHERE ai.application_id=a.id) AS infrastructure, \
    (SELECT json_agg(json_build_object('target_name', d.target_name, 'kind', d.kind, 'description', d.description)) \
       FROM application_dependencies d WHERE d.source_app_id=a.id) AS dependencies, \
    (SELECT json_agg(json_build_object('principal_type', g.principal_type, 'access_level', g.access_level, \
        'principal_name', CASE WHEN g.principal_type='user' THEN u.username ELSE grp.name END)) \
       FROM access_grants g LEFT JOIN users u ON g.principal_type='user' AND u.id=g.principal_id \
       LEFT JOIN groups grp ON g.principal_type='group' AND grp.id=g.principal_id WHERE g.application_id=a.id) AS access \
    FROM applications a WHERE a.id=$1) t";

const INFRA_DETAIL: &str = "SELECT to_jsonb(t) FROM (SELECT i.id, i.name, i.kind, i.version, \
    (SELECT json_agg(json_build_object('id', a.id, 'name', a.name, 'usage', ai.usage)) \
       FROM application_infrastructure ai JOIN applications a ON a.id=ai.application_id WHERE ai.infrastructure_id=i.id) AS applications \
    FROM infrastructure i WHERE i.id=$1) t";

const LIB_DETAIL: &str = "SELECT to_jsonb(t) FROM (SELECT l.id, l.name, l.ecosystem, \
    (SELECT json_agg(DISTINCT v.version) FROM library_versions v WHERE v.library_id=l.id) AS versions, \
    (SELECT json_agg(json_build_object('id', a.id, 'name', a.name, 'version', v.version, 'scope', al.scope)) \
       FROM library_versions v JOIN application_libraries al ON al.library_version_id=v.id \
       JOIN applications a ON a.id=al.application_id WHERE v.library_id=l.id) AS applications \
    FROM libraries l WHERE l.id=$1) t";

const USER_DETAIL: &str = "SELECT to_jsonb(t) FROM (SELECT u.id, u.username, u.email, \
    (SELECT json_agg(g.name) FROM group_memberships m JOIN groups g ON g.id=m.group_id WHERE m.user_id=u.id) AS groups, \
    (SELECT json_agg(json_build_object('id', a.id, 'name', a.name, 'access_level', ag.access_level)) \
       FROM access_grants ag JOIN applications a ON a.id=ag.application_id \
       WHERE ag.principal_type='user' AND ag.principal_id=u.id) AS applications \
    FROM users u WHERE u.id=$1) t";

const GROUP_DETAIL: &str = "SELECT to_jsonb(t) FROM (SELECT g.id, g.name, \
    (SELECT json_agg(u.username) FROM group_memberships m JOIN users u ON u.id=m.user_id WHERE m.group_id=g.id) AS members, \
    (SELECT json_agg(json_build_object('id', a.id, 'name', a.name, 'access_level', ag.access_level)) \
       FROM access_grants ag JOIN applications a ON a.id=ag.application_id \
       WHERE ag.principal_type='group' AND ag.principal_id=g.id) AS applications \
    FROM groups g WHERE g.id=$1) t";

#[async_trait]
impl PlatformQuery for PgPlatformQuery {
    async fn list(&self, entity: &str, q: &ListQuery) -> RepoResult<Page<Value>> {
        match entity {
            "applications" => self.list_for(APPS_LIST, "applications", "name", q).await,
            "infrastructure" => self.list_for(INFRA_LIST, "infrastructure", "name", q).await,
            "libraries" => self.list_for(LIBS_LIST, "libraries", "name", q).await,
            "users" => self.list_for(USERS_LIST, "users", "username", q).await,
            "groups" => self.list_for(GROUPS_LIST, "groups", "name", q).await,
            _ => Err(RepoError::Mapping(format!("unknown entity '{entity}'"))),
        }
    }

    async fn detail(&self, entity: &str, id: Uuid) -> RepoResult<Value> {
        let sql = match entity {
            "applications" => APP_DETAIL,
            "infrastructure" => INFRA_DETAIL,
            "libraries" => LIB_DETAIL,
            "users" => USER_DETAIL,
            "groups" => GROUP_DETAIL,
            _ => return Err(RepoError::Mapping(format!("unknown entity '{entity}'"))),
        };
        self.fetch_one_json(sql, id).await
    }
}
