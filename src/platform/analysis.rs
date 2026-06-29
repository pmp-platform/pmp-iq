//! The typed schema the AI returns for a repository, plus validation.

use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// A configured allowed `kind`/`app_type`/`ecosystem` value for an entity type.
#[derive(Debug, Clone)]
pub struct KindDef {
    pub kind_id: String,
    pub name: String,
    pub description: String,
}

/// A configured extraction property for an entity type.
#[derive(Debug, Clone)]
pub struct PropertyDef {
    pub prop_id: String,
    pub name: String,
    pub description: String,
    pub data_type: String,
}

/// User-configured analysis vocabulary keyed by entity-type name (e.g.
/// "applications", "services"): the allowed `kind`/`app_type`/`ecosystem` values
/// and the properties to extract into each entity's `metadata`.
#[derive(Debug, Clone, Default)]
pub struct AnalysisConfig {
    pub kinds: BTreeMap<String, Vec<KindDef>>,
    pub properties: BTreeMap<String, Vec<PropertyDef>>,
}

impl AnalysisConfig {
    /// The allowed kind-id set for an entity type, if a non-empty list is
    /// configured (otherwise `None` = unconstrained).
    fn allowed_kinds(&self, entity: &str) -> Option<HashSet<&str>> {
        self.kinds
            .get(entity)
            .filter(|v| !v.is_empty())
            .map(|v| v.iter().map(|k| k.kind_id.as_str()).collect())
    }

    /// The allowed property-id set for an entity type, if a non-empty list is
    /// configured (otherwise `None` = unconstrained).
    fn allowed_props(&self, entity: &str) -> Option<HashSet<&str>> {
        self.properties
            .get(entity)
            .filter(|v| !v.is_empty())
            .map(|v| v.iter().map(|p| p.prop_id.as_str()).collect())
    }
}

