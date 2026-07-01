//! RBAC service (M37): resolves a principal's effective role, gates mutations
//! by team ownership, and (when multi-tenant is enabled) scopes the visible
//! application set to the caller's tenant.

use super::model::{Role, Team, TeamInput};
use super::repository::{RoleRepository, TeamRepository};
use crate::auth::Principal;
use crate::error::AppError;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct RbacService {
    roles: Arc<dyn RoleRepository>,
    teams: Arc<dyn TeamRepository>,
    /// The bootstrap admin username (always admin), if configured.
    bootstrap_admin: Option<String>,
    /// When true, non-admins only see applications owned by their tenant's teams.
    multitenant: bool,
}

impl RbacService {
    pub fn new(
        roles: Arc<dyn RoleRepository>,
        teams: Arc<dyn TeamRepository>,
        bootstrap_admin: Option<String>,
        multitenant: bool,
    ) -> Self {
        Self { roles, teams, bootstrap_admin, multitenant }
    }

    pub fn multitenant(&self) -> bool {
        self.multitenant
    }

    /// The effective role for a principal: an explicit assignment wins; the
    /// bootstrap admin is always admin; with no roles assigned yet everyone is
    /// admin (first-admin bootstrap / pre-RBAC behaviour); otherwise viewer.
    pub async fn role_for(&self, username: &str) -> Result<Role, AppError> {
        if let Some(role) = self.roles.get(username).await? {
            return Ok(role);
        }
        if self.bootstrap_admin.as_deref() == Some(username) {
            return Ok(Role::Admin);
        }
        if self.roles.count().await? == 0 {
            return Ok(Role::Admin);
        }
        Ok(Role::Viewer)
    }

    /// May this principal mutate the given application? Admins always may;
    /// maintainers may only for applications a team they belong to owns.
    pub async fn can_mutate_app(&self, principal: &Principal, application_id: Uuid) -> Result<bool, AppError> {
        let role = Role::highest(&principal.roles);
        if role >= Role::Admin {
            return Ok(true);
        }
        if role < Role::Maintainer {
            return Ok(false);
        }
        let owners: HashSet<Uuid> = self.teams.owner_team_ids(application_id).await?.into_iter().collect();
        let mine = self.teams.principal_team_ids(&principal.username).await?;
        Ok(mine.iter().any(|id| owners.contains(id)))
    }

    /// The application ids visible to a principal, or `None` when unscoped
    /// (multi-tenant off, or an admin). When scoped, returns apps owned by a
    /// team in any of the principal's tenants.
    pub async fn visible_app_ids(&self, principal: &Principal) -> Result<Option<HashSet<Uuid>>, AppError> {
        if !self.multitenant || Role::highest(&principal.roles) >= Role::Admin {
            return Ok(None);
        }
        let mut ids = HashSet::new();
        for tenant in self.teams.tenants_for_principal(&principal.username).await? {
            ids.extend(self.teams.app_ids_for_tenant(&tenant).await?);
        }
        Ok(Some(ids))
    }

    // --- team management (admin) ---

    pub async fn create_team(&self, input: TeamInput) -> Result<Team, AppError> {
        Ok(self.teams.create(input).await?)
    }

    pub async fn list_teams(&self) -> Result<Vec<Team>, AppError> {
        Ok(self.teams.list().await?)
    }

    pub async fn delete_team(&self, id: Uuid) -> Result<(), AppError> {
        Ok(self.teams.delete(id).await?)
    }

    pub async fn add_member(&self, team_id: Uuid, principal: &str) -> Result<(), AppError> {
        Ok(self.teams.add_member(team_id, principal).await?)
    }

    pub async fn set_owner(&self, team_id: Uuid, application_id: Uuid) -> Result<(), AppError> {
        Ok(self.teams.set_owner(team_id, application_id).await?)
    }

    pub async fn set_role(&self, principal: &str, role: Role) -> Result<(), AppError> {
        Ok(self.roles.set(principal, role).await?)
    }

    pub async fn list_roles(&self) -> Result<Vec<(String, Role)>, AppError> {
        Ok(self.roles.list().await?)
    }

