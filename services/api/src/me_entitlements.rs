//! The `GET /me/entitlements` endpoint: resolve the caller, look up their
//! account's billing state, and return the entitlements the backend computed for
//! them (issue #29).
//!
//! This is the authoritative word on paid access (ADR-0001): the desktop calls
//! it and reads the answer, never deriving entitlements locally. The handler is
//! a thin boundary — extract the bearer credential, resolve the [`Principal`],
//! look up the account's [`AccountState`], run the pure [`resolve_entitlements`]
//! engine, and project the result onto [`EntitlementsResponse`]. All the policy
//! lives in the engine; all the identity logic lives in `auth`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};

use shared::EntitlementsResponse;

use crate::auth::{resolve_principal, AuthError, PrincipalStore};
use crate::entitlement::{resolve_entitlements, AccountStateStore};

/// Shared state for the entitlements route: the principal store (who is calling)
/// and the account-state store (what their account is entitled to). Both behind
/// `Arc<dyn …>` so the durable Postgres-backed stores drop in without touching
/// the handler.
#[derive(Clone)]
pub struct EntitlementsState {
    pub principals: Arc<dyn PrincipalStore>,
    pub accounts: Arc<dyn AccountStateStore>,
}

/// Routes for `GET /me/entitlements`, carrying their own [`EntitlementsState`].
///
/// Returned as a `Router<()>` (state already applied) so it merges cleanly into
/// the app router alongside the other stateful routes, none of which it touches.
pub fn router(state: EntitlementsState) -> Router {
    Router::new()
        .route("/me/entitlements", get(get_me_entitlements))
        .with_state(state)
}

/// `GET /me/entitlements` handler.
///
/// Returns 200 with the account's [`EntitlementsResponse`] for an authenticated
/// caller whose account has billing state on record; 401 on any auth failure
/// (missing/malformed credential or unknown token, indistinguishable on the
/// wire); 404 when the authenticated account has no billing state yet.
pub async fn get_me_entitlements(
    State(state): State<EntitlementsState>,
    headers: HeaderMap,
) -> Result<Json<EntitlementsResponse>, StatusCode> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let principal = match resolve_principal(state.principals.as_ref(), auth_header) {
        Ok(p) => p,
        Err(AuthError::MissingCredentials) | Err(AuthError::InvalidToken) => {
            return Err(StatusCode::UNAUTHORIZED)
        }
    };

    let account_state = state
        .accounts
        .account_state(&principal.account_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let entitlements = resolve_entitlements(principal.account_id, &account_state, now_unix());
    Ok(Json(EntitlementsResponse { entitlements }))
}

/// The current wall-clock instant in unix epoch seconds, used as the engine's
/// `now`. The engine itself takes `now` as a parameter (pure/testable); only
/// this boundary reads the clock.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
