//! Cloud Rust/Axum backend — the SaaS authority.
//!
//! In the walking skeleton this exposes only a health route. The authority
//! surfaces declared in `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`
//! (`/me`, `/me/entitlements`, `/billing/*`, `/stripe/webhook`, `/license/*`)
//! land in their own issues. The router is built here as a pure value so it can
//! be exercised in tests without binding a socket.

#![forbid(unsafe_code)]

pub mod license;

use axum::{routing::get, routing::post, Json, Router};
use serde_json::{json, Value};

use crate::license::LicenseState;

/// Build the application router. Kept separate from `serve` so tests can drive
/// it via `tower::ServiceExt::oneshot` without a network listener.
pub fn app() -> Router {
    Router::new().route("/health", get(health))
}

/// Build the router including the authority routes that need backend state —
/// currently `POST /license/refresh`, which signs short-lived tokens with the
/// backend key held in [`LicenseState`].
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
