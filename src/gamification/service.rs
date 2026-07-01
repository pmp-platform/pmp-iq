//! Gamification orchestration (M44): replay recorded operator actions into XP +
//! badges (idempotent), and read profiles + the leaderboard.

use super::engine::{LevelInfo, award_for, badges_earned, level_for};
use super::repository::{ActorTotal, GamificationRepository, XpAward, XpAwardInput};
use crate::audit::AuditRepository;
use crate::error::AppError;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

/// How many recent audit events a replay scans.
const REPLAY_LIMIT: i64 = 10_000;

/// One operator's profile.
#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub actor: String,
    pub total_xp: i64,
    pub level: LevelInfo,
    pub skills: Vec<SkillXp>,
    pub badges: Vec<String>,
    pub recent: Vec<XpAward>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillXp {
    pub skill: String,
    pub points: i64,
}

#[derive(Clone)]
pub struct GamificationService {
    audit: Arc<dyn AuditRepository>,
    repo: Arc<dyn GamificationRepository>,
}

impl GamificationService {
    pub fn new(audit: Arc<dyn AuditRepository>, repo: Arc<dyn GamificationRepository>) -> Self {
        Self { audit, repo }
    }

    /// Replay recorded actions into XP awards (idempotent by source event) and
    /// recompute badges. Returns the number of newly-awarded actions.
    pub async fn replay(&self) -> Result<usize, AppError> {
        let events = self.audit.list(REPLAY_LIMIT).await?;
        let mut new = 0usize;
        let mut actors: HashSet<String> = HashSet::new();
        for event in events {
            let Some((points, skill)) = award_for(&event.action) else { continue };
            let inserted = self
                .repo
                .record(XpAwardInput {
                    actor: event.actor.clone(),
                    reason: event.action,
                    points,
                    skill: skill.map(String::from),
                    source: Some(event.id.to_string()),
                })
                .await?;
            if inserted {
                new += 1;
            }
            actors.insert(event.actor);
        }
        for actor in actors {
            self.recompute_badges(&actor).await?;
        }
        Ok(new)
    }

    async fn recompute_badges(&self, actor: &str) -> Result<(), AppError> {
        let awards = self.repo.for_actor(actor).await?;
        let total: i64 = awards.iter().map(|a| a.points as i64).sum();
        let skills = self.repo.skills_for(actor).await?;
        for badge in badges_earned(total, awards.len() as i64, skills.len()) {
            self.repo.set_badge(actor, badge).await?;
        }
        Ok(())
    }

    pub async fn profile(&self, actor: &str) -> Result<Profile, AppError> {
        let recent = self.repo.for_actor(actor).await?;
        let total: i64 = recent.iter().map(|a| a.points as i64).sum();
        let skills = self
            .repo
            .skills_for(actor)
            .await?
            .into_iter()
            .map(|(skill, points)| SkillXp { skill, points })
            .collect();
        Ok(Profile {
            actor: actor.to_string(),
            total_xp: total,
            level: level_for(total),
            skills,
            badges: self.repo.badges_for(actor).await?,
            recent: recent.into_iter().take(20).collect(),
        })
    }

    pub async fn leaderboard(&self) -> Result<Vec<ActorTotal>, AppError> {
        Ok(self.repo.totals().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{AuditEvent, MockAuditRepository};
    use crate::gamification::repository::MockGamificationRepository;
    use chrono::Utc;
    use uuid::Uuid;

    fn event(action: &str, actor: &str) -> AuditEvent {
        AuditEvent {
            id: Uuid::new_v4(),
            actor: actor.into(),
            action: action.into(),
            target: None,
            metadata: None,
            occurred_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn replay_awards_recognised_actions_only_and_sets_badges() {
        let mut audit = MockAuditRepository::new();
        audit.expect_list().returning(|_| {
            Ok(vec![event("campaign.create", "alice"), event("login", "alice")])
        });

        let mut repo = MockGamificationRepository::new();
        // Only campaign.create (30 pts) is awarded; login is ignored.
        repo.expect_record()
            .withf(|a| a.reason == "campaign.create" && a.points == 30 && a.actor == "alice")
            .times(1)
            .returning(|_| Ok(true));
        // Badge recompute reads the actor's awards/skills and sets badges.
        repo.expect_for_actor().returning(|_| {
            Ok(vec![XpAward { reason: "campaign.create".into(), points: 30, skill: Some("automation".into()), awarded_at: Utc::now() }])
        });
        repo.expect_skills_for().returning(|_| Ok(vec![("automation".to_string(), 30)]));
        repo.expect_set_badge().withf(|a, b| a == "alice" && b == "first_action").times(1).returning(|_, _| Ok(()));

        let svc = GamificationService::new(Arc::new(audit), Arc::new(repo));
        assert_eq!(svc.replay().await.unwrap(), 1);
    }
}
