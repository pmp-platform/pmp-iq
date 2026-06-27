//! Registry of "linked entities": platform entities that share one shape — a
//! `(name, kind, version, metadata)` row joined to applications with a `usage`.
//! Infrastructure, Tools and the dependency-type entities (cloud providers,
//! services, platforms, external) are all instances. The writer, queries and
//! graph are table-driven from this registry so the pattern lives once.

/// Describes one linked entity: its public (route) name and the tables backing
/// it. The string fields are internal constants — safe to interpolate into SQL.
pub struct LinkedEntity {
    /// Route/entity name used in URLs and the analysis schema array.
    pub name: &'static str,
    /// Entity table holding the `(name, kind, version, metadata)` rows.
    pub table: &'static str,
    /// Join table linking the entity to applications.
    pub join_table: &'static str,
    /// Foreign-key column in the join table referencing the entity.
    pub fk_col: &'static str,
}

/// Every linked entity, in tab/display order.
pub const LINKED: &[LinkedEntity] = &[
    LinkedEntity {
        name: "infrastructure",
        table: "infrastructure",
        join_table: "application_infrastructure",
        fk_col: "infrastructure_id",
    },
    LinkedEntity {
        name: "tools",
        table: "tools",
        join_table: "application_tools",
        fk_col: "tool_id",
    },
    LinkedEntity {
        name: "cloud-providers",
        table: "cloud_providers",
        join_table: "application_cloud_providers",
        fk_col: "cloud_provider_id",
    },
    LinkedEntity {
        name: "services",
        table: "services",
        join_table: "application_services",
        fk_col: "service_id",
    },
    LinkedEntity {
        name: "platforms",
        table: "platforms",
        join_table: "application_platforms",
        fk_col: "platform_id",
    },
    LinkedEntity {
        name: "external",
        table: "external_deps",
        join_table: "application_external_deps",
        fk_col: "external_dep_id",
    },
];

/// Look up a linked entity by its route name.
pub fn linked(name: &str) -> Option<&'static LinkedEntity> {
    LINKED.iter().find(|e| e.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_known_entities() {
        assert_eq!(linked("tools").unwrap().table, "tools");
        assert_eq!(linked("cloud-providers").unwrap().fk_col, "cloud_provider_id");
        assert!(linked("widgets").is_none());
    }

    #[test]
    fn infrastructure_is_registered() {
        assert!(linked("infrastructure").is_some());
    }
}
