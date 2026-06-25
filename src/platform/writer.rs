//! Persists an [`AnalysisResult`] into the normalised platform model with
//! idempotent find-or-create upserts. The SQL is engine-portable, so the same
//! body backs both the Postgres and SQLite implementations.

use super::analysis::{AnalysisResult, AppInfo};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{PgPool, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

const UNKNOWN_VERSION: &str = "unknown";

/// Writes analysis results into the platform model.
#[async_trait]
pub trait PlatformWriter: Send + Sync {
    /// Persist `result` for the given repository; returns the application id.
    async fn write(&self, repository_id: Uuid, result: &AnalysisResult) -> RepoResult<Uuid>;
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
                for table in [
                    "DELETE FROM application_languages WHERE application_id=$1",
                    "DELETE FROM application_libraries WHERE application_id=$1",
                    "DELETE FROM application_infrastructure WHERE application_id=$1",
                    "DELETE FROM application_dependencies WHERE source_app_id=$1",
                    "DELETE FROM access_grants WHERE application_id=$1",
                ] {
                    sqlx::query(&$xform(table)).bind(app_id).execute(&self.pool).await?;
                }
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

            async fn find_or_create_group(&self, name: &str) -> RepoResult<Uuid> {
                self.find_or_create_id(
                    "INSERT INTO groups (id, name) VALUES ($1,$2) \
                     ON CONFLICT (name) DO UPDATE SET name=EXCLUDED.name RETURNING id",
                    &[name],
                )
                .await
            }

            async fn find_or_create_user(&self, username: &str, email: Option<&str>) -> RepoResult<Uuid> {
                let id = Uuid::new_v4();
                let (user_id,): (Uuid,) = sqlx::query_as(&$xform(
                    "INSERT INTO users (id, username, email) VALUES ($1,$2,$3) \
                     ON CONFLICT (username) DO UPDATE SET email=COALESCE(EXCLUDED.email, users.email) RETURNING id",
                ))
                .bind(id)
                .bind(username)
                .bind(email)
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
                    let lib_id = self
                        .find_or_create_id(
                            "INSERT INTO libraries (id, name, ecosystem) VALUES ($1,$2,$3) \
                             ON CONFLICT (name, ecosystem) DO UPDATE SET name=EXCLUDED.name RETURNING id",
                            &[&lib.name, &lib.ecosystem],
                        )
                        .await?;
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

            async fn write_infrastructure(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()> {
                for infra in &result.infrastructure {
                    let version = infra.version.as_deref().unwrap_or(UNKNOWN_VERSION);
                    let infra_id = self
                        .find_or_create_id(
                            "INSERT INTO infrastructure (id, name, kind, version) VALUES ($1,$2,$3,$4) \
                             ON CONFLICT (name, kind, version) DO UPDATE SET name=EXCLUDED.name RETURNING id",
                            &[&infra.name, &infra.kind, version],
                        )
                        .await?;
                    sqlx::query(&$xform(
                        "INSERT INTO application_infrastructure (application_id, infrastructure_id, usage) \
                         VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
                    ))
                    .bind(app_id)
                    .bind(infra_id)
                    .bind(&infra.usage)
                    .execute(&self.pool)
                    .await?;
                }
                Ok(())
            }

            async fn write_dependencies(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()> {
                for dep in &result.dependencies {
                    sqlx::query(&$xform(
                        "INSERT INTO application_dependencies (id, source_app_id, target_name, kind, description) \
                         VALUES ($1,$2,$3,$4,$5) ON CONFLICT (source_app_id, target_name, kind) DO NOTHING",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(app_id)
                    .bind(&dep.target_name)
                    .bind(&dep.kind)
                    .bind(&dep.description)
                    .execute(&self.pool)
                    .await?;
                }
                Ok(())
            }

            async fn write_access(&self, app_id: Uuid, result: &AnalysisResult) -> RepoResult<()> {
                let mut groups: HashMap<String, Uuid> = HashMap::new();
                let mut users: HashMap<String, Uuid> = HashMap::new();
                for group in &result.groups {
                    groups.insert(group.name.clone(), self.find_or_create_group(&group.name).await?);
                }
                for user in &result.users {
                    let user_id = self.find_or_create_user(&user.username, user.email.as_deref()).await?;
                    users.insert(user.username.clone(), user_id);
                    for group_name in &user.groups {
                        let group_id = self.find_or_create_group(group_name).await?;
                        groups.insert(group_name.clone(), group_id);
                        self.add_membership(group_id, user_id).await?;
                    }
                }
                for grant in &result.access {
                    let principal_id = if grant.principal_type == "group" {
                        match groups.get(&grant.principal_name) {
                            Some(id) => *id,
                            None => {
                                let id = self.find_or_create_group(&grant.principal_name).await?;
                                groups.insert(grant.principal_name.clone(), id);
                                id
                            }
                        }
                    } else {
                        match users.get(&grant.principal_name) {
                            Some(id) => *id,
                            None => {
                                let id = self.find_or_create_user(&grant.principal_name, None).await?;
                                users.insert(grant.principal_name.clone(), id);
                                id
                            }
                        }
                    };
                    sqlx::query(&$xform(
                        "INSERT INTO access_grants (id, application_id, principal_type, principal_id, access_level) \
                         VALUES ($1,$2,$3,$4,$5) \
                         ON CONFLICT (application_id, principal_type, principal_id, access_level) DO NOTHING",
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
        }

        #[async_trait]
        impl PlatformWriter for $name {
            async fn write(&self, repository_id: Uuid, result: &AnalysisResult) -> RepoResult<Uuid> {
                let app_id = self.upsert_application(repository_id, &result.application).await?;
                self.clear_associations(app_id).await?;
                self.write_languages(app_id, result).await?;
                self.write_libraries(app_id, result).await?;
                self.write_infrastructure(app_id, result).await?;
                self.write_dependencies(app_id, result).await?;
                self.write_access(app_id, result).await?;
                Ok(app_id)
            }
        }
    };
}

platform_writer_impl!(PgPlatformWriter, PgPool, identity);
platform_writer_impl!(SqlitePlatformWriter, SqlitePool, to_sqlite);
