//! Pure DORA computation (M47): derive the four DORA measures + a performance
//! tier from a fixed set of deployment / incident events. No I/O — fully unit
//! tested.

use super::model::{Deployment, DoraSummary, Incident};
use std::collections::HashSet;

/// Tier rank for compositing (higher is better).
fn rank(tier: &str) -> u8 {
    match tier {
        "elite" => 3,
        "high" => 2,
        "medium" => 1,
        _ => 0,
    }
}

fn tier_name(rank: u8) -> &'static str {
    match rank {
        3 => "elite",
        2 => "high",
        1 => "medium",
        _ => "low",
    }
}

/// Deployment-frequency tier from deploys per week.
fn frequency_tier(weekly: f64) -> &'static str {
    if weekly >= 7.0 {
        "elite" // multiple per day
    } else if weekly >= 1.0 {
        "high" // weekly
    } else if weekly >= 0.23 {
        "medium" // ~monthly
    } else {
        "low"
    }
}

/// Lead-time tier from median hours.
fn lead_time_tier(hours: f64) -> &'static str {
    if hours < 24.0 {
        "elite" // < 1 day
    } else if hours < 168.0 {
        "high" // < 1 week
    } else if hours < 730.0 {
        "medium" // < 1 month
    } else {
        "low"
    }
}

/// Change-failure-rate tier from a fraction (0..1).
fn cfr_tier(rate: f64) -> &'static str {
    if rate <= 0.05 {
        "elite"
    } else if rate <= 0.10 {
        "high"
    } else if rate <= 0.15 {
        "medium"
    } else {
        "low"
    }
}

/// MTTR tier from median hours.
fn mttr_tier(hours: f64) -> &'static str {
    if hours < 1.0 {
        "elite" // < 1 hour
    } else if hours < 24.0 {
        "high" // < 1 day
    } else if hours < 168.0 {
        "medium" // < 1 week
    } else {
        "low"
    }
}

/// Median of a value set (`None` when empty).
fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        Some((values[mid - 1] + values[mid]) / 2.0)
    } else {
        Some(values[mid])
    }
}

const HOUR: f64 = 3600.0;

/// Compute the DORA summary over an already-windowed set of events.
pub fn compute(deployments: &[Deployment], incidents: &[Incident], window_days: i64) -> DoraSummary {
    let weeks = (window_days.max(1) as f64) / 7.0;
    let successful = deployments.iter().filter(|d| d.succeeded).count();
    let deploy_frequency_weekly = successful as f64 / weeks;

    let lead_times: Vec<f64> = deployments
        .iter()
        .filter_map(|d| d.first_commit_at.map(|fc| (d.deployed_at - fc).num_seconds() as f64 / HOUR))
        .filter(|h| *h >= 0.0)
        .collect();
    let lead_time_hours = median(lead_times);

    let failed_deploys: HashSet<_> = incidents.iter().filter_map(|i| i.caused_by).collect();
    let change_failure_rate = if deployments.is_empty() {
        0.0
    } else {
        failed_deploys.len() as f64 / deployments.len() as f64
    };

    let restore_times: Vec<f64> = incidents
        .iter()
        .filter_map(|i| i.resolved_at.map(|r| (r - i.opened_at).num_seconds() as f64 / HOUR))
        .filter(|h| *h >= 0.0)
        .collect();
    let mttr_hours = median(restore_times);

    let tier = overall_tier(deploy_frequency_weekly, lead_time_hours, change_failure_rate, mttr_hours, deployments.len());

    DoraSummary {
        deploy_frequency_weekly,
        lead_time_hours,
        change_failure_rate,
        mttr_hours,
        tier,
        deployments: deployments.len(),
        incidents: incidents.len(),
    }
}

