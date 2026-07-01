//! `require_role` middleware (M37): gates a route on the caller's effective
//! role. Runs after `require_auth`, which sets the `Principal` extension; the
//! role is read from the session principal (enriched at login).

use super::model::Role;
use crate::auth::Principal;
use axum::Extension;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Allow the request only if the principal's highest role meets `min`.
pub async fn role_guard(
    State(min): State<Role>,
    principal: Option<Extension<Principal>>,
    request: Request,
    next: Next,
) -> Response {
    let allowed = principal.map(|Extension(p)| Role::highest(&p.roles) >= min).unwrap_or(false);
    if allowed {
        next.run(request).await
    } else {
        (StatusCode::FORBIDDEN, "insufficient role").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_meets_minimum() {
        assert!(Role::highest(&["admin".into()]) >= Role::Maintainer);
        assert!(Role::highest(&["maintainer".into()]) >= Role::Maintainer);
        assert!(Role::highest(&["viewer".into()]) < Role::Maintainer);
    }
}
