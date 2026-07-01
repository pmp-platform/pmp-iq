//! Configurable extraction prompts (M34): the analyzer's system prompt is
//! composed from per-section templates that operators can edit in Settings,
//! while the strict kinds/properties vocabulary and JSON output schema are
//! always injected programmatically (so edits can never weaken the contract).

use super::analysis::AnalysisConfig;
use super::{KindDef, PropertyDef};
use std::collections::BTreeMap;

/// The required JSON output shape, injected wherever `{{json_schema}}` appears.
pub const JSON_SCHEMA: &str = "{\"application\":{\"name\":string,\"app_type\":string,\"description\":string,\
\"primary_language\":string,\"metadata\":object},\"languages\":[{\"name\":string,\"percentage\":number}],\
\"libraries\":[{\"name\":string,\"ecosystem\":string,\"version\":string,\"scope\":string,\"metadata\":object}],\
\"infrastructure\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"tools\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"cloud_providers\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"services\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"platforms\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"external\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"dependencies\":[{\"target_name\":string,\"kind\":string,\"description\":string,\"component\":string,\"endpoint\":string,\"metadata\":object}],\
\"endpoints\":[{\"operation\":string,\"protocol\":\"http\"|\"grpc\"|\"graphql\",\"summary\":string,\"component\":string,\"files\":[string]}],\
\"users\":[{\"username\":string,\"email\":string,\"groups\":[string],\"metadata\":object}],\
\"groups\":[{\"name\":string,\"metadata\":object}],\
\"access\":[{\"principal_type\":\"user\"|\"group\",\"principal_name\":string,\"access_level\":string}],\
\"components\":[{\"name\":string,\"kind\":string,\"description\":string,\"metadata\":object,\"files\":[string],\
\"observability_signals\":[{\"name\":string,\"kind\":string,\"description\":string,\"metadata\":object}]}],\
\"use_cases\":[{\"name\":string,\"description\":string,\"metadata\":object,\"components\":[string],\"files\":[string],\
\"diagrams\":[{\"name\":string,\"kind\":string,\"description\":string,\"content\":string,\"metadata\":object}]}]}";

/// Analyzer sections, composed into the system prompt in this fixed order.
pub const ANALYZER_SECTIONS: &[&str] = &[
    "base", "applications", "components", "observability", "use_cases", "diagrams", "endpoints",
    "dependencies", "members",
];

/// The metrics-collection section (M31/M33), used by the metrics job's passes.
pub const METRICS_SECTION: &str = "metrics";

/// Every editable section key (analyzer order, then metrics).
pub fn all_sections() -> Vec<&'static str> {
    let mut v = ANALYZER_SECTIONS.to_vec();
    v.push(METRICS_SECTION);
    v
}

/// Default template text per section, decomposed from the original monolithic
/// system prompt so behaviour is unchanged until edited.
const DEFAULTS: &[(&str, &str)] = &[
    ("base", "You are a software platform analyst. Given files from a repository, extract structured \
metadata. Respond with ONLY a JSON object (no prose, no markdown fences) of the shape: {{json_schema}}. \
Use empty arrays when unknown."),
    ("applications", "For 'application', set a concise 'app_type', a thorough 'description', and the \
'primary_language'; list the 'languages' with rough percentages and the 'libraries' the project declares \
with their ecosystem and version."),
    ("components", "'components' are the internal building blocks of THIS application (e.g. controllers, \
models, services); give each a thorough 'description'. For every component set 'files' to the list of \
repository-relative paths (e.g. \"src/auth/login.rs\") it is implemented in; use an empty array when unknown."),
    ("observability", "For each component list the observability signals (metrics, traces, logs) it emits \
in 'observability_signals', each with its 'kind' and a description."),
    ("use_cases", "'use_cases' are the capabilities the application fulfils; give each a thorough \
'description' and reference the involved components by their exact 'name'. For every use case set 'files' \
to the repository-relative paths it affects; use an empty array when unknown."),
    ("diagrams", "For every use case ALWAYS include at least these two diagrams: (1) a sequence diagram \
with 'kind' set to \"sequence\" whose 'content' is a mermaid sequenceDiagram tracing the interaction \
between the actors and the involved components; and (2) a component diagram with 'kind' set to \"component\" \
whose 'content' is a mermaid flowchart/graph showing the involved components and how they connect. Each \
diagram's 'content' MUST be valid mermaid source that renders standalone (no markdown fences), and its \
'kind' names the mermaid diagram type."),
    ("endpoints", "'endpoints' are the API operations THIS application exposes to others: for each, set \
'operation' (e.g. \"POST /charge\" for http, \"billing.Charge\" for grpc, \"mutation pay\" for graphql), \
'protocol' (http | grpc | graphql), a short 'summary', the 'component' (by exact name) that implements it, \
and the repository 'files' it lives in. When a dependency calls another application's operation, set that \
dependency's 'endpoint' to the EXACT operation string."),
    ("dependencies", "Classify each discovered dependency into exactly one array: 'infrastructure' = \
self-hosted runtime backing services (database, cache, queue, storage, message broker); 'tools' = \
build/orchestration/dev tooling, not runtime (docker compose, gradle, maven, make, npm, terraform, CI like \
github actions); 'cloud_providers' = cloud platforms (AWS, GCP, Azure, Cloudflare); 'services' = \
third-party or internal network APIs the app calls (Stripe, Twilio, an internal auth-service); 'platforms' \
= SaaS for observability/identity/CI/error tracking (Datadog, Auth0, Sentry); 'external' = any other \
external dependency that fits none of the above; 'dependencies' = the outbound connections THIS app makes \
to other applications or services, detected from code (HTTP/REST/gRPC clients, database/cache/queue \
connections, SDK calls, configured service URLs or hostnames). For each, set 'target_name' to the \
application or service it connects to, 'kind' to the connection type (http, grpc, db, queue, cache, etc.), \
and 'component' to the EXACT name of the component (from the 'components' array) that makes the connection. \
A target may also appear in 'services'/'external' (the catalog of things used); 'dependencies' additionally \
records the connection edge and its component. Never place the same thing in more than one of the catalog \
arrays (infrastructure/tools/cloud_providers/services/platforms/external)."),
    ("members", "Populate 'users', 'groups' and 'access' ONLY from a CODEOWNERS file (its code owners and \
owning teams); leave all three as empty arrays when there is no CODEOWNERS file. Repository membership is \
collected separately from the provider, so do not infer members from commits, READMEs or other files."),
    ("metrics", "You are a senior software quality and security analyst. Inspect the repository checkout \
and report precise, evidence-based metrics from its code, tests, coverage reports and CI configuration. \
Never fabricate a number — omit any metric you cannot determine from the checkout."),
];

