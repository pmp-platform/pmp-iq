//! Persists an [`AnalysisResult`] into the normalised platform model with
//! idempotent find-or-create upserts. The SQL is engine-portable, so the same
//! body backs both the Postgres and SQLite implementations.

use super::analysis::{AnalysisResult, AppInfo, MemberInfo};
use super::changes::{Change, application_change, diff_keys};
use super::linked::LINKED;
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

const UNKNOWN_VERSION: &str = "unknown";

/// Writes analysis results into the platform model.
#[async_trait]
pub trait PlatformWriter: Send + Sync {
    /// Persist `result` for the given repository; returns the application id.
    async fn write(&self, repository_id: Uuid, result: &AnalysisResult) -> RepoResult<Uuid>;

    /// Partial merge (M41 incremental): upsert only the components/use cases in
    /// `result` for `app_id`, leaving untouched entities (and their attribution)
    /// intact — no delete-and-recreate, no orphan prune.
    async fn write_partial(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()>;

    /// Reconcile the current git-provider members of `app_id`: upsert each as a
    /// `member` (with role + permissions) and flip any previously-recorded member
    /// that is no longer present to `ex_member`. Codeowner grants are untouched.
    async fn reconcile_members(&self, app_id: Uuid, members: &[MemberInfo]) -> RepoResult<()>;

    /// Delete shared analysis entities no longer referenced by any application
    /// (libraries/versions, languages, and linked entities). Users and groups
    /// are left intact to preserve member/ex-member history.
    async fn prune_orphans(&self) -> RepoResult<()>;
}

macro_rules! platform_writer_impl {
    ($name:ident, $pool:ty, $xform:path) => {
        pub struct $name {
            pool: $pool,
        }
        impl $name {
            pub fn new(pool: $pool) -> Self {
                Self { pool }
            }

            /// Prior `(id, app_type, description)` for an app keyed by its repo
            /// (M36), captured before the upsert overwrites it.
            async fn app_prior(&self, repository_id: Uuid) -> RepoResult<Option<(Uuid, String, String)>> {
                let row: Option<(Uuid, Option<String>, Option<String>)> = sqlx::query_as(&$xform(
                    "SELECT id, app_type, description FROM applications WHERE repository_id=$1",
                ))
                .bind(repository_id)
                .fetch_optional(&self.pool)
                .await?;
                Ok(row.map(|(id, t, d)| (id, t.unwrap_or_default(), d.unwrap_or_default())))
            }

            /// The current dependency target names for an app (its natural keys).
            async fn dependency_keys(&self, app_id: Uuid) -> RepoResult<Vec<String>> {
                let rows: Vec<(String,)> = sqlx::query_as(&$xform(
                    "SELECT target_name FROM application_dependencies WHERE source_app_id=$1",
                ))
                .bind(app_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(t,)| t).collect())
            }

            /// Emit the application + dependency change events for this sync.
            async fn emit_changes(
                &self,
                app_id: Uuid,
                prior_app: &Option<(Uuid, String, String)>,
                prior_deps: &[String],
                result: &AnalysisResult,
            ) -> RepoResult<()> {
                let next_type = result.application.app_type.clone().unwrap_or_default();
                let next_desc = result.application.description.clone().unwrap_or_default();
                let prior = prior_app.as_ref().map(|(_, t, d)| (t.as_str(), d.as_str()));
                let mut changes: Vec<Change> = Vec::new();
                if let Some(c) = application_change(&result.application.name, prior, &next_type, &next_desc) {
                    changes.push(c);
                }
                let next_deps: Vec<String> =
                    result.dependencies.iter().map(|d| d.target_name.clone()).collect();
                changes.extend(diff_keys("dependency", prior_deps, &next_deps));
                for change in &changes {
                    self.record_change(app_id, change).await?;
                }
                Ok(())
            }

            async fn record_change(&self, app_id: Uuid, change: &Change) -> RepoResult<()> {
                let detail = if change.detail.is_null() { None } else { Some(change.detail.clone()) };
                sqlx::query(&$xform(
                    "INSERT INTO platform_changes \
                       (id, application_id, entity_type, entity_key, change, detail) \
                     VALUES ($1,$2,$3,$4,$5,$6)",
                ))
                .bind(Uuid::new_v4())
                .bind(app_id)
                .bind(&change.entity_type)
                .bind(&change.entity_key)
                .bind(change.kind.as_str())
                .bind(detail)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn upsert_application(&self, repository_id: Uuid, app: &AppInfo) -> RepoResult<Uuid> {
                let id = Uuid::new_v4();
                let (app_id,): (Uuid,) = sqlx::query_as(&$xform(
                    "INSERT INTO applications (id, repository_id, name, app_type, description, \
                     primary_language, metadata, last_analyzed_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7, CURRENT_TIMESTAMP) \
                     ON CONFLICT (repository_id) DO UPDATE SET name=EXCLUDED.name, \
                     app_type=EXCLUDED.app_type, description=EXCLUDED.description, \
                     primary_language=EXCLUDED.primary_language, metadata=EXCLUDED.metadata, \
                     last_analyzed_at=CURRENT_TIMESTAMP, updated_at=CURRENT_TIMESTAMP RETURNING id",
                ))
                .bind(id)
                .bind(repository_id)
                .bind(&app.name)
                .bind(&app.app_type)
                .bind(&app.description)
                .bind(&app.primary_language)
                .bind(&app.metadata)
                .fetch_one(&self.pool)
                .await?;
                Ok(app_id)
            }

            async fn clear_associations(&self, app_id: Uuid) -> RepoResult<()> {
                for entity in LINKED {
                    let sql = $xform(&format!(
                        "DELETE FROM {} WHERE application_id=$1",
                        entity.join_table
                    ));
                    sqlx::query(&sql).bind(app_id).execute(&self.pool).await?;
                }
                for table in [
                    "DELETE FROM application_languages WHERE application_id=$1",
                    "DELETE FROM application_libraries WHERE application_id=$1",
                    "DELETE FROM application_dependencies WHERE source_app_id=$1",
                    // Only the AI-derived codeowner grants are refreshed each run;
                    // member/ex_member history is preserved across analyses.
                    "DELETE FROM access_grants WHERE application_id=$1 AND association_type='codeowner'",
                    // App-owned sub-entities are rebuilt each sync; CASCADE clears
                    // observability_signals, diagrams, use_case_components and
                    // endpoint_files.
                    "DELETE FROM api_endpoints WHERE application_id=$1",
                    "DELETE FROM components WHERE application_id=$1",
                    "DELETE FROM use_cases WHERE application_id=$1",
                ] {
                    sqlx::query(&$xform(table)).bind(app_id).execute(&self.pool).await?;
                }
                Ok(())
            }

            /// Find-or-create a linked-entity row, returning its id.
            async fn upsert_linked(
                &self,
                table: &str,
                name: &str,
                kind: &str,
                version: &str,
                metadata: &Value,
            ) -> RepoResult<Uuid> {
                let id = Uuid::new_v4();
                let sql = $xform(&format!(
                    "INSERT INTO {table} (id, name, kind, version, metadata) VALUES ($1,$2,$3,$4,$5) \
                     ON CONFLICT (name, kind, version) DO UPDATE SET name=EXCLUDED.name RETURNING id"
                ));
                let (out,): (Uuid,) = sqlx::query_as(&sql)
                    .bind(id)
                    .bind(name)
                    .bind(kind)
                    .bind(version)
                    .bind(metadata)
                    .fetch_one(&self.pool)
                    .await?;
                Ok(out)
            }

            /// Link an application to a linked entity via its join table.
            async fn link_app(
                &self,
                join_table: &str,
                fk_col: &str,
                app_id: Uuid,
                entity_id: Uuid,
                usage: Option<&str>,
            ) -> RepoResult<()> {
                let sql = $xform(&format!(
                    "INSERT INTO {join_table} (application_id, {fk_col}, usage) \
                     VALUES ($1,$2,$3) ON CONFLICT DO NOTHING"
                ));
                sqlx::query(&sql)
                    .bind(app_id)
                    .bind(entity_id)
                    .bind(usage)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn find_or_create_id(&self, sql: &str, binds: &[&str]) -> RepoResult<Uuid> {
                let id = Uuid::new_v4();
                let translated = $xform(sql);
                let mut query = sqlx::query_as::<_, (Uuid,)>(&translated).bind(id);
                for b in binds {
                    query = query.bind(*b);
                }
                let (out,): (Uuid,) = query.fetch_one(&self.pool).await?;
                Ok(out)
            }

            async fn find_or_create_version(&self, library_id: Uuid, version: &str) -> RepoResult<Uuid> {
                let id = Uuid::new_v4();
                let (ver_id,): (Uuid,) = sqlx::query_as(&$xform(
                    "INSERT INTO library_versions (id, library_id, version) VALUES ($1,$2,$3) \
                     ON CONFLICT (library_id, version) DO UPDATE SET version=EXCLUDED.version RETURNING id",
                ))
                .bind(id)
                .bind(library_id)
                .bind(version)
                .fetch_one(&self.pool)
                .await?;
                Ok(ver_id)
            }

            /// Find-or-create a library by (name, ecosystem), setting metadata on insert.
            async fn upsert_library(&self, name: &str, ecosystem: &str, metadata: &Value) -> RepoResult<Uuid> {
                let id = Uuid::new_v4();
                let (out,): (Uuid,) = sqlx::query_as(&$xform(
                    "INSERT INTO libraries (id, name, ecosystem, metadata) VALUES ($1,$2,$3,$4) \
                     ON CONFLICT (name, ecosystem) DO UPDATE SET name=EXCLUDED.name RETURNING id",
                ))
                .bind(id)
                .bind(name)
                .bind(ecosystem)
                .bind(metadata)
                .fetch_one(&self.pool)
                .await?;
                Ok(out)
            }

            async fn find_or_create_group(&self, name: &str, metadata: &Value) -> RepoResult<Uuid> {
                let id = Uuid::new_v4();
                let (group_id,): (Uuid,) = sqlx::query_as(&$xform(
                    "INSERT INTO groups (id, name, metadata) VALUES ($1,$2,$3) \
                     ON CONFLICT (name) DO UPDATE SET name=EXCLUDED.name RETURNING id",
                ))
                .bind(id)
                .bind(name)
                .bind(metadata)
                .fetch_one(&self.pool)
                .await?;
                Ok(group_id)
            }

            async fn find_or_create_user(&self, username: &str, email: Option<&str>, metadata: &Value) -> RepoResult<Uuid> {
                let id = Uuid::new_v4();
                let (user_id,): (Uuid,) = sqlx::query_as(&$xform(
                    "INSERT INTO users (id, username, email, metadata) VALUES ($1,$2,$3,$4) \
                     ON CONFLICT (username) DO UPDATE SET email=COALESCE(EXCLUDED.email, users.email) RETURNING id",
                ))
                .bind(id)
                .bind(username)
                .bind(email)
                .bind(metadata)
                .fetch_one(&self.pool)
                .await?;
                Ok(user_id)
            }

            async fn add_membership(&self, group_id: Uuid, user_id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO group_memberships (group_id, user_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
                ))
                .bind(group_id)
                .bind(user_id)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn write_languages(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()> {
                for lang in &result.languages {
                    let lang_id = self
                        .find_or_create_id(
                            "INSERT INTO languages (id, name) VALUES ($1,$2) \
                             ON CONFLICT (name) DO UPDATE SET name=EXCLUDED.name RETURNING id",
                            &[&lang.name],
                        )
                        .await?;
                    sqlx::query(&$xform(
                        "INSERT INTO application_languages (application_id, language_id, percentage) \
                         VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
                    ))
                    .bind(app_id)
                    .bind(lang_id)
                    .bind(lang.percentage)
                    .execute(&self.pool)
                    .await?;
                }
                Ok(())
            }

            async fn write_libraries(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()> {
                for lib in &result.libraries {
                    let lib_id = self.upsert_library(&lib.name, &lib.ecosystem, &lib.metadata).await?;
                    let version = lib.version.as_deref().unwrap_or(UNKNOWN_VERSION);
                    let ver_id = self.find_or_create_version(lib_id, version).await?;
                    sqlx::query(&$xform(
                        "INSERT INTO application_libraries (application_id, library_version_id, scope) \
                         VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
                    ))
                    .bind(app_id)
                    .bind(ver_id)
                    .bind(&lib.scope)
                    .execute(&self.pool)
                    .await?;
                }
                Ok(())
            }

            /// Write every linked-entity array (infrastructure, tools, cloud
            /// providers, services, platforms, external) from the registry.
            async fn write_linked(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()> {
                for entity in LINKED {
                    for item in result.linked_items(entity.name) {
                        let version = item.version.as_deref().unwrap_or(UNKNOWN_VERSION);
                        let entity_id = self
                            .upsert_linked(entity.table, &item.name, &item.kind, version, &item.metadata)
                            .await?;
                        self.link_app(
                            entity.join_table,
                            entity.fk_col,
                            app_id,
                            entity_id,
                            item.usage.as_deref(),
                        )
                        .await?;
                    }
                }
                Ok(())
            }

            /// Insert the application's exposed API endpoints (M42), linking each
            /// to its implementing component; drop endpoints with an unsupported
            /// protocol.
            async fn write_endpoints(
                &self,
                app_id: Uuid,
                components: &HashMap<String, Uuid>,
                result: &AnalysisResult,
            ) -> RepoResult<()> {
                for ep in &result.endpoints {
                    if !ep.protocol_allowed() {
                        continue;
                    }
                    let component_id = ep.component.as_ref().and_then(|n| components.get(n)).copied();
                    let (endpoint_id,): (Uuid,) = sqlx::query_as(&$xform(
                        "INSERT INTO api_endpoints (id, application_id, protocol, operation, summary, component_id, metadata) \
                         VALUES ($1,$2,$3,$4,$5,$6,$7) \
                         ON CONFLICT (application_id, protocol, operation) DO UPDATE SET \
                         summary=EXCLUDED.summary, component_id=EXCLUDED.component_id, metadata=EXCLUDED.metadata RETURNING id",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(app_id)
                    .bind(&ep.protocol)
                    .bind(&ep.operation)
                    .bind(&ep.summary)
                    .bind(component_id)
                    .bind(&ep.metadata)
                    .fetch_one(&self.pool)
                    .await?;
                    for path in &ep.files {
                        sqlx::query(&$xform(
                            "INSERT INTO endpoint_files (endpoint_id, path) VALUES ($1,$2) ON CONFLICT DO NOTHING",
                        ))
                        .bind(endpoint_id)
                        .bind(path)
                        .execute(&self.pool)
                        .await?;
                    }
                }
                Ok(())
            }

            /// Resolve a dependency's called operation to a producer endpoint id
            /// (M42): the endpoint of the application named `target_name`.
            async fn resolve_endpoint(&self, target_name: &str, operation: &str) -> RepoResult<Option<Uuid>> {
                let row: Option<(Uuid,)> = sqlx::query_as(&$xform(
                    "SELECT e.id FROM api_endpoints e JOIN applications a ON a.id=e.application_id \
                     WHERE a.name=$1 AND e.operation=$2",
                ))
                .bind(target_name)
                .bind(operation)
                .fetch_optional(&self.pool)
                .await?;
                Ok(row.map(|(id,)| id))
            }

            async fn write_dependencies(
                &self,
                app_id: Uuid,
                components: &HashMap<String, Uuid>,
                result: &AnalysisResult,
            ) -> RepoResult<()> {
                for dep in &result.dependencies {
                    let component_id = dep
                        .component
                        .as_ref()
                        .and_then(|name| components.get(name))
                        .copied();
                    let endpoint_id = match dep.endpoint.as_deref().filter(|o| !o.is_empty()) {
                        Some(operation) => self.resolve_endpoint(&dep.target_name, operation).await?,
                        None => None,
                    };
                    sqlx::query(&$xform(
                        "INSERT INTO application_dependencies (id, source_app_id, component_id, target_name, kind, description, endpoint_id) \
                         VALUES ($1,$2,$3,$4,$5,$6,$7) \
                         ON CONFLICT (source_app_id, component_id, target_name, kind) DO UPDATE SET endpoint_id=EXCLUDED.endpoint_id",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(app_id)
                    .bind(component_id)
                    .bind(&dep.target_name)
                    .bind(&dep.kind)
                    .bind(&dep.description)
                    .bind(endpoint_id)
                    .execute(&self.pool)
                    .await?;
                }
                Ok(())
            }

            async fn write_access(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()> {
                let empty = serde_json::json!({});
                let mut groups: HashMap<String, Uuid> = HashMap::new();
                let mut users: HashMap<String, Uuid> = HashMap::new();
                for group in &result.groups {
                    groups.insert(group.name.clone(), self.find_or_create_group(&group.name, &group.metadata).await?);
                }
                for user in &result.users {
                    let user_id = self.find_or_create_user(&user.username, user.email.as_deref(), &user.metadata).await?;
                    users.insert(user.username.clone(), user_id);
                    for group_name in &user.groups {
                        let group_id = self.find_or_create_group(group_name, &empty).await?;
                        groups.insert(group_name.clone(), group_id);
                        self.add_membership(group_id, user_id).await?;
                    }
                }
                for grant in &result.access {
                    let principal_id = if grant.principal_type == "group" {
                        match groups.get(&grant.principal_name) {
                            Some(id) => *id,
                            None => {
                                let id = self.find_or_create_group(&grant.principal_name, &empty).await?;
                                groups.insert(grant.principal_name.clone(), id);
                                id
                            }
                        }
                    } else {
                        match users.get(&grant.principal_name) {
                            Some(id) => *id,
                            None => {
                                let id = self.find_or_create_user(&grant.principal_name, None, &empty).await?;
                                users.insert(grant.principal_name.clone(), id);
                                id
                            }
                        }
                    };
                    // AI access is sourced from CODEOWNERS → 'codeowner'. A real
                    // provider member for the same principal already exists and
                    // outranks this, so a conflict is left untouched.
                    sqlx::query(&$xform(
                        "INSERT INTO access_grants (id, application_id, principal_type, principal_id, association_type, access_level) \
                         VALUES ($1,$2,$3,$4,'codeowner',$5) \
                         ON CONFLICT (application_id, principal_type, principal_id) DO NOTHING",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(app_id)
                    .bind(&grant.principal_type)
                    .bind(principal_id)
                    .bind(&grant.access_level)
                    .execute(&self.pool)
                    .await?;
                }
                Ok(())
            }

            /// Flip members no longer present to `ex_member`. With an empty
            /// current set, every recorded member becomes an ex-member.
            async fn flip_ex_members(&self, app_id: Uuid, current: &[Uuid]) -> RepoResult<()> {
                if current.is_empty() {
                    sqlx::query(&$xform(
                        "UPDATE access_grants SET association_type='ex_member' \
                         WHERE application_id=$1 AND association_type='member'",
                    ))
                    .bind(app_id)
                    .execute(&self.pool)
                    .await?;
                    return Ok(());
                }
                let placeholders: Vec<String> =
                    (0..current.len()).map(|i| format!("${}", i + 2)).collect();
                let sql = $xform(&format!(
                    "UPDATE access_grants SET association_type='ex_member' \
                     WHERE application_id=$1 AND association_type='member' \
                     AND principal_id NOT IN ({})",
                    placeholders.join(",")
                ));
                let mut query = sqlx::query(&sql).bind(app_id);
                for id in current {
                    query = query.bind(*id);
                }
                query.execute(&self.pool).await?;
                Ok(())
            }

            /// Insert the application's components and the observability signals
            /// each emits; return a component name→id map for use-case linking.
            async fn write_components(
                &self,
                app_id: Uuid,
                result: &AnalysisResult,
            ) -> RepoResult<HashMap<String, Uuid>> {
                let mut ids = HashMap::new();
                for component in &result.components {
                    let (component_id,): (Uuid,) = sqlx::query_as(&$xform(
                        "INSERT INTO components (id, application_id, name, kind, description, metadata) \
                         VALUES ($1,$2,$3,$4,$5,$6) \
                         ON CONFLICT (application_id, name) DO UPDATE SET kind=EXCLUDED.kind, \
                         description=EXCLUDED.description, metadata=EXCLUDED.metadata RETURNING id",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(app_id)
                    .bind(&component.name)
                    .bind(&component.kind)
                    .bind(&component.description)
                    .bind(&component.metadata)
                    .fetch_one(&self.pool)
                    .await?;
                    for signal in &component.observability_signals {
                        sqlx::query(&$xform(
                            "INSERT INTO observability_signals (id, component_id, name, kind, description, metadata) \
                             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (component_id, name) DO NOTHING",
                        ))
                        .bind(Uuid::new_v4())
                        .bind(component_id)
                        .bind(&signal.name)
                        .bind(&signal.kind)
                        .bind(&signal.description)
                        .bind(&signal.metadata)
                        .execute(&self.pool)
                        .await?;
                    }
                    for path in &component.files {
                        sqlx::query(&$xform(
                            "INSERT INTO component_files (component_id, path) \
                             VALUES ($1,$2) ON CONFLICT DO NOTHING",
                        ))
                        .bind(component_id)
                        .bind(path)
                        .execute(&self.pool)
                        .await?;
                    }
                    ids.insert(component.name.clone(), component_id);
                }
                Ok(ids)
            }

            /// Insert the application's use cases, linking them to the resolved
            /// components and inserting their mermaid diagrams.
            async fn write_use_cases(
                &self,
                app_id: Uuid,
                components: &HashMap<String, Uuid>,
                result: &AnalysisResult,
            ) -> RepoResult<()> {
                for use_case in &result.use_cases {
                    let (use_case_id,): (Uuid,) = sqlx::query_as(&$xform(
                        "INSERT INTO use_cases (id, application_id, name, description, metadata) \
                         VALUES ($1,$2,$3,$4,$5) \
                         ON CONFLICT (application_id, name) DO UPDATE SET description=EXCLUDED.description, \
                         metadata=EXCLUDED.metadata RETURNING id",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(app_id)
                    .bind(&use_case.name)
                    .bind(&use_case.description)
                    .bind(&use_case.metadata)
                    .fetch_one(&self.pool)
                    .await?;
                    for component_name in &use_case.components {
                        if let Some(component_id) = components.get(component_name) {
                            sqlx::query(&$xform(
                                "INSERT INTO use_case_components (use_case_id, component_id) \
                                 VALUES ($1,$2) ON CONFLICT DO NOTHING",
                            ))
                            .bind(use_case_id)
                            .bind(*component_id)
                            .execute(&self.pool)
                            .await?;
                        }
                    }
                    for diagram in &use_case.diagrams {
                        sqlx::query(&$xform(
                            "INSERT INTO diagrams (id, use_case_id, name, kind, description, content, metadata) \
                             VALUES ($1,$2,$3,$4,$5,$6,$7) \
                             ON CONFLICT (use_case_id, name) DO UPDATE SET kind=EXCLUDED.kind, \
                             description=EXCLUDED.description, content=EXCLUDED.content, metadata=EXCLUDED.metadata",
                        ))
                        .bind(Uuid::new_v4())
                        .bind(use_case_id)
                        .bind(&diagram.name)
                        .bind(&diagram.kind)
                        .bind(&diagram.description)
                        .bind(&diagram.content)
                        .bind(&diagram.metadata)
                        .execute(&self.pool)
                        .await?;
                    }
                    for path in &use_case.files {
                        sqlx::query(&$xform(
                            "INSERT INTO use_case_files (use_case_id, path) \
                             VALUES ($1,$2) ON CONFLICT DO NOTHING",
                        ))
                        .bind(use_case_id)
                        .bind(path)
                        .execute(&self.pool)
                        .await?;
                    }
                }
                Ok(())
            }
        }

        #[async_trait]
        impl PlatformWriter for $name {
            async fn write(&self, repository_id: Uuid, result: &AnalysisResult) -> RepoResult<Uuid> {
                // Snapshot prior state (M36) before the delete-and-recreate so we
                // can diff it against the new model and emit precise changes.
                let prior_app = self.app_prior(repository_id).await?;
                let prior_deps = match &prior_app {
                    Some((id, _, _)) => self.dependency_keys(*id).await?,
                    None => Vec::new(),
                };
                let app_id = self.upsert_application(repository_id, &result.application).await?;
                self.clear_associations(app_id).await?;
                self.write_languages(app_id, result).await?;
                self.write_libraries(app_id, result).await?;
                self.write_linked(app_id, result).await?;
                self.write_access(app_id, result).await?;
                // Components first so endpoints, dependencies and use cases can
                // link to them.
                let components = self.write_components(app_id, result).await?;
                self.write_endpoints(app_id, &components, result).await?;
                self.write_dependencies(app_id, &components, result).await?;
                self.write_use_cases(app_id, &components, result).await?;
                self.emit_changes(app_id, &prior_app, &prior_deps, result).await?;
                Ok(app_id)
            }

            async fn write_partial(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()> {
                // Upsert only the affected components/use cases/endpoints (their
                // ids are preserved via ON CONFLICT, so dependencies keep their
                // link). Untouched siblings are left intact — no clear, no prune.
                let components = self.write_components(app_id, result).await?;
                self.write_endpoints(app_id, &components, result).await?;
                self.write_use_cases(app_id, &components, result).await?;
                for component in &result.components {
                    self.record_change(
                        app_id,
                        &Change::new("component", &component.name, super::changes::ChangeKind::Updated, Value::Null),
                    )
                    .await?;
                }
                Ok(())
            }

            async fn reconcile_members(&self, app_id: Uuid, members: &[MemberInfo]) -> RepoResult<()> {
                let mut current: Vec<Uuid> = Vec::with_capacity(members.len());
                for member in members {
                    let user_id = self
                        .find_or_create_user(&member.username, member.email.as_deref(), &member.metadata)
                        .await?;
                    sqlx::query(&$xform(
                        "INSERT INTO access_grants \
                         (id, application_id, principal_type, principal_id, association_type, access_level, permissions) \
                         VALUES ($1,$2,'user',$3,'member',$4,$5) \
                         ON CONFLICT (application_id, principal_type, principal_id) DO UPDATE SET \
                         association_type='member', access_level=EXCLUDED.access_level, permissions=EXCLUDED.permissions",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(app_id)
                    .bind(user_id)
                    .bind(&member.role)
                    .bind(&member.permissions)
                    .execute(&self.pool)
                    .await?;
                    current.push(user_id);
                }
                self.flip_ex_members(app_id, &current).await
            }

            async fn prune_orphans(&self) -> RepoResult<()> {
                for entity in LINKED {
                    let sql = $xform(&format!(
                        "DELETE FROM {table} WHERE NOT EXISTS \
                         (SELECT 1 FROM {join} j WHERE j.{fk}={table}.id)",
                        table = entity.table,
                        join = entity.join_table,
                        fk = entity.fk_col
                    ));
                    sqlx::query(&sql).execute(&self.pool).await?;
                }
                for sql in [
                    "DELETE FROM library_versions WHERE NOT EXISTS \
                     (SELECT 1 FROM application_libraries al WHERE al.library_version_id=library_versions.id)",
                    "DELETE FROM libraries WHERE NOT EXISTS \
                     (SELECT 1 FROM library_versions v WHERE v.library_id=libraries.id)",
                    "DELETE FROM languages WHERE NOT EXISTS \
                     (SELECT 1 FROM application_languages al WHERE al.language_id=languages.id)",
                ] {
                    sqlx::query(&$xform(sql)).execute(&self.pool).await?;
                }
                Ok(())
            }
        }
    };
}

platform_writer_impl!(PgPlatformWriter, PgPool, identity);
platform_writer_impl!(SqlitePlatformWriter, SqlitePool, to_sqlite);
