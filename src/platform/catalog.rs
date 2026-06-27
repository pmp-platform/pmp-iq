//! The platform catalog: the names of known entities a dependency can target.
//! Used to canonicalize each dependency's free-form `target_name` to an existing
//! entity so the read-layer name-join and the graph link it. Matching is
//! exact → normalized → fuzzy; only genuinely ambiguous fuzzy cases consult the
//! AI provider, and then with a short candidate list — never the whole catalog.

use super::analysis::AnalysisResult;
use crate::ai::{AiProvider, AiRequest};

/// One catalog entry: a canonical entity name and its kind (application,
/// service, external, …).
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub name: String,
    pub kind: String,
}

/// The set of known platform entities a dependency may target (a per-run
/// snapshot).
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
}

/// Outcome of resolving a target name against the catalog.
enum Resolution {
    /// A confident single match: use this canonical name.
    Matched(String),
    /// Several plausible candidates; needs the model to disambiguate.
    Ambiguous(Vec<CatalogEntry>),
    /// No plausible match; leave the name as-is.
    None,
}

/// At most this many candidates are shown to the model when disambiguating.
const MAX_CANDIDATES: usize = 8;

/// Lowercase + drop non-alphanumeric, so `auth-service`/`AuthService` collapse
/// to the same key.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl Catalog {
    pub fn new(entries: Vec<CatalogEntry>) -> Self {
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Resolve a free-form target name to a catalog entry.
    fn resolve(&self, target: &str) -> Resolution {
        if let Some(entry) = self.entries.iter().find(|e| e.name == target) {
            return Resolution::Matched(entry.name.clone());
        }
        let norm = normalize(target);
        if norm.is_empty() {
            return Resolution::None;
        }
        let exact: Vec<CatalogEntry> =
            self.entries.iter().filter(|e| normalize(&e.name) == norm).cloned().collect();
        match exact.len() {
            1 => return Resolution::Matched(exact[0].name.clone()),
            n if n > 1 => return Resolution::Ambiguous(exact),
            _ => {}
        }
        let fuzzy: Vec<CatalogEntry> = self
            .entries
            .iter()
            .filter(|e| {
                let en = normalize(&e.name);
                !en.is_empty() && (en.contains(&norm) || norm.contains(&en))
            })
            .cloned()
            .collect();
        match fuzzy.len() {
            0 => Resolution::None,
            1 => Resolution::Matched(fuzzy[0].name.clone()),
            _ => Resolution::Ambiguous(fuzzy),
        }
    }
}

/// Canonicalize each dependency's `target_name` against the catalog. Confident
/// matches are rewritten in place; ambiguous fuzzy matches consult the provider
/// (when present) with a short candidate list. Returns the number rewritten.
pub async fn resolve_dependencies(
    result: &mut AnalysisResult,
    catalog: &Catalog,
    provider: Option<&dyn AiProvider>,
) -> usize {
    if catalog.is_empty() {
        return 0;
    }
    let mut resolved = 0;
    for dep in &mut result.dependencies {
        let chosen = match catalog.resolve(&dep.target_name) {
            Resolution::Matched(name) => Some(name),
            Resolution::Ambiguous(candidates) => match provider {
                Some(provider) => pick_candidate(provider, &dep.target_name, &candidates).await,
                None => None,
            },
            Resolution::None => None,
        };
        if let Some(name) = chosen {
            if name != dep.target_name {
                dep.target_name = name;
                resolved += 1;
            }
        }
    }
    resolved
}

