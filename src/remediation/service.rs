//! Remediation evaluation + approval (M46): match rules against per-application
//! signals and propose deduplicated remediations; approve flips status.

use super::repository::RemediationRepository;
use super::trigger::{AppSignals, trigger_matches};
use crate::error::AppError;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct RemediationService {
    repo: Arc<dyn RemediationRepository>,
}

/// The application ids a rule's scope restricts to (`None` = whole fleet).
fn scope_app_ids(scope: &Value) -> Option<HashSet<Uuid>> {
    let arr = scope.get("application_ids").and_then(Value::as_array)?;
    let set: HashSet<Uuid> = arr.iter().filter_map(|v| v.as_str()).filter_map(|s| Uuid::parse_str(s).ok()).collect();
    if set.is_empty() { None } else { Some(set) }
}

impl RemediationService {
    pub fn new(repo: Arc<dyn RemediationRepository>) -> Self {
        Self { repo }
    }

    /// Evaluate every enabled rule against each application's signals and propose
    /// remediations for matches (deduped). Returns the number newly proposed.
    pub async fn evaluate(&self, apps: &[(Uuid, AppSignals)]) -> Result<usize, AppError> {
        let rules = self.repo.list_rules().await?;
        let mut proposed = 0;
        for rule in rules.iter().filter(|r| r.enabled) {
            let allowed = scope_app_ids(&rule.scope);
            for (app_id, signals) in apps {
                if allowed.as_ref().is_some_and(|set| !set.contains(app_id)) {
                    continue;
                }
                if trigger_matches(&rule.trigger_kind, &rule.params, signals)
                    && self.repo.propose(rule.id, *app_id, &app_id.to_string()).await?
                {
                    proposed += 1;
                }
            }
        }
        Ok(proposed)
    }

    /// Mark an approved remediation as running, linked to the agent task driving it.
    pub async fn mark_running(&self, remediation_id: Uuid, agent_task_id: Uuid) -> Result<(), AppError> {
        self.repo.set_status(remediation_id, "running", Some(agent_task_id)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remediation::repository::{MockRemediationRepository, RemediationRule};
    use serde_json::json;
    use std::collections::HashMap;

    fn rule(trigger: &str, params: Value) -> RemediationRule {
        RemediationRule {
            id: Uuid::new_v4(),
            name: "r".into(),
            trigger_kind: trigger.into(),
            params,
            action: "agent_task".into(),
            prompt: "fix it".into(),
            scope: json!({}),
            auto_approve: false,
            enabled: true,
        }
    }

    fn signals(coverage: f64) -> AppSignals {
        AppSignals { metrics: HashMap::from([("coverage_pct".to_string(), coverage)]), ..Default::default() }
    }

    #[tokio::test]
    async fn proposes_for_matching_apps_only() {
        let low = Uuid::new_v4();
        let high = Uuid::new_v4();
        let mut repo = MockRemediationRepository::new();
        repo.expect_list_rules()
            .returning(|| Ok(vec![rule("metric_below", json!({ "metric": "coverage_pct", "threshold": 70 }))]));
        // Only the low-coverage app is proposed.
        repo.expect_propose()
            .withf(move |_, app, _| *app == low)
            .times(1)
            .returning(|_, _, _| Ok(true));

        let svc = RemediationService::new(Arc::new(repo));
        let apps = vec![(low, signals(40.0)), (high, signals(90.0))];
        assert_eq!(svc.evaluate(&apps).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn scope_restricts_to_listed_apps() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut repo = MockRemediationRepository::new();
        let scoped = rule("dep_eol", json!({}));
        let mut scoped = scoped;
        scoped.scope = json!({ "application_ids": [a.to_string()] });
        repo.expect_list_rules().returning(move || Ok(vec![scoped.clone()]));
        repo.expect_propose().withf(move |_, app, _| *app == a).times(1).returning(|_, _, _| Ok(true));

        let svc = RemediationService::new(Arc::new(repo));
        let apps = vec![
            (a, AppSignals { eol_count: 1, ..Default::default() }),
            (b, AppSignals { eol_count: 1, ..Default::default() }),
        ];
        assert_eq!(svc.evaluate(&apps).await.unwrap(), 1);
    }
}
