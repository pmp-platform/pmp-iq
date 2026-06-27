//! Dual-engine repositories for the analysis-config tables (`entity_kinds`,
//! `entity_properties`). One macro generates the Postgres and SQLite impls.

use super::model::{DataType, EntityKind, EntityKindInput, EntityProperty, EntityPropertyInput};
use crate::db::{RepoError, RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

/// Allowed-kinds store.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EntityKindRepository: Send + Sync {
    async fn list(&self) -> RepoResult<Vec<EntityKind>>;
    async fn create(&self, input: EntityKindInput) -> RepoResult<EntityKind>;
    async fn update(&self, id: Uuid, input: EntityKindInput) -> RepoResult<EntityKind>;
    async fn delete(&self, id: Uuid) -> RepoResult<()>;
}

/// Extraction-properties store.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EntityPropertyRepository: Send + Sync {
    async fn list(&self) -> RepoResult<Vec<EntityProperty>>;
    async fn create(&self, input: EntityPropertyInput) -> RepoResult<EntityProperty>;
    async fn update(&self, id: Uuid, input: EntityPropertyInput) -> RepoResult<EntityProperty>;
    async fn delete(&self, id: Uuid) -> RepoResult<()>;
}

#[derive(FromRow)]
struct KindRow {
    id: Uuid,
    entity_type: String,
    kind_id: String,
    name: String,
    description: String,
    config: serde_json::Value,
}

impl From<KindRow> for EntityKind {
    fn from(r: KindRow) -> Self {
        EntityKind {
            id: r.id,
            entity_type: r.entity_type,
            kind_id: r.kind_id,
            name: r.name,
            description: r.description,
            config: r.config,
        }
    }
}

#[derive(FromRow)]
struct PropertyRow {
    id: Uuid,
    entity_type: String,
    prop_id: String,
    name: String,
    description: String,
    data_type: String,
}

impl TryFrom<PropertyRow> for EntityProperty {
    type Error = RepoError;
    fn try_from(r: PropertyRow) -> Result<Self, Self::Error> {
        Ok(EntityProperty {
            id: r.id,
            entity_type: r.entity_type,
            prop_id: r.prop_id,
            name: r.name,
            description: r.description,
            data_type: DataType::parse(&r.data_type).map_err(RepoError::Mapping)?,
        })
    }
}

macro_rules! entity_kind_repo_impl {
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
        impl EntityKindRepository for $name {
            async fn list(&self) -> RepoResult<Vec<EntityKind>> {
                let rows: Vec<KindRow> = sqlx::query_as(&$xform(
                    "SELECT id, entity_type, kind_id, name, description, config FROM entity_kinds \
                     ORDER BY entity_type, name",
                ))
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(EntityKind::from).collect())
            }

            async fn create(&self, input: EntityKindInput) -> RepoResult<EntityKind> {
                let id = Uuid::new_v4();
                let row: KindRow = sqlx::query_as(&$xform(
                    "INSERT INTO entity_kinds (id, entity_type, kind_id, name, description, config) \
                     VALUES ($1,$2,$3,$4,$5,$6) \
                     RETURNING id, entity_type, kind_id, name, description, config",
                ))
                .bind(id)
                .bind(&input.entity_type)
                .bind(&input.kind_id)
                .bind(&input.name)
                .bind(&input.description)
                .bind(&input.config)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.into())
            }

            async fn update(&self, id: Uuid, input: EntityKindInput) -> RepoResult<EntityKind> {
                let row: KindRow = sqlx::query_as(&$xform(
                    "UPDATE entity_kinds SET entity_type=$2, kind_id=$3, name=$4, description=$5, config=$6 \
                     WHERE id=$1 RETURNING id, entity_type, kind_id, name, description, config",
                ))
                .bind(id)
                .bind(&input.entity_type)
                .bind(&input.kind_id)
                .bind(&input.name)
                .bind(&input.description)
                .bind(&input.config)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                Ok(row.into())
            }

            async fn delete(&self, id: Uuid) -> RepoResult<()> {
                let res = sqlx::query(&$xform("DELETE FROM entity_kinds WHERE id=$1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                if res.rows_affected() == 0 {
                    return Err(RepoError::NotFound);
                }
                Ok(())
            }
        }
    };
}

macro_rules! entity_property_repo_impl {
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
        impl EntityPropertyRepository for $name {
            async fn list(&self) -> RepoResult<Vec<EntityProperty>> {
                let rows: Vec<PropertyRow> = sqlx::query_as(&$xform(
                    "SELECT id, entity_type, prop_id, name, description, data_type FROM entity_properties \
                     ORDER BY entity_type, name",
                ))
                .fetch_all(&self.pool)
                .await?;
                rows.into_iter().map(EntityProperty::try_from).collect()
            }

            async fn create(&self, input: EntityPropertyInput) -> RepoResult<EntityProperty> {
                let id = Uuid::new_v4();
                let row: PropertyRow = sqlx::query_as(&$xform(
                    "INSERT INTO entity_properties (id, entity_type, prop_id, name, description, data_type) \
                     VALUES ($1,$2,$3,$4,$5,$6) \
                     RETURNING id, entity_type, prop_id, name, description, data_type",
                ))
                .bind(id)
                .bind(&input.entity_type)
                .bind(&input.prop_id)
                .bind(&input.name)
                .bind(&input.description)
                .bind(input.data_type.as_str())
                .fetch_one(&self.pool)
                .await?;
                row.try_into()
            }

            async fn update(&self, id: Uuid, input: EntityPropertyInput) -> RepoResult<EntityProperty> {
                let row: PropertyRow = sqlx::query_as(&$xform(
                    "UPDATE entity_properties SET entity_type=$2, prop_id=$3, name=$4, description=$5, data_type=$6 \
                     WHERE id=$1 RETURNING id, entity_type, prop_id, name, description, data_type",
                ))
                .bind(id)
                .bind(&input.entity_type)
                .bind(&input.prop_id)
                .bind(&input.name)
                .bind(&input.description)
                .bind(input.data_type.as_str())
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                row.try_into()
            }

            async fn delete(&self, id: Uuid) -> RepoResult<()> {
                let res = sqlx::query(&$xform("DELETE FROM entity_properties WHERE id=$1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                if res.rows_affected() == 0 {
                    return Err(RepoError::NotFound);
                }
                Ok(())
            }
        }
    };
}

entity_kind_repo_impl!(PgEntityKindRepository, PgPool, identity);
entity_kind_repo_impl!(SqliteEntityKindRepository, SqlitePool, to_sqlite);
entity_property_repo_impl!(PgEntityPropertyRepository, PgPool, identity);
entity_property_repo_impl!(SqliteEntityPropertyRepository, SqlitePool, to_sqlite);