/// Top-level analysis result for one repository.
#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisResult {
    pub application: AppInfo,
    #[serde(default)]
    pub languages: Vec<LanguageInfo>,
    #[serde(default)]
    pub libraries: Vec<LibraryInfo>,
    #[serde(default)]
    pub infrastructure: Vec<LinkedInfo>,
    #[serde(default)]
    pub tools: Vec<LinkedInfo>,
    #[serde(default)]
    pub cloud_providers: Vec<LinkedInfo>,
    #[serde(default)]
    pub services: Vec<LinkedInfo>,
    #[serde(default)]
    pub platforms: Vec<LinkedInfo>,
    #[serde(default)]
    pub external: Vec<LinkedInfo>,
    #[serde(default)]
    pub dependencies: Vec<DependencyInfo>,
    #[serde(default)]
    pub users: Vec<UserInfo>,
    #[serde(default)]
    pub groups: Vec<GroupInfo>,
    #[serde(default)]
    pub access: Vec<AccessInfo>,
    #[serde(default)]
    pub components: Vec<ComponentInfo>,
    #[serde(default)]
    pub use_cases: Vec<UseCaseInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppInfo {
    pub name: String,
    #[serde(default)]
    pub app_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub primary_language: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LanguageInfo {
    pub name: String,
    #[serde(default)]
    pub percentage: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryInfo {
    pub name: String,
    pub ecosystem: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

/// A linked entity (infrastructure, tool, cloud provider, service, platform or
/// external dependency): a named `(kind, version)` thing an app uses, with
/// free-form metadata. One shape backs every entity in [`crate::platform::LINKED`].
#[derive(Debug, Clone, Deserialize)]
pub struct LinkedInfo {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub usage: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DependencyInfo {
    pub target_name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Name of the component (from `components`) that makes this connection.
    #[serde(default)]
    pub component: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserInfo {
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupInfo {
    pub name: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccessInfo {
    pub principal_type: String,
    pub principal_name: String,
    pub access_level: String,
}

/// An internal component of an application (controller, model, service, …) with
/// its kind and the observability signals it emits.
#[derive(Debug, Clone, Deserialize)]
pub struct ComponentInfo {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub observability_signals: Vec<ObservabilitySignalInfo>,
    /// Repository-relative paths this component is implemented in.
    #[serde(default)]
    pub files: Vec<String>,
}

/// A signal a component emits (metric, trace, log, …).
#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilitySignalInfo {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

/// A use case an application fulfils, referencing components (by name) and
/// carrying mermaid diagrams.
#[derive(Debug, Clone, Deserialize)]
pub struct UseCaseInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    /// Names of the application's components this use case involves.
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub diagrams: Vec<DiagramInfo>,
    /// Repository-relative paths this use case affects.
    #[serde(default)]
    pub files: Vec<String>,
}

/// A mermaid diagram (its `kind` names the mermaid diagram type; `content` is the
/// mermaid source).
#[derive(Debug, Clone, Deserialize)]
pub struct DiagramInfo {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub metadata: Value,
}

/// A repository member association sourced from a git-provider API (e.g. a
/// GitHub collaborator). Reconciled into the application↔principal association
/// as `member`/`ex_member`. Kept provider-neutral so `platform` stays decoupled
/// from `accounts`.
#[derive(Debug, Clone)]
pub struct MemberInfo {
    pub username: String,
    pub email: Option<String>,
    /// Provider role (stored as `access_level`).
    pub role: Option<String>,
    /// Raw provider permission flags (stored on the grant).
    pub permissions: Value,
    /// Free-form metadata stored on the user row.
    pub metadata: Value,
}

impl AnalysisResult {
    /// The linked-entity array for a registry entity name (empty if unknown).
    pub fn linked_items(&self, name: &str) -> &[LinkedInfo] {
        match name {
            "infrastructure" => &self.infrastructure,
            "tools" => &self.tools,
            "cloud-providers" => &self.cloud_providers,
            "services" => &self.services,
            "platforms" => &self.platforms,
            "external" => &self.external,
            _ => &[],
        }
    }

    /// Enforce the configured vocabulary: drop entities whose `kind` is not in
    /// the allowed list and strip metadata keys that are not configured
    /// properties. A type with no (non-empty) configured list is unconstrained.
    pub fn apply_config(&mut self, cfg: &AnalysisConfig) {
        self.enforce_kinds(cfg);
        self.strip_properties(cfg);
    }

    /// Drop collection entities with a disallowed `kind`; clear an invalid
    /// application `app_type` (the application itself is always kept).
    fn enforce_kinds(&mut self, cfg: &AnalysisConfig) {
        if let Some(allowed) = cfg.allowed_kinds("applications") {
            let keep = self.application.app_type.as_deref().is_none_or(|t| allowed.contains(t));
            if !keep {
                self.application.app_type = None;
            }
        }
        if let Some(allowed) = cfg.allowed_kinds("libraries") {
            self.libraries.retain(|lib| allowed.contains(lib.ecosystem.as_str()));
        }
        for (name, items) in self.linked_arrays() {
            if let Some(allowed) = cfg.allowed_kinds(name) {
                items.retain(|item| allowed.contains(item.kind.as_str()));
            }
        }
        if let Some(allowed) = cfg.allowed_kinds("components") {
            self.components.retain(|c| allowed.contains(c.kind.as_str()));
        }
        if let Some(allowed) = cfg.allowed_kinds("observability-signals") {
            for component in &mut self.components {
                component.observability_signals.retain(|s| allowed.contains(s.kind.as_str()));
            }
        }
        if let Some(allowed) = cfg.allowed_kinds("diagrams") {
            for use_case in &mut self.use_cases {
                use_case.diagrams.retain(|d| allowed.contains(d.kind.as_str()));
            }
        }
    }

    /// Strip metadata keys outside the configured property set for each entity
    /// type that has a (non-empty) configured set.
    fn strip_properties(&mut self, cfg: &AnalysisConfig) {
        if let Some(allowed) = cfg.allowed_props("applications") {
            strip_metadata(&mut self.application.metadata, &allowed);
        }
        if let Some(allowed) = cfg.allowed_props("libraries") {
            for lib in &mut self.libraries {
                strip_metadata(&mut lib.metadata, &allowed);
            }
        }
        for (name, items) in self.linked_arrays() {
            if let Some(allowed) = cfg.allowed_props(name) {
                for item in items {
                    strip_metadata(&mut item.metadata, &allowed);
                }
            }
        }
        if let Some(allowed) = cfg.allowed_props("users") {
            for user in &mut self.users {
                strip_metadata(&mut user.metadata, &allowed);
            }
        }
        if let Some(allowed) = cfg.allowed_props("groups") {
            for group in &mut self.groups {
                strip_metadata(&mut group.metadata, &allowed);
            }
        }
        self.strip_subentity_properties(cfg);
    }

    /// Metadata stripping for the application sub-entities.
    fn strip_subentity_properties(&mut self, cfg: &AnalysisConfig) {
        let component_props = cfg.allowed_props("components");
        let signal_props = cfg.allowed_props("observability-signals");
        for component in &mut self.components {
            if let Some(allowed) = &component_props {
                strip_metadata(&mut component.metadata, allowed);
            }
            if let Some(allowed) = &signal_props {
                for signal in &mut component.observability_signals {
                    strip_metadata(&mut signal.metadata, allowed);
                }
            }
        }
        let diagram_props = cfg.allowed_props("diagrams");
        for use_case in &mut self.use_cases {
            if let Some(allowed) = cfg.allowed_props("use-cases") {
                strip_metadata(&mut use_case.metadata, &allowed);
            }
            if let Some(allowed) = &diagram_props {
                for diagram in &mut use_case.diagrams {
                    strip_metadata(&mut diagram.metadata, allowed);
                }
            }
        }
    }

    /// The six linked-entity arrays paired with their registry names.
    fn linked_arrays(&mut self) -> [(&'static str, &mut Vec<LinkedInfo>); 6] {
        [
            ("infrastructure", &mut self.infrastructure),
            ("tools", &mut self.tools),
            ("cloud-providers", &mut self.cloud_providers),
            ("services", &mut self.services),
            ("platforms", &mut self.platforms),
            ("external", &mut self.external),
        ]
    }

    /// Parse from a (possibly fenced) model response, then validate.
    pub fn parse(text: &str) -> Result<Self, String> {
        let json = extract_json(text);
        let result: AnalysisResult =
            serde_json::from_str(&json).map_err(|e| format!("invalid analysis JSON: {e}"))?;
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> Result<(), String> {
        if self.application.name.trim().is_empty() {
            return Err("application.name is required".into());
        }
        for access in &self.access {
            if access.principal_type != "user" && access.principal_type != "group" {
                return Err(format!("invalid principal_type '{}'", access.principal_type));
            }
        }
        Ok(())
    }
}

/// Remove object keys from `metadata` that are not in the `allowed` set. A
/// non-object value is left unchanged.
fn strip_metadata(metadata: &mut Value, allowed: &HashSet<&str>) {
    if let Some(obj) = metadata.as_object_mut() {
        obj.retain(|key, _| allowed.contains(key.as_str()));
    }
}

/// Strip Markdown code fences and isolate the first JSON object.
fn extract_json(text: &str) -> String {
    let trimmed = text.trim();
    let without_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|s| s.trim_start())
        .unwrap_or(trimmed);
    let body = without_fence.strip_suffix("```").unwrap_or(without_fence);
    match (body.find('{'), body.rfind('}')) {
        (Some(start), Some(end)) if end > start => body[start..=end].to_string(),
        _ => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json() {
        let text = "```json\n{\"application\":{\"name\":\"api\"},\"languages\":[{\"name\":\"Rust\"}]}\n```";
        let result = AnalysisResult::parse(text).unwrap();
        assert_eq!(result.application.name, "api");
        assert_eq!(result.languages.len(), 1);
    }

    #[test]
    fn parses_with_surrounding_prose() {
        let text = "Here is the analysis:\n{\"application\":{\"name\":\"web\"}}\nThanks!";
        let result = AnalysisResult::parse(text).unwrap();
        assert_eq!(result.application.name, "web");
    }

    fn kind(kind_id: &str) -> KindDef {
        KindDef { kind_id: kind_id.into(), name: kind_id.into(), description: String::new() }
    }

    #[test]
    fn apply_config_enforces_subentity_kinds() {
        let text = r#"{"application":{"name":"a"},
            "components":[
              {"name":"Ctl","kind":"controller","observability_signals":[
                {"name":"reqs","kind":"metric"},{"name":"bad","kind":"nope"}]},
              {"name":"Weird","kind":"made-up"}],
            "use_cases":[{"name":"Register","components":["Ctl"],
              "diagrams":[{"name":"Flow","kind":"flowchart","content":"graph TD; A-->B"},
                          {"name":"Bad","kind":"nope","content":"x"}]}]}"#;
        let mut result = AnalysisResult::parse(text).unwrap();
        let mut cfg = AnalysisConfig::default();
        cfg.kinds.insert("components".into(), vec![kind("controller")]);
        cfg.kinds.insert("observability-signals".into(), vec![kind("metric")]);
        cfg.kinds.insert("diagrams".into(), vec![kind("flowchart")]);
        result.apply_config(&cfg);
        assert_eq!(result.components.len(), 1, "disallowed component dropped");
        assert_eq!(result.components[0].name, "Ctl");
        assert_eq!(result.components[0].observability_signals.len(), 1, "disallowed signal dropped");
        assert_eq!(result.use_cases[0].diagrams.len(), 1, "disallowed diagram dropped");
        assert_eq!(result.use_cases[0].diagrams[0].kind, "flowchart");
    }

    #[test]
    fn apply_config_drops_disallowed_kinds() {
        let text = r#"{"application":{"name":"a","app_type":"weird"},
            "libraries":[{"name":"x","ecosystem":"weird"},{"name":"y","ecosystem":"cargo"}],
            "services":[{"name":"s","kind":"weird"}],
            "tools":[{"name":"t","kind":"build"}]}"#;
        let mut result = AnalysisResult::parse(text).unwrap();
        let mut cfg = AnalysisConfig::default();
        for (entity, value) in [
            ("applications", "api"),
            ("libraries", "cargo"),
            ("services", "payments"),
            ("tools", "build"),
        ] {
            cfg.kinds.insert(entity.into(), vec![kind(value)]);
        }
        result.apply_config(&cfg);
        // Invalid application app_type is cleared; the application is kept.
        assert_eq!(result.application.app_type, None);
        // Disallowed collection entities are dropped, allowed ones kept.
        assert_eq!(result.libraries.len(), 1);
        assert_eq!(result.libraries[0].ecosystem, "cargo");
        assert!(result.services.is_empty(), "disallowed service dropped");
        assert_eq!(result.tools.len(), 1, "allowed tool kept");
    }

    #[test]
    fn apply_config_strips_unconfigured_metadata_keys() {
        let text = r#"{"application":{"name":"a","metadata":{"framework":"axum","secret":"x"}}}"#;
        let mut result = AnalysisResult::parse(text).unwrap();
        let mut cfg = AnalysisConfig::default();
        cfg.properties.insert("applications".into(), vec![PropertyDef {
            prop_id: "framework".into(),
            name: "Framework".into(),
            description: String::new(),
            data_type: "string".into(),
        }]);
        result.apply_config(&cfg);
        assert_eq!(result.application.metadata["framework"], "axum");
        assert!(result.application.metadata.get("secret").is_none(), "unconfigured key stripped");
    }

    #[test]
    fn apply_config_leaves_unconfigured_types_untouched() {
        let text = r#"{"application":{"name":"a","metadata":{"k":"v"}},"services":[{"name":"s","kind":"weird"}]}"#;
        let mut result = AnalysisResult::parse(text).unwrap();
        result.apply_config(&AnalysisConfig::default());
        assert_eq!(result.services[0].kind, "weird");
        assert_eq!(result.application.metadata["k"], "v");
    }

    #[test]
    fn rejects_empty_name() {
        let text = "{\"application\":{\"name\":\"\"}}";
        assert!(AnalysisResult::parse(text).is_err());
    }

    #[test]
    fn rejects_invalid_principal_type() {
        let text = r#"{"application":{"name":"a"},"access":[{"principal_type":"robot","principal_name":"x","access_level":"read"}]}"#;
        assert!(AnalysisResult::parse(text).is_err());
    }

    #[test]
    fn invalid_json_errors() {
        assert!(AnalysisResult::parse("not json").is_err());
    }
}