/// Ask the model which catalog entry a target refers to, from a short list.
/// Returns the chosen canonical name, or `None` to leave the target unchanged.
async fn pick_candidate(
    provider: &dyn AiProvider,
    target: &str,
    candidates: &[CatalogEntry],
) -> Option<String> {
    let shortlist = &candidates[..candidates.len().min(MAX_CANDIDATES)];
    let options = shortlist
        .iter()
        .map(|c| format!("- {} ({})", c.name, c.kind))
        .collect::<Vec<_>>()
        .join("\n");
    let system = "You match a dependency target to a known platform entity. \
        Reply with EXACTLY one of the listed entity names, or NONE if none match. \
        Output only the name (or NONE), nothing else.";
    let prompt = format!(
        "An application's code connects to a system it refers to as \"{target}\". \
         Which of these known entities is it?\n{options}\n\nAnswer:"
    );
    let reply = provider.complete(AiRequest::new(prompt).with_system(system)).await.ok()?;
    let answer = reply.text.trim();
    if answer.eq_ignore_ascii_case("none") {
        return None;
    }
    shortlist
        .iter()
        .find(|c| c.name == answer)
        .or_else(|| shortlist.iter().find(|c| c.name.eq_ignore_ascii_case(answer)))
        .map(|c| c.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::AiResponse;
    use crate::ai::provider::MockAiProvider;

    fn entry(name: &str, kind: &str) -> CatalogEntry {
        CatalogEntry { name: name.into(), kind: kind.into() }
    }

    fn catalog(names: &[(&str, &str)]) -> Catalog {
        Catalog::new(names.iter().map(|(n, k)| entry(n, k)).collect())
    }

    fn result_with_dep(target: &str) -> AnalysisResult {
        let json = format!(
            r#"{{"application":{{"name":"a"}},"dependencies":[{{"target_name":"{target}"}}]}}"#
        );
        AnalysisResult::parse(&json).unwrap()
    }

    #[test]
    fn normalize_collapses_separators_and_case() {
        assert_eq!(normalize("auth-service"), "authservice");
        assert_eq!(normalize("Auth_Service"), "authservice");
        assert_eq!(normalize("AuthService"), "authservice");
    }

    #[tokio::test]
    async fn exact_and_normalized_and_fuzzy_unique_rewrite_without_provider() {
        let cat = catalog(&[("auth-service", "application"), ("billing", "application")]);

        // Exact: unchanged name, not counted as a rewrite.
        let mut exact = result_with_dep("auth-service");
        assert_eq!(resolve_dependencies(&mut exact, &cat, None).await, 0);
        assert_eq!(exact.dependencies[0].target_name, "auth-service");

        // Normalized-unique: "Auth_Service" → "auth-service".
        let mut norm = result_with_dep("Auth_Service");
        assert_eq!(resolve_dependencies(&mut norm, &cat, None).await, 1);
        assert_eq!(norm.dependencies[0].target_name, "auth-service");

        // Fuzzy-unique: "auth" is contained only by "auth-service".
        let mut fuzzy = result_with_dep("auth");
        assert_eq!(resolve_dependencies(&mut fuzzy, &cat, None).await, 1);
        assert_eq!(fuzzy.dependencies[0].target_name, "auth-service");
    }

    #[tokio::test]
    async fn no_match_is_left_unchanged() {
        let cat = catalog(&[("billing", "application")]);
        let mut result = result_with_dep("totally-unknown");
        assert_eq!(resolve_dependencies(&mut result, &cat, None).await, 0);
        assert_eq!(result.dependencies[0].target_name, "totally-unknown");
    }

    #[tokio::test]
    async fn ambiguous_without_provider_is_left_unchanged() {
        let cat = catalog(&[("auth-service", "application"), ("auth-gateway", "service")]);
        let mut result = result_with_dep("auth");
        assert_eq!(resolve_dependencies(&mut result, &cat, None).await, 0);
        assert_eq!(result.dependencies[0].target_name, "auth");
    }

    #[tokio::test]
    async fn ambiguous_with_provider_uses_the_shortlist_pick() {
        let cat = catalog(&[("auth-service", "application"), ("auth-gateway", "service")]);
        let mut provider = MockAiProvider::new();
        provider.expect_complete().returning(|_| {
            Ok(AiResponse { text: "auth-gateway".into(), input_tokens: None, output_tokens: None })
        });
        let mut result = result_with_dep("auth");
        assert_eq!(resolve_dependencies(&mut result, &cat, Some(&provider)).await, 1);
        assert_eq!(result.dependencies[0].target_name, "auth-gateway");
    }

    #[tokio::test]
    async fn ambiguous_with_provider_none_reply_leaves_unchanged() {
        let cat = catalog(&[("auth-service", "application"), ("auth-gateway", "service")]);
        let mut provider = MockAiProvider::new();
        provider.expect_complete().returning(|_| {
            Ok(AiResponse { text: "NONE".into(), input_tokens: None, output_tokens: None })
        });
        let mut result = result_with_dep("auth");
        assert_eq!(resolve_dependencies(&mut result, &cat, Some(&provider)).await, 0);
        assert_eq!(result.dependencies[0].target_name, "auth");
    }
}
