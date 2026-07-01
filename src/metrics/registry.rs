//! Metric registry (M33): the single source of truth for which **category** each
//! metric key belongs to. The collection job emits keyed metrics; the repository
//! stamps each row's category from here at write time, so the Insights panel and
//! dashboard can group/rank metrics by theme without per-metric UI code.

/// The theme a metric belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricCategory {
    CodeHealth,
    Security,
    Delivery,
    Ownership,
    Architecture,
    ModelCoverage,
    General,
}

impl MetricCategory {
    /// The stored string form (also the value the API serialises per metric).
    pub fn as_str(self) -> &'static str {
        match self {
            MetricCategory::CodeHealth => "code_health",
            MetricCategory::Security => "security",
            MetricCategory::Delivery => "delivery",
            MetricCategory::Ownership => "ownership",
            MetricCategory::Architecture => "architecture",
            MetricCategory::ModelCoverage => "model_coverage",
            MetricCategory::General => "general",
        }
    }
}

/// Known metric keys → their category. Keys absent here fall back to `General`,
/// so collecting a new metric is additive (it just shows under "General" until
/// registered).
const CATEGORIES: &[(&str, MetricCategory)] = &[
    // Code health (M31 core + M33 additions) — LLM-sourced.
    ("tests_total", MetricCategory::CodeHealth),
    ("tests_passed", MetricCategory::CodeHealth),
    ("tests_failed", MetricCategory::CodeHealth),
    ("coverage_pct", MetricCategory::CodeHealth),
    ("complexity_avg", MetricCategory::CodeHealth),
    ("loc", MetricCategory::CodeHealth),
    ("has_ci", MetricCategory::CodeHealth),
    ("duplication_pct", MetricCategory::CodeHealth),
    ("lint_warnings", MetricCategory::CodeHealth),
    ("todo_count", MetricCategory::CodeHealth),
    ("doc_coverage_pct", MetricCategory::CodeHealth),
    ("fns_over_50_lines", MetricCategory::CodeHealth),
    ("files_over_1000_lines", MetricCategory::CodeHealth),
    ("fns_over_4_params", MetricCategory::CodeHealth),
    // Security / supply chain — LLM-sourced.
    ("vuln_critical", MetricCategory::Security),
    ("vuln_high", MetricCategory::Security),
    ("vuln_medium", MetricCategory::Security),
    ("vuln_low", MetricCategory::Security),
    ("deps_outdated", MetricCategory::Security),
    ("dependency_count", MetricCategory::Security),
    ("secrets_detected", MetricCategory::Security),
    ("max_dep_age_days", MetricCategory::Security),
    // Delivery performance — DORA (M47), derived from deployment/incident events.
    ("dora_deploy_freq_weekly", MetricCategory::Delivery),
    ("dora_lead_time_hours", MetricCategory::Delivery),
    ("dora_change_failure_rate", MetricCategory::Delivery),
    ("dora_mttr_hours", MetricCategory::Delivery),
    // Architecture — derived from the catalog/graph (no LLM).
    ("fan_out", MetricCategory::Architecture),
    ("external_dependency_count", MetricCategory::Architecture),
    // Model coverage (how complete the platform model is) — derived.
    ("component_count", MetricCategory::ModelCoverage),
    ("use_case_count", MetricCategory::ModelCoverage),
    ("observability_signal_count", MetricCategory::ModelCoverage),
    ("has_use_cases", MetricCategory::ModelCoverage),
    ("has_diagrams", MetricCategory::ModelCoverage),
];

/// The category for a metric key (defaults to `General` for unregistered keys).
pub fn category_for(key: &str) -> MetricCategory {
    CATEGORIES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, c)| *c)
        .unwrap_or(MetricCategory::General)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_keys_and_defaults_unknown() {
        assert_eq!(category_for("coverage_pct"), MetricCategory::CodeHealth);
        assert_eq!(category_for("vuln_critical"), MetricCategory::Security);
        assert_eq!(category_for("fan_out"), MetricCategory::Architecture);
        assert_eq!(category_for("has_use_cases"), MetricCategory::ModelCoverage);
        assert_eq!(category_for("something_new"), MetricCategory::General);
        assert_eq!(MetricCategory::ModelCoverage.as_str(), "model_coverage");
    }
}
