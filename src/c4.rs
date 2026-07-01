//! C4 model projection & export (M29). Projects the platform connection graph
//! (applications = software systems; infrastructure/services/external/etc. =
//! external systems; edges = relationships) into a System-Context view, exported
//! as Structurizr DSL and C4 Mermaid. The model is derived from code, so the
//! views are always current.

use serde_json::Value;
use std::collections::HashSet;

/// A C4 element projected from a graph node.
struct Element {
    id: String,
    label: String,
    /// True for the platform's own applications (vs. external systems).
    internal: bool,
}

struct Relationship {
    source: String,
    target: String,
    description: String,
}

/// Turn a graph node/edge id into a valid identifier (alnum + `_`).
fn ident(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("n_{s}")
    } else {
        s
    }
}

fn escape(label: &str) -> String {
    label.replace('"', "'")
}

/// Project a graph (`{ nodes, edges }`) into C4 elements + relationships. When
/// `include_dependencies` is false, only the platform's own applications are
/// kept (plus the relationships strictly between them) — the default view.
fn project(graph: &Value, include_dependencies: bool) -> (Vec<Element>, Vec<Relationship>) {
    let nodes = graph.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
    let edges = graph.get("edges").and_then(Value::as_array).cloned().unwrap_or_default();
    // Graph nodes/edges carry their fields under `data` (see GraphBuilder).
    let mut elements: Vec<Element> = nodes
        .iter()
        .filter_map(|n| {
            let data = n.get("data")?;
            let id = data.get("id").and_then(Value::as_str)?;
            let label = data.get("label").and_then(Value::as_str).unwrap_or(id);
            let kind = data.get("kind").and_then(Value::as_str).unwrap_or("");
            Some(Element {
                id: ident(id),
                label: escape(label),
                internal: kind == "application",
            })
        })
        .collect();
    let mut relationships: Vec<Relationship> = edges
        .iter()
        .filter_map(|e| {
            let data = e.get("data")?;
            let source = data.get("source").and_then(Value::as_str)?;
            let target = data.get("target").and_then(Value::as_str)?;
            let description = data
                .get("kind")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("uses");
            Some(Relationship {
                source: ident(source),
                target: ident(target),
                description: escape(description),
            })
        })
        .collect();
    if !include_dependencies {
        elements.retain(|e| e.internal);
        let kept: HashSet<&str> = elements.iter().map(|e| e.id.as_str()).collect();
        relationships.retain(|r| kept.contains(r.source.as_str()) && kept.contains(r.target.as_str()));
    }
    (elements, relationships)
}

/// Export the graph as a Structurizr DSL workspace (model + landscape view).
pub fn structurizr_dsl(graph: &Value, include_dependencies: bool) -> String {
    let (elements, relationships) = project(graph, include_dependencies);
    let mut s = String::from("workspace \"pmp-iq\" {\n  model {\n");
    for e in &elements {
        let tag = if e.internal { "" } else { " \"External\"" };
        s.push_str(&format!("    {} = softwareSystem \"{}\"{}\n", e.id, e.label, tag));
    }
    for r in &relationships {
        s.push_str(&format!("    {} -> {} \"{}\"\n", r.source, r.target, r.description));
    }
    s.push_str("  }\n  views {\n    systemLandscape \"landscape\" {\n      include *\n      autolayout lr\n    }\n  }\n}\n");
    s
}

/// Export the graph as a C4 Mermaid System-Context diagram.
pub fn mermaid_context(graph: &Value, include_dependencies: bool) -> String {
    let (elements, relationships) = project(graph, include_dependencies);
    let mut s = String::from("C4Context\n  title Platform system context\n");
    for e in &elements {
        let macro_name = if e.internal { "System" } else { "System_Ext" };
        s.push_str(&format!("  {}({}, \"{}\")\n", macro_name, e.id, e.label));
    }
    for r in &relationships {
        s.push_str(&format!("  Rel({}, {}, \"{}\")\n", r.source, r.target, r.description));
    }
    s
}

// ---------------------------------------------------------------------------
// M38 — Container & Component levels.
// ---------------------------------------------------------------------------

