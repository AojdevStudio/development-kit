//! Cloud Rust/Axum backend — the SaaS authority.
//!
//! Exposes the public health route and the first authenticated authority
//! surface, `GET /me` (issue #27), which resolves who is calling and which
//! account they belong to. The remaining surfaces declared in
//! `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md` (`/me/entitlements`, `/billing/*`,
//! `/stripe/webhook`, `/license/*`) land in their own issues. The router is
//! built as a pure value so it can be exercised in tests without binding a
//! socket.

#![forbid(unsafe_code)]

pub mod auth;
pub mod me;
pub mod principal;
pub mod store;

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use crate::auth::PrincipalStore;
use crate::me::{get_me, AuthState};

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
pub fn app() -> Router {
    app_with_store(Arc::new(store::InMemoryPrincipalStore::dev_seed()))
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
