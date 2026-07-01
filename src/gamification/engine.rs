//! Pure gamification rules (M44): map an operator action to XP + a skill, the
//! XP→level curve, skill aggregation, and badge conditions. No I/O.

use serde::Serialize;

/// XP + an optional skill tag awarded for one operator action (audit action key,
/// M36). Actions not listed award nothing (e.g. `login`).
pub fn award_for(action: &str) -> Option<(i32, Option<&'static str>)> {
    match action {
        "prompt.update" | "prompt.reset" => Some((5, Some("config"))),
        "team.create" => Some((10, Some("platform"))),
        "role.set" => Some((5, Some("platform"))),
        "agent_task.create" => Some((20, Some("automation"))),
        "campaign.create" => Some((30, Some("automation"))),
        "budget.create" => Some((5, Some("cost"))),
        _ => None,
    }
}

/// XP per level (linear curve).
const XP_PER_LEVEL: i64 = 100;

/// Level + progress within the current level.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LevelInfo {
    pub level: i64,
    pub into_level: i64,
    pub to_next: i64,
}

/// Level (1-based) and progress for a cumulative XP total.
pub fn level_for(total_xp: i64) -> LevelInfo {
    let xp = total_xp.max(0);
    let into = xp % XP_PER_LEVEL;
    LevelInfo { level: xp / XP_PER_LEVEL + 1, into_level: into, to_next: XP_PER_LEVEL - into }
}

/// Badges earned for the given lifetime stats.
pub fn badges_earned(total_xp: i64, award_count: i64, distinct_skills: usize) -> Vec<&'static str> {
    let mut out = Vec::new();
    if award_count >= 1 {
        out.push("first_action");
    }
    if award_count >= 10 {
        out.push("contributor");
    }
    if total_xp >= 100 {
        out.push("centurion");
    }
    if total_xp >= 500 {
        out.push("platform_hero");
    }
    if distinct_skills >= 3 {
        out.push("polyglot");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awards_map_actions_to_points_and_skill() {
        assert_eq!(award_for("campaign.create"), Some((30, Some("automation"))));
        assert_eq!(award_for("team.create"), Some((10, Some("platform"))));
        assert_eq!(award_for("login"), None); // not awarded
    }

    #[test]
    fn level_curve() {
        assert_eq!(level_for(0), LevelInfo { level: 1, into_level: 0, to_next: 100 });
        assert_eq!(level_for(150), LevelInfo { level: 2, into_level: 50, to_next: 50 });
        assert_eq!(level_for(-5).level, 1); // clamps negative
    }

    #[test]
    fn badges_thresholds() {
        assert!(badges_earned(0, 0, 0).is_empty());
        assert_eq!(badges_earned(10, 1, 1), vec!["first_action"]);
        let many = badges_earned(600, 12, 3);
        assert!(many.contains(&"contributor") && many.contains(&"centurion"));
        assert!(many.contains(&"platform_hero") && many.contains(&"polyglot"));
    }
}
