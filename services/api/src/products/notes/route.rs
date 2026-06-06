//! The Notes product's backend routes (issue #37).
//!
//! Notes contributes its cloud routes through the seam's
//! [`BackendModule`](crate::product_module::BackendModule): [`NotesModule`]
//! returns the product [`meta`] and an `axum::Router`, and
//! `crate::product_module::mount` nests it under `/notes` additively.
//!
//! The one route, `POST /publish` (mounted as `/notes/publish`), is the PAID
//! capability. Authority boundary (ADR-0001): it resolves the caller's Notes
//! product entitlements SERVER-SIDE from their bearer token — auth → account
//! state → the Notes policy ([`resolve_product_entitlements`]) — and gates against
//! *that*, exactly the way the baseline authenticated gate
//! (`/gated-feature/{feature}`) resolves `Entitlements`. The request body is never
//! read for the authority decision, so a forged/lying body can never grant
//! `notes.publish_note`. This replaces the seam example's body-driven scaffold
//! (`tests/product_module.rs`) with real server-side resolution, which is the
//! capstone proof issue #37 requires.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use shared::ProductModuleMeta;

use crate::auth::{resolve_principal, AuthError, PrincipalStore};
use crate::entitlement::AccountStateStore;
use crate::feature_gate::require_product_feature;
use crate::product_module::BackendModule;
use crate::products::notes::entitlement::{publish_note_key, resolve_product_entitlements};
use crate::products::notes::meta;

/// Shared state for the Notes authenticated routes: the principal store (who is
/// calling) and the account-state store (their billing state). Both behind
/// `Arc<dyn …>` so the durable Postgres-backed stores drop in unchanged — the
/// exact seam the spine's [`crate::feature_gate::FeatureGateState`] uses.
#[derive(Clone)]
pub struct NotesState {
    pub principals: Arc<dyn PrincipalStore>,
    pub accounts: Arc<dyn AccountStateStore>,
}

/// The Notes [`BackendModule`]: its identity and its (un-prefixed) routes.
///
/// Carries the [`NotesState`] so its routes are authenticated. `mount(&module)`
/// nests `router()` under `/notes` additively; the module touches no baseline
/// route. Held as a value (not a unit struct) because the routes need state.
pub struct NotesModule {
    state: NotesState,
}

impl NotesModule {
    /// Build the Notes module over the given authenticated state.
    pub fn new(state: NotesState) -> Self {
        Self { state }
    }
}

impl BackendModule for NotesModule {
    fn meta(&self) -> ProductModuleMeta {
        meta()
    }

    fn router(&self) -> Router {
        // The route is returned as a `Router<()>` (state already applied) so it
        // composes cleanly when `mount` nests it under the namespace prefix.
        Router::new()
            .route("/publish", post(publish_note))
            .with_state(self.state.clone())
    }
}

/// `POST /notes/publish` — the Notes paid action, gated SERVER-SIDE.
///
/// Resolves the caller's Notes product entitlements from their bearer token (auth
/// → account state → the Notes policy), then gates `notes.publish_note`. Returns
/// 200 + a JSON acknowledgement when the resolved snapshot grants it, 403 when it
/// does not, 401 on any auth failure. The request body is intentionally NOT read
/// for the authority decision — the verdict is the backend's, computed from the
/// account's real billing state, so a forged body can never grant access
/// (ADR-0001).
async fn publish_note(
    State(state): State<NotesState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
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
        // Authenticated, but no billing state on record: deny-by-default.
        .ok_or(StatusCode::FORBIDDEN)?;

    // Server-side resolution: the product snapshot comes from the account's real
    // billing state, NOT from the request body.
    let snapshot = resolve_product_entitlements(principal.account_id, &account_state, now_unix());
    require_product_feature(&snapshot, &publish_note_key())?;
    // The real product would persist the published note's cloud record here; the
    // sample returns a deterministic payload so the allow path is observable.
    Ok(Json(json!({ "published": true })))
}

/// Build the Notes backend router over `state`, mounted under `/notes`.
///
/// The one entrypoint `crate::app()` calls to plug Notes in: it constructs the
/// [`NotesModule`] and nests it additively, so adding Notes is a single
/// `.merge(notes::route::router(state))` with no edit to any baseline route.
pub fn router(state: NotesState) -> Router {
    crate::product_module::mount(&NotesModule::new(state))
}

/// The current wall-clock instant in unix epoch seconds, used as the policy's
/// `now`. The policy takes `now` as a parameter (pure/testable); only this
/// boundary reads the clock.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
