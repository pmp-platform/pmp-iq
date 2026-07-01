//! Incremental-analysis decision logic (M41): decide whether a sync can
//! re-analyze only the entities affected by the changed files, or must fall back
//! to a full analysis (first sync, unreachable base commit, or a structural
//! change that can alter applications/dependencies/libraries/members).

use std::collections::HashSet;

/// Filenames/path fragments whose change forces a full re-analysis because they
/// can alter applications, dependencies, libraries or members — none of which
/// are file-attributed.
const STRUCTURAL: &[&str] = &[
    // Manifests.
    "cargo.toml", "package.json", "requirements.txt", "pyproject.toml", "go.mod",
    "pom.xml", "build.gradle", "gemfile", "composer.json",
    // Lockfiles.
    "cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "go.sum",
    "gemfile.lock", "poetry.lock", "composer.lock",
    // CI / ownership / containerisation.
    ".github/workflows", ".gitlab-ci.yml", "jenkinsfile", "dockerfile",
    "docker-compose", "codeowners",
];

/// Whether a single path is structural (case-insensitive, by basename or a
/// known path fragment).
pub fn is_structural(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    STRUCTURAL.iter().any(|s| base == *s || lower.contains(s))
}

/// Whether any changed path forces a full re-analysis.
pub fn requires_full(changed: &[String]) -> bool {
    changed.iter().any(|p| is_structural(p))
}

/// The analysis mode for this sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Full,
    Incremental,
}

/// Decide the analysis mode. Full when there's no prior analyzed commit, the
/// base commit is unreachable (force-push/rebase), or a structural file changed;
/// otherwise incremental.
pub fn decide(last_sha: Option<&str>, base_missing: bool, changed: &[String]) -> Mode {
    if last_sha.is_none() || base_missing || requires_full(changed) {
        Mode::Full
    } else {
        Mode::Incremental
    }
}

/// One entity's file attribution (its natural name + the repo paths it covers).
#[derive(Debug, Clone)]
pub struct Attributed {
    pub name: String,
    pub files: Vec<String>,
}

/// The names of entities affected by the changed files (any attributed file is
/// in the changed set). Deterministic, de-duplicated order preserved by input.
pub fn affected(changed: &[String], attributed: &[Attributed]) -> Vec<String> {
    let changed_set: HashSet<&str> = changed.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entity in attributed {
        if entity.files.iter().any(|f| changed_set.contains(f.as_str())) && seen.insert(entity.name.clone()) {
            out.push(entity.name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &str, files: &[&str]) -> Attributed {
        Attributed { name: name.into(), files: files.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn structural_detection() {
        assert!(is_structural("Cargo.toml"));
        assert!(is_structural("services/api/package.json"));
        assert!(is_structural(".github/workflows/ci.yml"));
        assert!(is_structural("CODEOWNERS"));
        assert!(!is_structural("src/auth/login.rs"));
        assert!(!is_structural("README-notes.txt"));
    }

    #[test]
    fn decide_falls_back_to_full() {
        assert_eq!(decide(None, false, &["src/a.rs".into()]), Mode::Full); // first sync
        assert_eq!(decide(Some("sha"), true, &["src/a.rs".into()]), Mode::Full); // base missing
        assert_eq!(decide(Some("sha"), false, &["Cargo.toml".into()]), Mode::Full); // structural
        assert_eq!(decide(Some("sha"), false, &["src/a.rs".into()]), Mode::Incremental);
    }

    #[test]
    fn affected_inverts_attribution() {
        let attributed = vec![
            attr("Login", &["src/auth/login.rs", "src/auth/mod.rs"]),
            attr("Billing", &["src/billing.rs"]),
            attr("Search", &["src/search.rs"]),
        ];
        let changed = vec!["src/auth/login.rs".to_string(), "src/billing.rs".to_string()];
        let got = affected(&changed, &attributed);
        assert_eq!(got, vec!["Login".to_string(), "Billing".to_string()]);
    }

    #[test]
    fn affected_dedups() {
        let attributed = vec![attr("X", &["a.rs"]), attr("X", &["a.rs"])];
        assert_eq!(affected(&["a.rs".into()], &attributed), vec!["X".to_string()]);
    }
}
