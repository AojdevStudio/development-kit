//! The `GET /me` endpoint: load the resolved principal for an authenticated
//! request (issue #27).
//!
//! This is the first real end-to-end path through the system — an authenticated
//! HTTP request in, a resolved identity out. The handler does three things and
//! nothing more: extract the bearer credential, resolve it to a principal via
//! the [`PrincipalStore`], and project that principal onto the wire response.
//! Authority lives in the resolution (ADR-0001); this module is the boundary.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::{resolve_principal, AuthError, PrincipalStore};
use crate::principal::{Principal, Role};

/// The shared application state carried by authenticated routes: the principal
/// store used to resolve callers. Held behind an `Arc<dyn …>` so the durable
/// Postgres-backed store can replace the in-memory one without touching the
/// handler.
#[derive(Clone)]
pub struct AuthState {
    pub store: Arc<dyn PrincipalStore>,
}

/// The `GET /me` response body: the resolved principal, projected into the
/// nested user/account/membership shape the desktop reads.
///
/// This is a server-owned response DTO. It deliberately nests so the desktop can
/// bind `user`, `account`, and `membership` directly, matching the data-model
/// vocabulary in `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeResponse {
    pub user: UserView,
    pub account: AccountView,
    pub membership: MembershipView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserView {
    pub id: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipView {
    pub role: Role,
}

impl From<Principal> for MeResponse {
    fn from(p: Principal) -> Self {
        MeResponse {
            user: UserView {
                id: p.user_id,
                email: p.email,
            },
            account: AccountView {
                id: p.account_id,
                name: p.account_name,
            },
            membership: MembershipView { role: p.role },
        }
    }
}

/// `GET /me` handler. Resolves the caller and returns the principal, or a 401 on
/// any auth failure. Both "no/!malformed credential" and "unknown token" map to
/// `401 Unauthorized`: the endpoint never reveals which one it was.
pub async fn get_me(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, StatusCode> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match resolve_principal(state.store.as_ref(), auth_header) {
        Ok(principal) => Ok(Json(MeResponse::from(principal))),
        Err(AuthError::MissingCredentials) | Err(AuthError::InvalidToken) => {
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal() -> Principal {
        Principal {
            user_id: "user_alice".into(),
            email: "alice@example.com".into(),
            account_id: "acct_acme".into(),
            account_name: "Acme".into(),
            role: Role::Owner,
        }
    }

    #[test]
    fn me_response_projects_principal_into_nested_views() {
        let response = MeResponse::from(principal());
        assert_eq!(response.user.id, "user_alice");
        assert_eq!(response.user.email, "alice@example.com");
        assert_eq!(response.account.id, "acct_acme");
        assert_eq!(response.account.name, "Acme");
        assert_eq!(response.membership.role, Role::Owner);
    }

    #[test]
    fn me_response_serializes_role_as_wire_string() {
        let json = serde_json::to_value(MeResponse::from(principal())).unwrap();
        assert_eq!(json["membership"]["role"], "owner");
    }

    #[test]
    fn me_response_round_trips_through_json() {
        let response = MeResponse::from(principal());
        let json = serde_json::to_string(&response).unwrap();
        let back: MeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, back);
    }
}
