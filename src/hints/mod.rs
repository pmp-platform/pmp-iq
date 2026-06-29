//! Per-entity LLM hints: user-provided corrections injected into the analysis
//! prompt so the model can fix or augment what it inferred for an application
//! and its entities.

pub mod model;
pub mod repository;

pub use model::{EntityHint, EntityHintInput};
pub use repository::{
    EntityHintRepository, PgEntityHintRepository, SqliteEntityHintRepository,
};

/// Render an application's hints as an authoritative-corrections block appended
/// to the analysis prompt. Returns an empty string when there are no hints.
pub fn render_hints(hints: &[EntityHint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\nUser-provided hints — AUTHORITATIVE corrections you MUST honor over your own \
         inference. Each is scoped to an entity type and (optionally) a specific entity by name:",
    );
    for hint in hints {
        let scope = if hint.entity_key.trim().is_empty() {
            format!("all {}", hint.entity_type)
        } else {
            format!("{} '{}'", hint.entity_type, hint.entity_key)
        };
        out.push_str(&format!("\n- [{scope}] {}", hint.hint.trim()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn hint(entity_type: &str, key: &str, text: &str) -> EntityHint {
        EntityHint {
            id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            entity_type: entity_type.into(),
            entity_key: key.into(),
            hint: text.into(),
        }
    }

    #[test]
    fn render_is_empty_without_hints() {
        assert_eq!(render_hints(&[]), "");
    }

    #[test]
    fn render_scopes_type_and_specific_entity() {
        let hints = vec![
            hint("use_case", "Checkout", "include the refund path"),
            hint("library", "", "ignore dev dependencies"),
        ];
        let out = render_hints(&hints);
        assert!(out.contains("[use_case 'Checkout'] include the refund path"), "{out}");
        assert!(out.contains("[all library] ignore dev dependencies"), "{out}");
    }
}