/// Placeholders a section's template must retain on save (so injection works).
pub fn required_placeholders(section: &str) -> &'static [&'static str] {
    match section {
        "base" => &["{{json_schema}}"],
        _ => &[],
    }
}

/// One editable prompt section.
#[derive(Debug, Clone)]
pub struct PromptSection {
    pub template: String,
    pub enabled: bool,
}

/// The active per-section prompt templates (defaults + stored overrides).
#[derive(Debug, Clone)]
pub struct PromptConfig {
    pub sections: BTreeMap<String, PromptSection>,
}

impl Default for PromptConfig {
    fn default() -> Self {
        let sections = DEFAULTS
            .iter()
            .map(|(k, t)| (k.to_string(), PromptSection { template: t.to_string(), enabled: true }))
            .collect();
        Self { sections }
    }
}

impl PromptConfig {
    /// The shipped default template for a section, if any.
    pub fn default_template(section: &str) -> Option<&'static str> {
        DEFAULTS.iter().find(|(k, _)| *k == section).map(|(_, t)| *t)
    }

    /// Apply a stored override for a section (creating it if new).
    pub fn set(&mut self, section: &str, template: String, enabled: bool) {
        self.sections.insert(section.to_string(), PromptSection { template, enabled });
    }

    /// The metrics-collection preamble, if its section is enabled.
    pub fn metrics_template(&self) -> String {
        self.sections
            .get(METRICS_SECTION)
            .filter(|s| s.enabled)
            .map(|s| s.template.clone())
            .unwrap_or_default()
    }
}

/// Reject an empty template or one missing a required placeholder.
pub fn validate_section(section: &str, template: &str) -> Result<(), String> {
    if template.trim().is_empty() {
        return Err("template must not be empty".into());
    }
    for ph in required_placeholders(section) {
        if !template.contains(ph) {
            return Err(format!("template for '{section}' must contain the {ph} placeholder"));
        }
    }
    Ok(())
}

/// The prompt field label carrying the kind for an entity type.
fn kind_field_label(entity: &str) -> String {
    match entity {
        "applications" => "application app_type".to_string(),
        "libraries" => "library ecosystem".to_string(),
        other => format!("{other} kind"),
    }
}

fn describe_kind(k: &KindDef) -> String {
    if k.description.trim().is_empty() {
        k.kind_id.clone()
    } else {
        format!("{} ({})", k.kind_id, k.description.trim())
    }
}

fn describe_property(p: &PropertyDef) -> String {
    if p.description.trim().is_empty() {
        format!("{} ({})", p.prop_id, p.data_type)
    } else {
        format!("{} ({}, {})", p.prop_id, p.data_type, p.description.trim())
    }
}

