//! Read layer for the API surface (M42): an application's exposed endpoints and,
//! per endpoint, the applications that consume it (impact). One macro body backs
//! both engines.

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

/// One exposed API operation, with the repository files implementing it.
#[derive(Debug, Clone, Serialize)]
pub struct Endpoint {
    pub id: Uuid,
    pub protocol: String,
    pub operation: String,
    pub summary: Option<String>,
    pub files: Vec<String>,
}

/// An application that consumes an endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct Consumer {
    pub application_id: Uuid,
    pub name: String,
}

/// Read an application's endpoints and an endpoint's consumers.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ApiEndpointRepository: Send + Sync {
    async fn for_application(&self, application_id: Uuid) -> RepoResult<Vec<Endpoint>>;
    /// The applications whose dependencies resolve to `endpoint_id` (impact).
    async fn consumers(&self, endpoint_id: Uuid) -> RepoResult<Vec<Consumer>>;
}

macro_rules! endpoint_impl {
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
        impl ApiEndpointRepository for $name {
            async fn for_application(&self, application_id: Uuid) -> RepoResult<Vec<Endpoint>> {
                let rows: Vec<(Uuid, String, String, Option<String>)> = sqlx::query_as(&$xform(
                    "SELECT id, protocol, operation, summary FROM api_endpoints \
                     WHERE application_id=$1 ORDER BY protocol, operation",
                ))
                .bind(application_id)
                .fetch_all(&self.pool)
                .await?;
                let mut out = Vec::with_capacity(rows.len());
                for (id, protocol, operation, summary) in rows {
                    let files: Vec<(String,)> = sqlx::query_as(&$xform(
                        "SELECT path FROM endpoint_files WHERE endpoint_id=$1 ORDER BY path",
                    ))
                    .bind(id)
                    .fetch_all(&self.pool)
                    .await?;
                    out.push(Endpoint {
                        id,
                        protocol,
                        operation,
                        summary,
                        files: files.into_iter().map(|(p,)| p).collect(),
                    });
                }
                Ok(out)
            }

            async fn consumers(&self, endpoint_id: Uuid) -> RepoResult<Vec<Consumer>> {
                let rows: Vec<(Uuid, String)> = sqlx::query_as(&$xform(
                    "SELECT DISTINCT a.id, a.name FROM application_dependencies d \
                     JOIN applications a ON a.id=d.source_app_id WHERE d.endpoint_id=$1 ORDER BY a.name",
                ))
                .bind(endpoint_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(application_id, name)| Consumer { application_id, name }).collect())
            }
        }
    };
}

endpoint_impl!(PgApiEndpointRepository, PgPool, identity);
endpoint_impl!(SqliteApiEndpointRepository, SqlitePool, to_sqlite);
