//! Operator gamification (M44): XP, levels, skills and badges derived from the
//! audit log + change feed (M36) — no new tracking.

pub mod engine;
pub mod job;
pub mod repository;
pub mod service;

pub use engine::{LevelInfo, award_for, badges_earned, level_for};
pub use job::{GamificationJob, JOB_TYPE, ensure_job};
pub use repository::{
    ActorTotal, GamificationRepository, PgGamificationRepository, SqliteGamificationRepository,
    XpAward, XpAwardInput,
};
pub use service::{GamificationService, Profile};
