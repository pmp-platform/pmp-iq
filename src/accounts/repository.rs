//! Data access for repository accounts.

use super::model::{
    AccountInput, AuthType, ProviderType, RepositoryAccount, SelectionMode,
};
use crate::db::{RepoError, RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

/// CRUD access to configured repository accounts.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RepositoryAccountRepository: Send + Sync {
    async fn create(&self, input: AccountInput) -> RepoResult<RepositoryAccount>;
    async fn update(&self, id: Uuid, input: AccountInput) -> RepoResult<RepositoryAccount>;
    async fn delete(&self, id: Uuid) -> RepoResult<()>;
    async fn get(&self, id: Uuid) -> RepoResult<RepositoryAccount>;
    async fn list(&self) -> RepoResult<Vec<RepositoryAccount>>;
    async fn list_enabled(&self) -> RepoResult<Vec<RepositoryAccount>>;
}

#[derive(FromRow)]
struct AccountRow {
    id: Uuid,
    name: String,
    provider_type: String,
    auth_type: String,
    base_url: Option<String>,
    organization: Option<String>,
    credentials_enc: Option<Vec<u8>>,
    selection_mode: String,
    selection_value: Option<String>,
    enabled: bool,
}

impl TryFrom<AccountRow> for RepositoryAccount {
    type Error = RepoError;

    fn try_from(row: AccountRow) -> Result<Self, Self::Error> {
        let map = |e: super::model::ModelError| RepoError::Mapping(e.to_string());
        Ok(RepositoryAccount {
            id: row.id,
            name: row.name,
            provider_type: ProviderType::parse(&row.provider_type).map_err(map)?,
            auth_type: AuthType::parse(&row.auth_type).map_err(map)?,
            base_url: row.base_url,
            organization: row.organization,
            credentials_enc: row.credentials_enc,
            selection_mode: SelectionMode::parse(&row.selection_mode).map_err(map)?,
            selection_value: row.selection_value,
            enabled: row.enabled,
        })
    }
}

const COLS: &str = "id, name, provider_type, auth_type, base_url, organization, credentials_enc, \
     selection_mode, selection_value, enabled";

macro_rules! account_repo_impl {
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
        impl RepositoryAccountRepository for $name {
            async fn create(&self, input: AccountInput) -> RepoResult<RepositoryAccount> {
                let id = Uuid::new_v4();
                let row: AccountRow = sqlx::query_as(&$xform(
                    "INSERT INTO repository_accounts \
                     (id, name, provider_type, auth_type, base_url, organization, credentials_enc, \
                      selection_mode, selection_value, enabled) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
                     RETURNING id, name, provider_type, auth_type, base_url, organization, \
                               credentials_enc, selection_mode, selection_value, enabled",
                ))
                .bind(id)
                .bind(&input.name)
                .bind(input.provider_type.as_str())
                .bind(input.auth_type.as_str())
                .bind(&input.base_url)
                .bind(&input.organization)
                .bind(&input.credentials_enc)
                .bind(input.selection_mode.as_str())
                .bind(&input.selection_value)
                .bind(input.enabled)
                .fetch_one(&self.pool)
                .await?;
                row.try_into()
            }

            async fn update(&self, id: Uuid, input: AccountInput) -> RepoResult<RepositoryAccount> {
                let row: AccountRow = sqlx::query_as(&$xform(
                    "UPDATE repository_accounts SET name=$2, provider_type=$3, auth_type=$4, \
                     base_url=$5, organization=$6, credentials_enc=$7, selection_mode=$8, \
                     selection_value=$9, enabled=$10, updated_at=CURRENT_TIMESTAMP WHERE id=$1 \
                     RETURNING id, name, provider_type, auth_type, base_url, organization, \
                               credentials_enc, selection_mode, selection_value, enabled",
                ))
                .bind(id)
                .bind(&input.name)
                .bind(input.provider_type.as_str())
                .bind(input.auth_type.as_str())
                .bind(&input.base_url)
                .bind(&input.organization)
                .bind(&input.credentials_enc)
                .bind(input.selection_mode.as_str())
                .bind(&input.selection_value)
                .bind(input.enabled)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                row.try_into()
            }

            async fn delete(&self, id: Uuid) -> RepoResult<()> {
                let res = sqlx::query(&$xform("DELETE FROM repository_accounts WHERE id=$1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                if res.rows_affected() == 0 {
                    return Err(RepoError::NotFound);
                }
                Ok(())
            }

            async fn get(&self, id: Uuid) -> RepoResult<RepositoryAccount> {
                let row: AccountRow = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM repository_accounts WHERE id=$1"
                )))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                row.try_into()
            }

            async fn list(&self) -> RepoResult<Vec<RepositoryAccount>> {
                let rows: Vec<AccountRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM repository_accounts ORDER BY name"
                )))
                .fetch_all(&self.pool)
                .await?;
                rows.into_iter().map(RepositoryAccount::try_from).collect()
            }

            async fn list_enabled(&self) -> RepoResult<Vec<RepositoryAccount>> {
                let rows: Vec<AccountRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM repository_accounts WHERE enabled ORDER BY name"
                )))
                .fetch_all(&self.pool)
                .await?;
                rows.into_iter().map(RepositoryAccount::try_from).collect()
            }
        }
    };
}

account_repo_impl!(PgRepositoryAccountRepository, PgPool, identity);
account_repo_impl!(SqliteRepositoryAccountRepository, SqlitePool, to_sqlite);