fn nodes_of(graph: &Value) -> Vec<Value> {
    graph.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn edges_of(graph: &Value) -> Vec<Value> {
    graph.get("edges").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// Storage-ish keywords marking a container as a datastore (→ `ContainerDb`).
const DATASTORE_HINTS: &[&str] = &[
    "db", "database", "sql", "postgres", "mysql", "maria", "sqlite", "mongo",
    "redis", "cache", "cassandra", "dynamo", "queue", "kafka", "rabbit",
    "store", "bucket", "elastic", "memcache", "datastore", "warehouse",
];

fn is_datastore(kind: &str, sub: &str) -> bool {
    let hay = format!("{kind} {sub}").to_ascii_lowercase();
    DATASTORE_HINTS.iter().any(|h| hay.contains(h))
}

/// A container projected for the C4 Container view.
struct Container {
    id: String,
    label: String,
    /// Datastore → rendered with `ContainerDb` / a `Database` tag.
    db: bool,
    /// Outside the application boundary (a dependency target).
    external: bool,
}

/// Projection of a focused application graph into a Container view.
struct ContainerProjection {
    app_label: String,
    containers: Vec<Container>,
    relationships: Vec<Relationship>,
}

/// Project a focused application graph (centre = the application) into its
/// containers (infra/services it owns) plus external systems (dependency
/// targets, only when `include_dependencies`). The relationships originate from
/// the application's runtime container (`<app>_app`).
fn project_containers(graph: &Value, app_id: &str, include_dependencies: bool) -> ContainerProjection {
    let app_ident = ident(app_id);
    let mut app_label = app_id.to_string();
    let mut containers = Vec::new();
    for n in nodes_of(graph) {
        let Some(data) = n.get("data") else { continue };
        let Some(id) = data.get("id").and_then(Value::as_str) else { continue };
        let label = data.get("label").and_then(Value::as_str).unwrap_or(id);
        if id == app_id {
            app_label = escape(label);
            continue;
        }
        let kind = data.get("kind").and_then(Value::as_str).unwrap_or("");
        let sub = data.get("sub").and_then(Value::as_str).unwrap_or("");
        let external = kind == "application" || kind == "external";
        if external && !include_dependencies {
            continue;
        }
        containers.push(Container {
            id: ident(id),
            label: escape(label),
            db: !external && is_datastore(kind, sub),
            external,
        });
    }
    let kept: HashSet<&str> = containers.iter().map(|c| c.id.as_str()).collect();
    let relationships = container_rels(graph, &app_ident, &kept);
    ContainerProjection { app_label, containers, relationships }
}

/// Edges from the application node to a kept container/external, re-sourced to
/// the application's runtime container id.
fn container_rels(graph: &Value, app_ident: &str, kept: &HashSet<&str>) -> Vec<Relationship> {
    let runtime = format!("{app_ident}_app");
    let mut rels = Vec::new();
    for e in edges_of(graph) {
        let Some(data) = e.get("data") else { continue };
        let (Some(source), Some(target)) = (
            data.get("source").and_then(Value::as_str),
            data.get("target").and_then(Value::as_str),
        ) else {
            continue;
        };
        let target = ident(target);
        if ident(source) == app_ident && kept.contains(target.as_str()) {
            let kind = data.get("kind").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("uses");
            rels.push(Relationship { source: runtime.clone(), target, description: escape(kind) });
        }
    }
    rels
}

/// Export a focused application graph as a Structurizr Container view.
pub fn container_dsl(graph: &Value, app_id: &str, include_dependencies: bool) -> String {
    let p = project_containers(graph, app_id, include_dependencies);
    let app = ident(app_id);
    let mut s = String::from("workspace \"pmp-iq\" {\n  model {\n");
    s.push_str(&format!("    {app} = softwareSystem \"{}\" {{\n", p.app_label));
    s.push_str(&format!("      {app}_app = container \"{}\" \"application\"\n", p.app_label));
    for c in p.containers.iter().filter(|c| !c.external) {
        let tag = if c.db { " \"Database\"" } else { "" };
        s.push_str(&format!("      {} = container \"{}\"{}\n", c.id, c.label, tag));
    }
    s.push_str("    }\n");
    for c in p.containers.iter().filter(|c| c.external) {
        s.push_str(&format!("    {} = softwareSystem \"{}\" \"External\"\n", c.id, c.label));
    }
    for r in &p.relationships {
        s.push_str(&format!("    {} -> {} \"{}\"\n", r.source, r.target, r.description));
    }
    s.push_str(&format!(
        "  }}\n  views {{\n    container {app} \"containers\" {{\n      include *\n      autolayout lr\n    }}\n  }}\n}}\n"
    ));
    s
}

/// Export a focused application graph as a C4 Mermaid Container diagram.
pub fn container_mermaid(graph: &Value, app_id: &str, include_dependencies: bool) -> String {
    let p = project_containers(graph, app_id, include_dependencies);
    let app = ident(app_id);
    let mut s = String::from("C4Container\n  title Container view\n");
    s.push_str(&format!("  System_Boundary(b_{app}, \"{}\") {{\n", p.app_label));
    s.push_str(&format!("    Container({app}_app, \"{}\", \"application\")\n", p.app_label));
    for c in p.containers.iter().filter(|c| !c.external) {
        let macro_name = if c.db { "ContainerDb" } else { "Container" };
        s.push_str(&format!("    {macro_name}({}, \"{}\", \"\")\n", c.id, c.label));
    }
    s.push_str("  }\n");
    for c in p.containers.iter().filter(|c| c.external) {
        s.push_str(&format!("  System_Ext({}, \"{}\")\n", c.id, c.label));
    }
    for r in &p.relationships {
        s.push_str(&format!("  Rel({}, {}, \"{}\")\n", r.source, r.target, r.description));
    }
    s
}

/// A component projected for the C4 Component view.
struct Comp {
    id: String,
    label: String,
}

/// Projection of an application detail into a Component view.
struct ComponentProjection {
    app_label: String,
    components: Vec<Comp>,
    externals: Vec<(String, String)>,
    relationships: Vec<Relationship>,
}

/// Inter-component edges: components co-participating in a use case are linked
/// in their (name-sorted) order, labelled with the use-case name.
fn use_case_edges(detail: &Value) -> Vec<Relationship> {
    let mut rels = Vec::new();
    for uc in detail.get("use_cases").and_then(Value::as_array).into_iter().flatten() {
        let name = uc.get("name").and_then(Value::as_str).unwrap_or("uses");
        let members: Vec<String> = uc
            .get("components")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| m.get("id").and_then(Value::as_str).map(ident))
            .collect();
        for w in members.windows(2) {
            rels.push(Relationship { source: w[0].clone(), target: w[1].clone(), description: escape(name) });
        }
    }
    rels
}

/// Component → external-system edges, from dependencies attributed to a
/// component (`component_id`). Returns the external system nodes; appends edges.
fn dependency_edges(detail: &Value, comp_ids: &HashSet<String>, rels: &mut Vec<Relationship>) -> Vec<(String, String)> {
    let mut externals = Vec::new();
    let mut seen = HashSet::new();
    for d in detail.get("dependencies").and_then(Value::as_array).into_iter().flatten() {
        let Some(cid) = d.get("component_id").and_then(Value::as_str) else { continue };
        let comp = ident(cid);
        if !comp_ids.contains(&comp) {
            continue;
        }
        let target = d.get("target_name").and_then(Value::as_str).unwrap_or("");
        if target.is_empty() {
            continue;
        }
        let ext = ident(&format!("ext_{target}"));
        if seen.insert(ext.clone()) {
            externals.push((ext.clone(), escape(target)));
        }
        let kind = d.get("kind").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("uses");
        rels.push(Relationship { source: comp, target: ext, description: escape(kind) });
    }
    externals
}

fn project_components(detail: &Value) -> ComponentProjection {
    let app_label = escape(detail.get("name").and_then(Value::as_str).unwrap_or("application"));
    let components: Vec<Comp> = detail
        .get("components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c.get("id").and_then(Value::as_str)?;
            let name = c.get("name").and_then(Value::as_str).unwrap_or(id);
            Some(Comp { id: ident(id), label: escape(name) })
        })
        .collect();
    let ids: HashSet<String> = components.iter().map(|c| c.id.clone()).collect();
    let mut relationships = use_case_edges(detail);
    let externals = dependency_edges(detail, &ids, &mut relationships);
    ComponentProjection { app_label, components, externals, relationships }
}

