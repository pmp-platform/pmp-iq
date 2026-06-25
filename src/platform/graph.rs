//! Builds a connection graph (nodes + edges) from the platform model. The SQL
//! is portable and JSON is assembled in Rust, so one body backs both engines.

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{PgPool, SqlitePool};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 300;

/// Scope/filters for a graph query (bundled to bound parameters).
#[derive(Debug, Clone)]
pub struct GraphScope {
    /// Centre the graph on one application id (only it + direct neighbours).
    pub center: Option<Uuid>,
    /// Maximum number of application nodes to include.
    pub limit: i64,
}

impl GraphScope {
    pub fn new(center: Option<Uuid>, limit: Option<i64>) -> Self {
        Self {
            center,
            limit: limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 2000),
        }
    }
}

/// Builds platform connection graphs.
#[async_trait]
pub trait GraphQuery: Send + Sync {
    async fn build(&self, scope: &GraphScope) -> RepoResult<Value>;
}

/// Internal accumulator while assembling the graph.
#[derive(Default)]
struct GraphBuilder {
    nodes: Vec<Value>,
    edges: Vec<Value>,
    node_ids: HashSet<String>,
    app_names: HashMap<String, Uuid>,
}

impl GraphBuilder {
    fn add_node(&mut self, id: String, label: String, kind: &str, sub: Option<&str>) {
        if self.node_ids.insert(id.clone()) {
            self.nodes.push(json!({
                "data": { "id": id, "label": label, "kind": kind, "sub": sub }
            }));
        }
    }

    fn add_edge(&mut self, source: String, target: String, kind: &str) {
        let id = format!("{source}->{target}:{kind}");
        self.edges.push(json!({
            "data": { "id": id, "source": source, "target": target, "kind": kind }
        }));
    }
}

/// Restrict to the centre node and its direct neighbours, if a centre is set.
fn focus(builder: GraphBuilder, center: Option<Uuid>) -> (Vec<Value>, Vec<Value>) {
    let Some(center) = center else {
        return (builder.nodes, builder.edges);
    };
    let center = center.to_string();
    let mut keep: HashSet<String> = HashSet::new();
    keep.insert(center.clone());
    let mut kept_edges = Vec::new();
    for edge in &builder.edges {
        let source = edge["data"]["source"].as_str().unwrap_or_default().to_string();
        let target = edge["data"]["target"].as_str().unwrap_or_default().to_string();
        if source == center || target == center {
            keep.insert(source);
            keep.insert(target);
            kept_edges.push(edge.clone());
        }
    }
    let nodes = builder
        .nodes
        .into_iter()
        .filter(|n| keep.contains(n["data"]["id"].as_str().unwrap_or_default()))
        .collect();
    (nodes, kept_edges)
}

macro_rules! graph_query_impl {
    ($name:ident, $pool:ty, $xform:path) => {
        pub struct $name {
            pool: $pool,
        }
        impl $name {
            pub fn new(pool: $pool) -> Self {
                Self { pool }
            }

            async fn add_applications(&self, builder: &mut GraphBuilder, limit: i64) -> RepoResult<()> {
                let apps: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(&$xform(
                    "SELECT id, name, app_type FROM applications ORDER BY name LIMIT $1",
                ))
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
                for (id, name, app_type) in apps {
                    builder.app_names.insert(name.clone(), id);
                    builder.add_node(id.to_string(), name, "application", app_type.as_deref());
                }
                Ok(())
            }

            async fn add_infrastructure_edges(&self, builder: &mut GraphBuilder) -> RepoResult<()> {
                let rows: Vec<(Uuid, String, String, Uuid)> = sqlx::query_as(&$xform(
                    "SELECT i.id, i.name, i.kind, ai.application_id FROM application_infrastructure ai \
                     JOIN infrastructure i ON i.id = ai.infrastructure_id",
                ))
                .fetch_all(&self.pool)
                .await?;
                for (infra_id, name, kind, app_id) in rows {
                    let app_node = app_id.to_string();
                    if !builder.node_ids.contains(&app_node) {
                        continue;
                    }
                    let infra_node = format!("infra:{infra_id}");
                    builder.add_node(infra_node.clone(), name, "infrastructure", Some(&kind));
                    builder.add_edge(app_node, infra_node, &kind);
                }
                Ok(())
            }

            async fn add_dependency_edges(&self, builder: &mut GraphBuilder) -> RepoResult<()> {
                let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(&$xform(
                    "SELECT source_app_id, target_name, kind FROM application_dependencies",
                ))
                .fetch_all(&self.pool)
                .await?;
                for (source, target_name, kind) in rows {
                    let source_node = source.to_string();
                    if !builder.node_ids.contains(&source_node) {
                        continue;
                    }
                    let kind = kind.unwrap_or_else(|| "depends".into());
                    let target_node = match builder.app_names.get(&target_name) {
                        Some(id) => id.to_string(),
                        None => {
                            let ext = format!("ext:{target_name}");
                            builder.add_node(ext.clone(), target_name.clone(), "external", None);
                            ext
                        }
                    };
                    builder.add_edge(source_node, target_node, &kind);
                }
                Ok(())
            }
        }

        #[async_trait]
        impl GraphQuery for $name {
            async fn build(&self, scope: &GraphScope) -> RepoResult<Value> {
                let mut builder = GraphBuilder::default();
                let (total_apps,): (i64,) =
                    sqlx::query_as(&$xform("SELECT COUNT(*) FROM applications"))
                        .fetch_one(&self.pool)
                        .await?;
                self.add_applications(&mut builder, scope.limit).await?;
                self.add_infrastructure_edges(&mut builder).await?;
                self.add_dependency_edges(&mut builder).await?;

                let truncated = total_apps > scope.limit;
                let (nodes, edges) = focus(builder, scope.center);
                Ok(json!({
                    "nodes": nodes,
                    "edges": edges,
                    "truncated": truncated,
                    "total_applications": total_apps,
                }))
            }
        }
    };
}

graph_query_impl!(PgGraphQuery, PgPool, identity);
graph_query_impl!(SqliteGraphQuery, SqlitePool, to_sqlite);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_clamps_limit() {
        assert_eq!(GraphScope::new(None, Some(99999)).limit, 2000);
        assert_eq!(GraphScope::new(None, None).limit, DEFAULT_LIMIT);
    }

    #[test]
    fn focus_keeps_only_center_neighbourhood() {
        let center = Uuid::new_v4();
        let other = Uuid::new_v4();
        let far = Uuid::new_v4();
        let mut builder = GraphBuilder::default();
        builder.add_node(center.to_string(), "c".into(), "application", None);
        builder.add_node(other.to_string(), "o".into(), "application", None);
        builder.add_node(far.to_string(), "f".into(), "application", None);
        builder.add_edge(center.to_string(), other.to_string(), "http");
        builder.add_edge(far.to_string(), far.to_string(), "http");

        let (nodes, edges) = focus(builder, Some(center));
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
    }
}
