//! Stable lock-key formatting, reused across the project so keys are uniform.

use uuid::Uuid;

/// The leader-election key for the job controller.
pub fn controller() -> String {
    "job-controller".to_string()
}

/// A per-job lock key.
pub fn job(job_id: Uuid) -> String {
    format!("job:{job_id}")
}

/// A per-repository lock key (serialises work on one repository).
pub fn repository(full_name: &str) -> String {
    format!("repo:{full_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_prefixed_and_stable() {
        assert_eq!(controller(), "job-controller");
        assert_eq!(repository("org/api"), "repo:org/api");
        let id = Uuid::nil();
        assert_eq!(job(id), format!("job:{id}"));
    }
}