    /// Owning team names for an application (shown on the app detail).
    pub async fn owner_team_names(&self, application_id: Uuid) -> Result<Vec<String>, AppError> {
        let owners: HashSet<Uuid> = self.teams.owner_team_ids(application_id).await?.into_iter().collect();
        Ok(self.teams.list().await?.into_iter().filter(|t| owners.contains(&t.id)).map(|t| t.name).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbac::repository::{MockRoleRepository, MockTeamRepository};

    fn principal(role: &str) -> Principal {
        Principal { username: "u".into(), display_name: "u".into(), roles: vec![role.into()] }
    }

    fn service(roles: MockRoleRepository, teams: MockTeamRepository, multitenant: bool) -> RbacService {
        RbacService::new(Arc::new(roles), Arc::new(teams), Some("root".into()), multitenant)
    }

    #[tokio::test]
    async fn role_for_bootstrap_and_default() {
        let mut roles = MockRoleRepository::new();
        roles.expect_get().returning(|_| Ok(None));
        roles.expect_count().returning(|| Ok(3)); // roles configured → defaults to viewer
        let svc = service(roles, MockTeamRepository::new(), false);
        assert_eq!(svc.role_for("root").await.unwrap(), Role::Admin); // bootstrap admin
        assert_eq!(svc.role_for("other").await.unwrap(), Role::Viewer);
    }

    #[tokio::test]
    async fn role_for_empty_table_is_admin() {
        let mut roles = MockRoleRepository::new();
        roles.expect_get().returning(|_| Ok(None));
        roles.expect_count().returning(|| Ok(0));
        let svc = service(roles, MockTeamRepository::new(), false);
        assert_eq!(svc.role_for("anyone").await.unwrap(), Role::Admin);
    }

    #[tokio::test]
    async fn admin_can_mutate_any_app() {
        let svc = service(MockRoleRepository::new(), MockTeamRepository::new(), false);
        assert!(svc.can_mutate_app(&principal("admin"), Uuid::new_v4()).await.unwrap());
    }

    #[tokio::test]
    async fn viewer_cannot_mutate() {
        let svc = service(MockRoleRepository::new(), MockTeamRepository::new(), false);
        assert!(!svc.can_mutate_app(&principal("viewer"), Uuid::new_v4()).await.unwrap());
    }

    #[tokio::test]
    async fn maintainer_can_mutate_only_owned_apps() {
        let app = Uuid::new_v4();
        let team = Uuid::new_v4();
        let mut owned_teams = MockTeamRepository::new();
        owned_teams.expect_owner_team_ids().returning(move |_| Ok(vec![team]));
        owned_teams.expect_principal_team_ids().returning(move |_| Ok(vec![team]));
        let svc = service(MockRoleRepository::new(), owned_teams, false);
        assert!(svc.can_mutate_app(&principal("maintainer"), app).await.unwrap());

        // A maintainer in a different team cannot.
        let mut other = MockTeamRepository::new();
        other.expect_owner_team_ids().returning(move |_| Ok(vec![team]));
        other.expect_principal_team_ids().returning(|_| Ok(vec![Uuid::new_v4()]));
        let svc2 = service(MockRoleRepository::new(), other, false);
        assert!(!svc2.can_mutate_app(&principal("maintainer"), app).await.unwrap());
    }

    #[tokio::test]
    async fn tenant_scope_limits_non_admins_when_enabled() {
        let app = Uuid::new_v4();
        let mut teams = MockTeamRepository::new();
        teams.expect_tenants_for_principal().returning(|_| Ok(vec!["acme".into()]));
        teams.expect_app_ids_for_tenant().returning(move |_| Ok(vec![app]));
        let svc = service(MockRoleRepository::new(), teams, true);
        let visible = svc.visible_app_ids(&principal("viewer")).await.unwrap().unwrap();
        assert!(visible.contains(&app) && visible.len() == 1);

        // Admins are unscoped; multitenant off is unscoped.
        assert!(svc.visible_app_ids(&principal("admin")).await.unwrap().is_none());
        let off = service(MockRoleRepository::new(), MockTeamRepository::new(), false);
        assert!(off.visible_app_ids(&principal("viewer")).await.unwrap().is_none());
    }
}
