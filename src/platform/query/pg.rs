//! PostgreSQL implementation of [`PlatformQuery`] using `to_jsonb`/`json_agg`.

use super::{ListQuery, Page, PlatformQuery, filter_clause, filter_fields, table_for};
use crate::db::{RepoError, RepoResult};
use crate::platform::catalog::{Catalog, CatalogEntry};
use crate::platform::linked::{LINKED, LinkedEntity, linked};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// The list SQL plus the table/name column its total count is derived from.
struct ListSql<'a> {
    sql: &'a str,
    table: &'a str,
    name_col: &'a str,
}

/// A filter as (allowlisted column, value).
type Filters<'a> = [(&'static str, &'a str)];

pub struct PgPlatformQuery {
    pool: PgPool,
}

impl PgPlatformQuery {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn total(&self, table: &str, name_col: &str, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<i64> {
        let clause = filter_clause(filters, "", 2, false);
        let sql = format!(
            "SELECT COUNT(*) FROM {table} WHERE ($1 = '' OR {name_col} ILIKE '%'||$1||'%'){clause}"
        );
        let mut query = sqlx::query_as::<_, (i64,)>(&sql).bind(&q.search);
        for (_, v) in filters {
            query = query.bind(*v);
        }
        let (n,) = query.fetch_one(&self.pool).await?;
        Ok(n)
    }

    async fn list_json(&self, sql: &str, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<Vec<Value>> {
        let mut query = sqlx::query_as::<_, (Value,)>(sql)
            .bind(&q.search)
            .bind(q.page_size)
            .bind(q.offset());
        for (_, v) in filters {
            query = query.bind(*v);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|(v,)| v).collect())
    }

    async fn fetch_one_json(&self, sql: &str, id: Uuid) -> RepoResult<Value> {
        let row: Option<(Value,)> = sqlx::query_as(sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|(v,)| v).ok_or(RepoError::NotFound)
    }

    async fn list_for(&self, s: ListSql<'_>, q: &ListQuery, filters: &Filters<'_>) -> RepoResult<Page<Value>> {
        let items = self.list_json(s.sql, q, filters).await?;
        let total = self.total(s.table, s.name_col, q, filters).await?;
        Ok(Page::new(items, total, q))
    }
}

fn apps_list_sql(filter: &str) -> String {
    format!(
        "SELECT to_jsonb(t) FROM (\
         SELECT a.id, a.name, a.app_type, a.primary_language, \
           (SELECT COUNT(*) FROM application_libraries al WHERE al.application_id=a.id) AS libraries, \
           (SELECT COUNT(*) FROM application_infrastructure ai WHERE ai.application_id=a.id) AS infrastructure, \
           (SELECT COUNT(*) FROM application_dependencies d WHERE d.source_app_id=a.id) AS dependencies \
         FROM applications a WHERE ($1 = '' OR a.name ILIKE '%'||$1||'%'){filter} \
         ORDER BY a.name LIMIT $2 OFFSET $3) t"
    )
}

fn libs_list_sql(filter: &str) -> String {
    format!(
        "SELECT to_jsonb(t) FROM (\
         SELECT l.id, l.name, l.ecosystem, l.metadata, \
           (SELECT COUNT(*) FROM library_versions v WHERE v.library_id=l.id) AS versions, \
           (SELECT COUNT(DISTINCT al.application_id) FROM library_versions v \
              JOIN application_libraries al ON al.library_version_id=v.id WHERE v.library_id=l.id) AS applications \
         FROM libraries l WHERE ($1 = '' OR l.name ILIKE '%'||$1||'%'){filter} \
         ORDER BY l.name LIMIT $2 OFFSET $3) t"
    )
}

const USERS_LIST: &str = "SELECT to_jsonb(t) FROM (\
    SELECT u.id, u.username, u.email, u.metadata, \
      (SELECT COUNT(*) FROM group_memberships m WHERE m.user_id=u.id) AS groups, \
      (SELECT COUNT(*) FROM access_grants g WHERE g.principal_type='user' AND g.principal_id=u.id) AS applications \
    FROM users u WHERE ($1 = '' OR u.username ILIKE '%'||$1||'%') \
    ORDER BY u.username LIMIT $2 OFFSET $3) t";

const GROUPS_LIST: &str = "SELECT to_jsonb(t) FROM (\
    SELECT g.id, g.name, g.metadata, \
      (SELECT COUNT(*) FROM group_memberships m WHERE m.group_id=g.id) AS members, \
      (SELECT COUNT(*) FROM access_grants ag WHERE ag.principal_type='group' AND ag.principal_id=g.id) AS applications \
    FROM groups g WHERE ($1 = '' OR g.name ILIKE '%'||$1||'%') \
    ORDER BY g.name LIMIT $2 OFFSET $3) t";

/// List SQL for any linked entity (infrastructure / tools / dependency types).
fn linked_list_sql(e: &LinkedEntity, filter: &str) -> String {
    format!(
        "SELECT to_jsonb(t) FROM (\
         SELECT i.id, i.name, i.kind, i.version, i.metadata, \
           (SELECT COUNT(*) FROM {join} ai WHERE ai.{fk}=i.id) AS applications \
         FROM {table} i WHERE ($1 = '' OR i.name ILIKE '%'||$1||'%'){filter} \
         ORDER BY i.name LIMIT $2 OFFSET $3) t",
        join = e.join_table,
        fk = e.fk_col,
        table = e.table
    )
}

/// Detail SQL for any linked entity (the entity plus the applications using it).
fn linked_detail_sql(e: &LinkedEntity) -> String {
    format!(
        "SELECT to_jsonb(t) FROM (SELECT i.id, i.name, i.kind, i.version, i.metadata, \
         (SELECT json_agg(json_build_object('id', a.id, 'name', a.name, 'usage', ai.usage)) \
            FROM {join} ai JOIN applications a ON a.id=ai.application_id WHERE ai.{fk}=i.id) AS applications \
         FROM {table} i WHERE i.id=$1) t",
        join = e.join_table,
        fk = e.fk_col,
        table = e.table
    )
}

/// Application detail SQL, with one sub-select per linked entity (keyed by its
/// registry name) so libraries/dependencies/linked rows all carry ids to link.
fn app_detail_sql() -> String {
    let mut linked_selects = String::new();
    for e in LINKED {
        linked_selects.push_str(&format!(
            ", (SELECT json_agg(json_build_object('id', x.id, 'name', x.name, 'kind', x.kind, \
                'version', x.version, 'usage', j.usage)) \
               FROM {join} j JOIN {table} x ON x.id=j.{fk} WHERE j.application_id=a.id) AS \"{name}\"",
            join = e.join_table,
            table = e.table,
            fk = e.fk_col,
            name = e.name
        ));
    }
    format!(
        "SELECT to_jsonb(t) FROM (SELECT \
         a.id, a.name, a.app_type, a.description, a.primary_language, a.metadata, \
         (SELECT json_agg(json_build_object('name', l.name, 'percentage', al.percentage)) \
            FROM application_languages al JOIN languages l ON l.id=al.language_id WHERE al.application_id=a.id) AS languages, \
         (SELECT json_agg(json_build_object('id', lib.id, 'name', lib.name, 'ecosystem', lib.ecosystem, \
             'version', v.version, 'scope', al.scope, 'metadata', lib.metadata)) \
            FROM application_libraries al JOIN library_versions v ON v.id=al.library_version_id \
            JOIN libraries lib ON lib.id=v.library_id WHERE al.application_id=a.id) AS libraries, \
         (SELECT json_agg(json_build_object('target_name', d.target_name, 'kind', d.kind, \
             'description', d.description, 'target_app_id', ta.id, \
             'component_id', co.id, 'component_name', co.name)) \
            FROM application_dependencies d LEFT JOIN applications ta ON ta.name=d.target_name \
            LEFT JOIN components co ON co.id=d.component_id \
            WHERE d.source_app_id=a.id) AS dependencies, \
         (SELECT json_agg(json_build_object('principal_type', g.principal_type, 'access_level', g.access_level, \
             'association_type', g.association_type, 'permissions', g.permissions, \
             'principal_name', CASE WHEN g.principal_type='user' THEN u.username ELSE grp.name END)) \
            FROM access_grants g LEFT JOIN users u ON g.principal_type='user' AND u.id=g.principal_id \
            LEFT JOIN groups grp ON g.principal_type='group' AND grp.id=g.principal_id WHERE g.application_id=a.id) AS access, \
         (SELECT json_agg(json_build_object('id', c.id, 'name', c.name, 'kind', c.kind, \
             'description', c.description, 'metadata', c.metadata, 'observability_signals', \
             (SELECT json_agg(json_build_object('id', s.id, 'name', s.name, 'kind', s.kind, 'description', s.description)) \
                FROM observability_signals s WHERE s.component_id=c.id))) \
            FROM components c WHERE c.application_id=a.id) AS components, \
         (SELECT json_agg(json_build_object('id', uc.id, 'name', uc.name, 'description', uc.description, \
             'metadata', uc.metadata, 'components', \
             (SELECT json_agg(json_build_object('id', cc.id, 'name', cc.name)) \
                FROM use_case_components ucc JOIN components cc ON cc.id=ucc.component_id WHERE ucc.use_case_id=uc.id), \
             'diagrams', \
             (SELECT json_agg(json_build_object('id', dg.id, 'name', dg.name, 'kind', dg.kind, \
                 'description', dg.description, 'content', dg.content)) \
                FROM diagrams dg WHERE dg.use_case_id=uc.id))) \
            FROM use_cases uc WHERE uc.application_id=a.id) AS use_cases\
         {linked} \
         FROM applications a WHERE a.id=$1) t",
        linked = linked_selects
    )
}

const LIB_DETAIL: &str = "SELECT to_jsonb(t) FROM (SELECT l.id, l.name, l.ecosystem, l.metadata, \
    (SELECT json_agg(DISTINCT v.version) FROM library_versions v WHERE v.library_id=l.id) AS versions, \
    (SELECT json_agg(json_build_object('id', a.id, 'name', a.name, 'version', v.version, 'scope', al.scope)) \
       FROM library_versions v JOIN application_libraries al ON al.library_version_id=v.id \
       JOIN applications a ON a.id=al.application_id WHERE v.library_id=l.id) AS applications \
    FROM libraries l WHERE l.id=$1) t";

const USER_DETAIL: &str = "SELECT to_jsonb(t) FROM (SELECT u.id, u.username, u.email, u.metadata, \
    (SELECT json_agg(g.name) FROM group_memberships m JOIN groups g ON g.id=m.group_id WHERE m.user_id=u.id) AS groups, \
    (SELECT json_agg(json_build_object('id', a.id, 'name', a.name, 'access_level', ag.access_level, \
        'association_type', ag.association_type, 'permissions', ag.permissions)) \
       FROM access_grants ag JOIN applications a ON a.id=ag.application_id \
       WHERE ag.principal_type='user' AND ag.principal_id=u.id) AS applications \
    FROM users u WHERE u.id=$1) t";

const GROUP_DETAIL: &str = "SELECT to_jsonb(t) FROM (SELECT g.id, g.name, g.metadata, \
    (SELECT json_agg(u.username) FROM group_memberships m JOIN users u ON u.id=m.user_id WHERE m.group_id=g.id) AS members, \
    (SELECT json_agg(json_build_object('id', a.id, 'name', a.name, 'access_level', ag.access_level, \
        'association_type', ag.association_type, 'permissions', ag.permissions)) \
       FROM access_grants ag JOIN applications a ON a.id=ag.application_id \
       WHERE ag.principal_type='group' AND ag.principal_id=g.id) AS applications \
    FROM groups g WHERE g.id=$1) t";

#[async_trait]
impl PlatformQuery for PgPlatformQuery {
    async fn list(&self, entity: &str, q: &ListQuery) -> RepoResult<Page<Value>> {
        let filters = q.effective_filters(entity);
        if let Some(e) = linked(entity) {
            let sql = linked_list_sql(e, &filter_clause(&filters, "i.", 4, false));
            return self
                .list_for(ListSql { sql: &sql, table: e.table, name_col: "name" }, q, &filters)
                .await;
        }
        match entity {
            "applications" => {
                let sql = apps_list_sql(&filter_clause(&filters, "a.", 4, false));
                self.list_for(ListSql { sql: &sql, table: "applications", name_col: "name" }, q, &filters)
                    .await
            }
            "libraries" => {
                let sql = libs_list_sql(&filter_clause(&filters, "l.", 4, false));
                self.list_for(ListSql { sql: &sql, table: "libraries", name_col: "name" }, q, &filters)
                    .await
            }
            "users" => {
                self.list_for(ListSql { sql: USERS_LIST, table: "users", name_col: "username" }, q, &filters)
                    .await
            }
            "groups" => {
                self.list_for(ListSql { sql: GROUPS_LIST, table: "groups", name_col: "name" }, q, &filters)
                    .await
            }
            _ => Err(RepoError::Mapping(format!("unknown entity '{entity}'"))),
        }
    }

    async fn detail(&self, entity: &str, id: Uuid) -> RepoResult<Value> {
        if let Some(e) = linked(entity) {
            let sql = linked_detail_sql(e);
            return self.fetch_one_json(&sql, id).await;
        }
        let app_detail;
        let sql = match entity {
            "applications" => {
                app_detail = app_detail_sql();
                app_detail.as_str()
            }
            "libraries" => LIB_DETAIL,
            "users" => USER_DETAIL,
            "groups" => GROUP_DETAIL,
            _ => return Err(RepoError::Mapping(format!("unknown entity '{entity}'"))),
        };
        self.fetch_one_json(sql, id).await
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
}
