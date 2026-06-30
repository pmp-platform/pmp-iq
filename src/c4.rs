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
}
