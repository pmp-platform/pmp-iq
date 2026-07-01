//! Dual-engine persistence for roles, teams, membership and ownership (M37).

use super::model::{Role, Team, TeamInput};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

/// Role assignments per principal.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RoleRepository: Send + Sync {
    async fn get(&self, principal: &str) -> RepoResult<Option<Role>>;
    async fn set(&self, principal: &str, role: Role) -> RepoResult<()>;
    async fn list(&self) -> RepoResult<Vec<(String, Role)>>;
    /// Number of assigned roles (0 ⇒ first-admin bootstrap mode).
    async fn count(&self) -> RepoResult<i64>;
}

/// Teams, membership and team→application ownership.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TeamRepository: Send + Sync {
    async fn create(&self, input: TeamInput) -> RepoResult<Team>;
    async fn list(&self) -> RepoResult<Vec<Team>>;
    async fn delete(&self, id: Uuid) -> RepoResult<()>;
    async fn add_member(&self, team_id: Uuid, principal: &str) -> RepoResult<()>;
    async fn members(&self, team_id: Uuid) -> RepoResult<Vec<String>>;
    async fn set_owner(&self, team_id: Uuid, application_id: Uuid) -> RepoResult<()>;
    /// Team ids owning an application.
    async fn owner_team_ids(&self, application_id: Uuid) -> RepoResult<Vec<Uuid>>;
    /// Team ids a principal belongs to.
    async fn principal_team_ids(&self, principal: &str) -> RepoResult<Vec<Uuid>>;
    /// Distinct tenant ids of the teams a principal belongs to.
    async fn tenants_for_principal(&self, principal: &str) -> RepoResult<Vec<String>>;
    /// Application ids owned by any team in the given tenant.
    async fn app_ids_for_tenant(&self, tenant: &str) -> RepoResult<Vec<Uuid>>;
}

macro_rules! role_impl {
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
        impl RoleRepository for $name {
            async fn get(&self, principal: &str) -> RepoResult<Option<Role>> {
                let row: Option<(String,)> = sqlx::query_as(&$xform("SELECT role FROM roles WHERE principal=$1"))
                    .bind(principal)
                    .fetch_optional(&self.pool)
                    .await?;
                Ok(row.map(|(r,)| Role::parse(&r)))
            }

            async fn set(&self, principal: &str, role: Role) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO roles (principal, role) VALUES ($1,$2) \
                     ON CONFLICT (principal) DO UPDATE SET role=excluded.role",
                ))
                .bind(principal)
                .bind(role.as_str())
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn list(&self) -> RepoResult<Vec<(String, Role)>> {
                let rows: Vec<(String, String)> = sqlx::query_as("SELECT principal, role FROM roles ORDER BY principal")
                    .fetch_all(&self.pool)
                    .await?;
                Ok(rows.into_iter().map(|(p, r)| (p, Role::parse(&r))).collect())
            }

            async fn count(&self) -> RepoResult<i64> {
                let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM roles").fetch_one(&self.pool).await?;
                Ok(n)
            }
        }
    };
}

role_impl!(PgRoleRepository, PgPool, identity);
role_impl!(SqliteRoleRepository, SqlitePool, to_sqlite);

macro_rules! team_impl {
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
        impl TeamRepository for $name {
            async fn create(&self, input: TeamInput) -> RepoResult<Team> {
                let id = Uuid::new_v4();
                sqlx::query(&$xform("INSERT INTO teams (id, name, tenant_id) VALUES ($1,$2,$3)"))
                    .bind(id)
                    .bind(&input.name)
                    .bind(&input.tenant_id)
                    .execute(&self.pool)
                    .await?;
                Ok(Team { id, name: input.name, tenant_id: input.tenant_id })
            }

            async fn list(&self) -> RepoResult<Vec<Team>> {
                Ok(sqlx::query_as("SELECT id, name, tenant_id FROM teams ORDER BY name")
                    .fetch_all(&self.pool)
                    .await?)
            }

            async fn delete(&self, id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform("DELETE FROM teams WHERE id=$1")).bind(id).execute(&self.pool).await?;
                Ok(())
            }

            async fn add_member(&self, team_id: Uuid, principal: &str) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO team_members (team_id, principal) VALUES ($1,$2) ON CONFLICT DO NOTHING",
                ))
                .bind(team_id)
                .bind(principal)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn members(&self, team_id: Uuid) -> RepoResult<Vec<String>> {
                let rows: Vec<(String,)> =
                    sqlx::query_as(&$xform("SELECT principal FROM team_members WHERE team_id=$1 ORDER BY principal"))
                        .bind(team_id)
                        .fetch_all(&self.pool)
                        .await?;
                Ok(rows.into_iter().map(|(p,)| p).collect())
            }

            async fn set_owner(&self, team_id: Uuid, application_id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO team_applications (team_id, application_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
                ))
                .bind(team_id)
                .bind(application_id)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn owner_team_ids(&self, application_id: Uuid) -> RepoResult<Vec<Uuid>> {
                let rows: Vec<(Uuid,)> = sqlx::query_as(&$xform(
                    "SELECT team_id FROM team_applications WHERE application_id=$1",
                ))
                .bind(application_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(id,)| id).collect())
            }

            async fn principal_team_ids(&self, principal: &str) -> RepoResult<Vec<Uuid>> {
                let rows: Vec<(Uuid,)> = sqlx::query_as(&$xform(
                    "SELECT team_id FROM team_members WHERE principal=$1",
                ))
                .bind(principal)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(id,)| id).collect())
            }

            async fn tenants_for_principal(&self, principal: &str) -> RepoResult<Vec<String>> {
                let rows: Vec<(String,)> = sqlx::query_as(&$xform(
                    "SELECT DISTINCT t.tenant_id FROM teams t JOIN team_members m ON m.team_id=t.id \
                     WHERE m.principal=$1 AND t.tenant_id IS NOT NULL",
                ))
                .bind(principal)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(t,)| t).collect())
            }

            async fn app_ids_for_tenant(&self, tenant: &str) -> RepoResult<Vec<Uuid>> {
                let rows: Vec<(Uuid,)> = sqlx::query_as(&$xform(
                    "SELECT DISTINCT ta.application_id FROM team_applications ta \
                     JOIN teams t ON t.id=ta.team_id WHERE t.tenant_id=$1",
                ))
                .bind(tenant)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(id,)| id).collect())
            }
        }
    };
}

team_impl!(PgTeamRepository, PgPool, identity);
team_impl!(SqliteTeamRepository, SqlitePool, to_sqlite);