/// Composite tier — the worst (minimum) sub-tier across the measures that have
/// evidence. Deployment frequency always counts; CFR only when deployments
/// exist; lead time / MTTR only when their event sets are non-empty.
fn overall_tier(
    weekly: f64,
    lead_time: Option<f64>,
    cfr: f64,
    mttr: Option<f64>,
    deployments: usize,
) -> String {
    let mut worst = rank(frequency_tier(weekly));
    if deployments > 0 {
        worst = worst.min(rank(cfr_tier(cfr)));
    }
    if let Some(h) = lead_time {
        worst = worst.min(rank(lead_time_tier(h)));
    }
    if let Some(h) = mttr {
        worst = worst.min(rank(mttr_tier(h)));
    }
    tier_name(worst).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn deploy(days_ago_deployed: i64, lead_hours: Option<i64>, succeeded: bool) -> Deployment {
        let deployed_at = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap() - chrono::Duration::days(days_ago_deployed);
        Deployment {
            id: Uuid::new_v4(),
            application_id: None,
            environment: "production".into(),
            sha: None,
            succeeded,
            deployed_at,
            first_commit_at: lead_hours.map(|h| deployed_at - chrono::Duration::hours(h)),
        }
    }

    fn incident(caused_by: Option<Uuid>, restore_hours: Option<i64>) -> Incident {
        let opened_at = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        Incident {
            id: Uuid::new_v4(),
            application_id: None,
            caused_by,
            opened_at,
            resolved_at: restore_hours.map(|h| opened_at + chrono::Duration::hours(h)),
        }
    }

    #[test]
    fn empty_history_is_safe_and_low() {
        let s = compute(&[], &[], 30);
        assert_eq!(s.deploy_frequency_weekly, 0.0);
        assert_eq!(s.lead_time_hours, None);
        assert_eq!(s.change_failure_rate, 0.0);
        assert_eq!(s.mttr_hours, None);
        assert_eq!(s.tier, "low");
    }

    #[test]
    fn computes_frequency_lead_time_and_mttr() {
        // 14 successful deploys over 14 days = 1/day = 7/week (elite frequency),
        // lead times {2,4,6}h median 4h (elite), no incidents.
        let mut deploys: Vec<Deployment> = (0..14).map(|d| deploy(d, None, true)).collect();
        deploys[0].first_commit_at = Some(deploys[0].deployed_at - chrono::Duration::hours(2));
        deploys[1].first_commit_at = Some(deploys[1].deployed_at - chrono::Duration::hours(4));
        deploys[2].first_commit_at = Some(deploys[2].deployed_at - chrono::Duration::hours(6));
        let s = compute(&deploys, &[], 14);
        assert!((s.deploy_frequency_weekly - 7.0).abs() < 1e-9);
        assert_eq!(s.lead_time_hours, Some(4.0));
        assert_eq!(s.tier, "elite");
    }

    #[test]
    fn change_failure_rate_links_incident_to_deploy() {
        let deploys = vec![deploy(1, Some(1), true), deploy(2, Some(1), true), deploy(3, Some(1), true), deploy(4, Some(1), true)];
        // One incident caused by the first deploy → 1/4 = 25% CFR (low).
        let incidents = vec![incident(Some(deploys[0].id), Some(2))];
        let s = compute(&deploys, &incidents, 7);
        assert!((s.change_failure_rate - 0.25).abs() < 1e-9);
        assert_eq!(s.mttr_hours, Some(2.0));
        // 4 deploys/week = high frequency, but 25% CFR is low → composite low.
        assert_eq!(s.tier, "low");
    }

    #[test]
    fn even_count_median_averages_middle_pair() {
        assert_eq!(median(vec![1.0, 3.0, 5.0, 7.0]), Some(4.0));
        assert_eq!(median(vec![5.0]), Some(5.0));
        assert_eq!(median(vec![]), None);
    }

    #[test]
    fn tier_bands_map_correctly() {
        assert_eq!(frequency_tier(10.0), "elite");
        assert_eq!(frequency_tier(2.0), "high");
        assert_eq!(frequency_tier(0.3), "medium");
        assert_eq!(frequency_tier(0.1), "low");
        assert_eq!(lead_time_tier(10.0), "elite");
        assert_eq!(lead_time_tier(100.0), "high");
        assert_eq!(lead_time_tier(500.0), "medium");
        assert_eq!(lead_time_tier(1000.0), "low");
        assert_eq!(cfr_tier(0.0), "elite");
        assert_eq!(cfr_tier(0.5), "low");
        assert_eq!(mttr_tier(0.5), "elite");
        assert_eq!(mttr_tier(200.0), "low");
    }
}
