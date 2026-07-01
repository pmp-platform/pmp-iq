//! Roles, teams & (optional) multi-tenant access control (M37).

pub mod middleware;
pub mod model;
pub mod repository;
pub mod service;

pub use middleware::role_guard;
pub use model::{Role, Team, TeamInput};
pub use repository::{
    PgRoleRepository, PgTeamRepository, RoleRepository, SqliteRoleRepository, SqliteTeamRepository,
    TeamRepository,
};
pub use service::RbacService;