/// The strict allowed-kinds injection (empty when no kinds are configured).
fn kinds_injection(config: &AnalysisConfig) -> String {
    let sections: Vec<String> = config
        .kinds
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(entity, kinds)| {
            let list = kinds.iter().map(describe_kind).collect::<Vec<_>>().join(", ");
            format!("{}: [{}]", kind_field_label(entity), list)
        })
        .collect();
    if sections.is_empty() {
        return String::new();
    }
    format!(
        "Allowed kind values per type — output EXACTLY one listed id (the value before any parenthesis) \
         for each item's kind; never invent a value. An item whose kind is not listed will be discarded, \
         so prefer the closest listed id. {}.",
        sections.join("; ")
    )
}

/// The strict extraction-properties injection (empty when none are configured).
fn props_injection(config: &AnalysisConfig) -> String {
    let sections: Vec<String> = config
        .properties
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(entity, props)| {
            let list = props.iter().map(describe_property).collect::<Vec<_>>().join(", ");
            format!("{entity} — {list}")
        })
        .collect();
    if sections.is_empty() {
        return String::new();
    }
    format!(
        "Populate each entity's metadata object ONLY with these keys (when known); do not add any other \
         keys (unlisted keys are discarded): {}.",
        sections.join("; ")
    )
}

/// Compose the analyzer system prompt: each enabled section in fixed order,
/// then the always-injected schema + strict kinds/properties vocabulary.
pub fn compose_system_prompt(prompts: &PromptConfig, config: &AnalysisConfig) -> String {
    let mut out = String::new();
    for &key in ANALYZER_SECTIONS {
        if let Some(s) = prompts.sections.get(key) {
            if s.enabled && !s.template.trim().is_empty() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(s.template.trim());
            }
        }
    }
    // Guarantee the schema + strict vocabulary are always present, even if a
    // section that held a placeholder was disabled.
    if !out.contains("{{json_schema}}") {
        out.push_str(" Respond with the JSON shape: {{json_schema}}.");
    }
    out.push_str(" {{kinds}} {{properties}}");

    out = out.replace("{{json_schema}}", JSON_SCHEMA);
    out = out.replace("{{kinds}}", &kinds_injection(config));
    out = out.replace("{{properties}}", &props_injection(config));
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{KindDef, PropertyDef};

    fn config_with_vocab() -> AnalysisConfig {
        let mut cfg = AnalysisConfig::default();
        cfg.kinds.insert(
            "services".into(),
            vec![KindDef { kind_id: "payments".into(), name: "Payments".into(), description: String::new() }],
        );
        cfg.properties.insert(
            "applications".into(),
            vec![PropertyDef { prop_id: "framework".into(), name: "F".into(), description: String::new(), data_type: "string".into() }],
        );
        cfg
    }

    #[test]
    fn composes_base_sections_schema_and_vocabulary() {
        let cfg = config_with_vocab();
        let prompt = compose_system_prompt(&cfg.prompts, &cfg);
        assert!(prompt.contains("You are a software platform analyst"));
        assert!(prompt.contains("\"application\":{\"name\":string")); // schema injected
        assert!(prompt.contains("Classify each discovered dependency")); // dependencies section
        assert!(prompt.contains("services kind: [payments]")); // kinds injection
        assert!(prompt.contains("framework (string)")); // properties injection
        assert!(!prompt.contains("{{")); // all placeholders substituted
    }

    #[test]
    fn disabled_section_is_omitted() {
        let mut cfg = config_with_vocab();
        cfg.prompts.set("dependencies", PromptConfig::default_template("dependencies").unwrap().into(), false);
        let prompt = compose_system_prompt(&cfg.prompts, &cfg);
        assert!(!prompt.contains("Classify each discovered dependency"));
        // Schema + vocabulary still injected.
        assert!(prompt.contains("\"application\":{\"name\":string"));
        assert!(prompt.contains("services kind: [payments]"));
    }

    #[test]
    fn edited_section_text_takes_effect() {
        let mut cfg = config_with_vocab();
        cfg.prompts.set("members", "Custom members rule.".into(), true);
        let prompt = compose_system_prompt(&cfg.prompts, &cfg);
        assert!(prompt.contains("Custom members rule."));
        assert!(!prompt.contains("CODEOWNERS"));
    }

    #[test]
    fn validation_requires_placeholder_and_non_empty() {
        assert!(validate_section("base", "no placeholder here").is_err());
        assert!(validate_section("base", "schema {{json_schema}} ok").is_ok());
        assert!(validate_section("members", "any text").is_ok());
        assert!(validate_section("members", "   ").is_err());
    }

    #[test]
    fn metrics_template_respects_enabled() {
        let mut cfg = PromptConfig::default();
        assert!(cfg.metrics_template().contains("quality and security analyst"));
        cfg.set("metrics", "x".into(), false);
        assert_eq!(cfg.metrics_template(), "");
    }
}
