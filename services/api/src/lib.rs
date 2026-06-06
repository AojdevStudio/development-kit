//! Cloud Rust/Axum backend — the SaaS authority.
//!
//! Exposes the public health route and the first authenticated authority
//! surface, `GET /me` (issue #27), which resolves who is calling and which
//! account they belong to, `POST /license/refresh` (issue #28), which mints
//! short-lived signed license tokens for offline paid access, and
//! `GET /me/entitlements` (issue #29), which runs the entitlement engine over an
//! account's billing state and returns the paid access the backend computed for
//! it. The remaining surfaces declared in `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`
//! (`/billing/*`, `/stripe/webhook`) land in their own issues. The router is
//! built as a pure value so it can be exercised in tests without binding a
//! socket.

#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod entitlement;
pub mod feature_gate;
pub mod license;
pub mod me;
pub mod me_entitlements;
pub mod principal;
pub mod store;

use std::sync::Arc;

use axum::{routing::get, routing::post, Json, Router};
use serde_json::{json, Value};

use crate::auth::PrincipalStore;
use crate::entitlement::{AccountStateStore, InMemoryAccountStateStore};
use crate::license::LicenseState;
use crate::me::{get_me, AuthState};
use crate::me_entitlements::EntitlementsState;

/// Build the application router with a caller-supplied principal store. Kept
/// separate from `serve` so tests can drive it via `tower::ServiceExt::oneshot`
/// without a network listener, and parameterized by the store so the durable
/// backing can replace the in-memory one without reshaping the router.
pub fn app_with_store(store: Arc<dyn PrincipalStore>) -> Router {
    let state = AuthState { store };
    Router::new()
        .route("/health", get(health))
        .route("/me", get(get_me))
        .with_state(state)
}

/// Build the application router with the walking-skeleton dev store. This is the
/// entrypoint the binary uses; its store resolves [`store::DEV_TOKEN`] so the
/// desktop dev build can load the current account end-to-end (issue #27).
///
/// The backend feature gate (`/gated/{feature}`, issue #25) is merged here so
/// the platform spine ships a real, exercised authority boundary the feature-key
/// coverage gate can count. It carries no backend state, so it merges after the
/// auth state is applied, leaving both routers as `Router<()>`.
pub fn app() -> Router {
    app_with_store(Arc::new(store::InMemoryPrincipalStore::dev_seed()))
        .merge(feature_gate::router())
        .merge(me_entitlements::router(dev_entitlements_state()))
}

/// The walking-skeleton state for `GET /me/entitlements`: the dev principal store
/// (resolves [`store::DEV_TOKEN`]) plus the dev account-state store (seeds the dev
/// account to an active Pro subscription), so the desktop dev build loads real
/// paid entitlements end-to-end (issue #29). The durable Postgres-backed stores
/// replace both behind the same traits.
fn dev_entitlements_state() -> EntitlementsState {
    EntitlementsState {
        principals: Arc::new(store::InMemoryPrincipalStore::dev_seed()),
        accounts: Arc::new(InMemoryAccountStateStore::dev_seed()),
    }
}

/// Build the `GET /me/entitlements` router with caller-supplied stores. Kept
/// separate so integration tests can drive the endpoint via
/// `tower::ServiceExt::oneshot` with a known principal and account state, without
/// binding a socket. The trait objects let a test inject any backing.
pub fn entitlements_app(
    principals: Arc<dyn PrincipalStore>,
    accounts: Arc<dyn AccountStateStore>,
) -> Router {
    me_entitlements::router(EntitlementsState {
        principals,
        accounts,
    })
}

/// Build the router including the authority routes that need backend state —
/// the auth-backed `GET /me` (via the dev store) plus `POST /license/refresh`,
/// which signs short-lived tokens with the backend key held in [`LicenseState`].
///
/// `/license/refresh` carries its own [`LicenseState`]; `/me` keeps the auth
/// state applied in [`app`]. Both routes are mounted — neither is dropped.
pub fn app_with_license(license: LicenseState) -> Router {
    app().route(
        "/license/refresh",
        post(license::refresh).with_state(license),
    )
}

/// Liveness probe. Returns 200 with a small JSON body. No auth — this is the
/// one public unauthenticated, non-webhook route.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Test fixtures shared across integration tests. Compiled into the library so
/// the `tests/` binaries can build a router seeded with a known principal
/// without duplicating the seed shape. The seed is the same dev seed the
/// runnable binary uses, so the tests exercise the real default backing.
pub mod test_support {
    use std::sync::Arc;

    use axum::Router;

    use crate::principal::Principal;
    use crate::store::{self, InMemoryPrincipalStore};

    /// The bearer token wired to the seeded principal below.
    pub const SEEDED_TOKEN: &str = store::DEV_TOKEN;

    /// The principal the seeded store resolves [`SEEDED_TOKEN`] to.
    pub fn seeded_principal() -> Principal {
        store::dev_principal()
    }

    /// A router whose store resolves [`SEEDED_TOKEN`] to [`seeded_principal`].
    pub fn app_with_seeded_store() -> Router {
        crate::app_with_store(Arc::new(InMemoryPrincipalStore::dev_seed()))
    }
}