/// Export an application's components as a Structurizr Component view.
pub fn component_dsl(detail: &Value) -> String {
    let p = project_components(detail);
    let app = ident(detail.get("id").and_then(Value::as_str).unwrap_or("app"));
    let mut s = String::from("workspace \"pmp-iq\" {\n  model {\n");
    s.push_str(&format!("    {app} = softwareSystem \"{}\" {{\n", p.app_label));
    s.push_str(&format!("      {app}_c = container \"{}\" {{\n", p.app_label));
    for c in &p.components {
        s.push_str(&format!("        {} = component \"{}\"\n", c.id, c.label));
    }
    s.push_str("      }\n    }\n");
    for (id, label) in &p.externals {
        s.push_str(&format!("    {} = softwareSystem \"{}\" \"External\"\n", id, label));
    }
    for r in &p.relationships {
        s.push_str(&format!("    {} -> {} \"{}\"\n", r.source, r.target, r.description));
    }
    s.push_str(&format!(
        "  }}\n  views {{\n    component {app}_c \"components\" {{\n      include *\n      autolayout lr\n    }}\n  }}\n}}\n"
    ));
    s
}

/// Export an application's components as a C4 Mermaid Component diagram.
pub fn component_mermaid(detail: &Value) -> String {
    let p = project_components(detail);
    let app = ident(detail.get("id").and_then(Value::as_str).unwrap_or("app"));
    let mut s = String::from("C4Component\n  title Component view\n");
    s.push_str(&format!("  Container_Boundary(b_{app}, \"{}\") {{\n", p.app_label));
    for c in &p.components {
        s.push_str(&format!("    Component({}, \"{}\", \"\")\n", c.id, c.label));
    }
    s.push_str("  }\n");
    for (id, label) in &p.externals {
        s.push_str(&format!("  System_Ext({}, \"{}\")\n", id, label));
    }
    for r in &p.relationships {
        s.push_str(&format!("  Rel({}, {}, \"{}\")\n", r.source, r.target, r.description));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "nodes": [
                { "data": { "id": "app:1", "label": "api", "kind": "application" } },
                { "data": { "id": "infra:2", "label": "Postgres", "kind": "infrastructure" } }
            ],
            "edges": [ { "data": { "source": "app:1", "target": "infra:2", "kind": "stores data" } } ]
        })
    }

    /// Two applications (with an app→app edge) plus one external dependency.
    fn sample_with_apps() -> Value {
        json!({
            "nodes": [
                { "data": { "id": "app:1", "label": "api", "kind": "application" } },
                { "data": { "id": "app:2", "label": "web", "kind": "application" } },
                { "data": { "id": "infra:3", "label": "Postgres", "kind": "infrastructure" } }
            ],
            "edges": [
                { "data": { "source": "app:2", "target": "app:1", "kind": "http" } },
                { "data": { "source": "app:1", "target": "infra:3", "kind": "stores data" } }
            ]
        })
    }

    #[test]
    fn ident_sanitises() {
        assert_eq!(ident("app:1-2"), "app_1_2");
        assert_eq!(ident("9x"), "n_9x");
    }

    #[test]
    fn structurizr_dsl_has_systems_and_relationships() {
        let dsl = structurizr_dsl(&sample(), true);
        assert!(dsl.contains("workspace"));
        assert!(dsl.contains("app_1 = softwareSystem \"api\""));
        assert!(dsl.contains("infra_2 = softwareSystem \"Postgres\" \"External\""));
        assert!(dsl.contains("app_1 -> infra_2 \"stores data\""));
        assert!(dsl.contains("systemLandscape"));
    }

    #[test]
    fn mermaid_context_uses_c4_macros() {
        let m = mermaid_context(&sample(), true);
        assert!(m.starts_with("C4Context"));
        assert!(m.contains("System(app_1, \"api\")"));
        assert!(m.contains("System_Ext(infra_2, \"Postgres\")"));
        assert!(m.contains("Rel(app_1, infra_2, \"stores data\")"));
    }

    #[test]
    fn apps_only_drops_dependencies_but_keeps_app_to_app() {
        let m = mermaid_context(&sample_with_apps(), false);
        // Both applications and the app→app relationship survive.
        assert!(m.contains("System(app_1, \"api\")"));
        assert!(m.contains("System(app_2, \"web\")"));
        assert!(m.contains("Rel(app_2, app_1, \"http\")"));
        // The external system and the app→external relationship are dropped.
        assert!(!m.contains("infra_3"));
        assert!(!m.contains("System_Ext"));
        assert!(!m.contains("stores data"));
    }

    #[test]
    fn including_dependencies_keeps_external_systems() {
        let m = mermaid_context(&sample_with_apps(), true);
        assert!(m.contains("System_Ext(infra_3, \"Postgres\")"));
        assert!(m.contains("Rel(app_1, infra_3, \"stores data\")"));
        let dsl = structurizr_dsl(&sample_with_apps(), false);
        // Apps-only DSL keeps app→app but not the external system.
        assert!(dsl.contains("app_2 -> app_1 \"http\""));
        assert!(!dsl.contains("infra_3"));
    }

    /// A focused graph: app `a` uses a Postgres datastore + a `mailer` service,
    /// and depends on an external `stripe` system.
    fn focused_graph() -> Value {
        json!({
            "nodes": [
                { "data": { "id": "a", "label": "api", "kind": "application" } },
                { "data": { "id": "infrastructure:1", "label": "Postgres", "kind": "infrastructure", "sub": "database" } },
                { "data": { "id": "service:2", "label": "mailer", "kind": "service", "sub": "email" } },
                { "data": { "id": "ext:stripe", "label": "stripe", "kind": "external" } }
            ],
            "edges": [
                { "data": { "source": "a", "target": "infrastructure:1", "kind": "stores data" } },
                { "data": { "source": "a", "target": "service:2", "kind": "sends" } },
                { "data": { "source": "a", "target": "ext:stripe", "kind": "http" } }
            ]
        })
    }

    #[test]
    fn is_datastore_matches_storage_kinds() {
        assert!(is_datastore("infrastructure", "postgres database"));
        assert!(is_datastore("infrastructure", "redis"));
        assert!(!is_datastore("service", "email"));
    }

    #[test]
    fn container_view_includes_app_datastores_and_boundary() {
        let m = container_mermaid(&focused_graph(), "a", false);
        assert!(m.starts_with("C4Container"));
        assert!(m.contains("System_Boundary(b_a, \"api\")"));
        assert!(m.contains("Container(a_app, \"api\", \"application\")"));
        // Postgres → datastore container.
        assert!(m.contains("ContainerDb(infrastructure_1, \"Postgres\", \"\")"));
        // mailer service → regular container.
        assert!(m.contains("Container(service_2, \"mailer\", \"\")"));
        assert!(m.contains("Rel(a_app, infrastructure_1, \"stores data\")"));
        // External dependency hidden by default.
        assert!(!m.contains("stripe"));
    }

    #[test]
    fn container_view_with_dependencies_shows_external() {
        let m = container_mermaid(&focused_graph(), "a", true);
        assert!(m.contains("System_Ext(ext_stripe, \"stripe\")"));
        assert!(m.contains("Rel(a_app, ext_stripe, \"http\")"));
        let dsl = container_dsl(&focused_graph(), "a", true);
        assert!(dsl.contains("a = softwareSystem \"api\" {"));
        assert!(dsl.contains("infrastructure_1 = container \"Postgres\" \"Database\""));
        assert!(dsl.contains("ext_stripe = softwareSystem \"stripe\" \"External\""));
        assert!(dsl.contains("a_app -> ext_stripe \"http\""));
        assert!(dsl.contains("container a \"containers\""));
    }

    /// An application detail with two components, a use case linking both, and a
    /// dependency attributed to one component.
    fn app_detail() -> Value {
        json!({
            "id": "app-x",
            "name": "billing",
            "components": [
                { "id": "c1", "name": "ApiHandler" },
                { "id": "c2", "name": "PaymentClient" }
            ],
            "use_cases": [
                { "name": "Charge card", "components": [ { "id": "c1" }, { "id": "c2" } ] }
            ],
            "dependencies": [
                { "target_name": "stripe", "kind": "http", "component_id": "c2", "component_name": "PaymentClient" },
                { "target_name": "unattributed", "kind": "http" }
            ]
        })
    }

    #[test]
    fn component_view_has_components_and_edges() {
        let m = component_mermaid(&app_detail());
        assert!(m.starts_with("C4Component"));
        assert!(m.contains("Container_Boundary(b_app_x, \"billing\")"));
        assert!(m.contains("Component(c1, \"ApiHandler\", \"\")"));
        assert!(m.contains("Component(c2, \"PaymentClient\", \"\")"));
        // Inter-component edge from the shared use case.
        assert!(m.contains("Rel(c1, c2, \"Charge card\")"));
        // Component → external from the attributed dependency.
        assert!(m.contains("System_Ext(ext_stripe, \"stripe\")"));
        assert!(m.contains("Rel(c2, ext_stripe, \"http\")"));
        // The unattributed dependency produces no edge.
        assert!(!m.contains("unattributed"));
    }

    #[test]
    fn component_dsl_nests_components_in_a_container() {
        let dsl = component_dsl(&app_detail());
        assert!(dsl.contains("app_x = softwareSystem \"billing\" {"));
        assert!(dsl.contains("app_x_c = container \"billing\" {"));
        assert!(dsl.contains("c1 = component \"ApiHandler\""));
        assert!(dsl.contains("ext_stripe = softwareSystem \"stripe\" \"External\""));
        assert!(dsl.contains("c2 -> ext_stripe \"http\""));
        assert!(dsl.contains("component app_x_c \"components\""));
    }

    #[test]
    fn component_view_empty_is_safe() {
        let m = component_mermaid(&json!({ "id": "z", "name": "empty" }));
        assert!(m.starts_with("C4Component"));
        assert!(m.contains("Container_Boundary(b_z, \"empty\")"));
    }
}
