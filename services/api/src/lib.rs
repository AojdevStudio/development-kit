//! Cloud Rust/Axum backend — the SaaS authority.
//!
//! Exposes the public health route and the first authenticated authority
//! surface, `GET /me` (issue #27), which resolves who is calling and which
//! account they belong to, `POST /license/refresh` (issue #28), which mints
//! short-lived signed license tokens for offline paid access, and
//! `GET /me/entitlements` (issue #29), which runs the entitlement engine over an
//! account's billing state and returns the paid access the backend computed for
//! it, `POST /billing/*` (issue #31), which mints provider session URLs, and
//! `POST /webhooks/stripe` (issue #32), which ingests Stripe billing events and
//! idempotently reconciles account state so entitlement reflects billing changes.
//! The router is built as a pure value so it can be exercised in tests without
//! binding a socket.

#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod billing;
pub mod entitlement;
pub mod feature_gate;
pub mod license;
pub mod me;
pub mod me_entitlements;
pub mod principal;
pub mod product_module;
pub mod products;
pub mod store;
pub mod webhook;

use std::sync::Arc;

use axum::{routing::get, routing::post, Json, Router};
use serde_json::{json, Value};

use crate::auth::PrincipalStore;
use crate::billing::{BillingProvider, BillingState, MockBillingProvider};
use crate::entitlement::{AccountStateStore, InMemoryAccountStateStore};
use crate::feature_gate::FeatureGateState;
use crate::license::LicenseState;
use crate::me::{get_me, AuthState};
use crate::me_entitlements::EntitlementsState;
use crate::webhook::{
    MockWebhookVerifier, ProcessedEventStore, StripeWebhookVerifier, WebhookState, WebhookVerifier,
};

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
/// The backend authority boundary for paid actions is the AUTHENTICATED feature
/// gate (`/gated-feature/{feature}`, issue #30), merged below. The earlier
/// body-trusting `/gated/{feature}` route was removed (issue #57): it gated on an
/// `Entitlements` body the caller supplied, so it was a pure function over HTTP,
/// not an authority boundary, and must never earn backend coverage credit.
///
/// The billing routes (`/billing/checkout`, `/billing/portal`, issue #31) merge
/// here too, wired to the [`MockBillingProvider`] so the dev server and tests
/// exercise the full checkout/portal flow with NO Stripe key. The real provider
/// drops in behind the same trait via [`billing_app`] for production.
///
/// The authenticated feature gate (`/gated-feature/{feature}`, issue #30) merges
/// here too, resolving the caller's entitlement snapshot from their token so the
/// desktop can enforce a paid action on the server end-to-end (ADR-0001).
///
/// The Stripe webhook route (`/webhooks/stripe`, issue #32) merges here too,
/// wired to the [`MockWebhookVerifier`] so the dev server and tests exercise the
/// full ingest+reconcile path with NO Stripe webhook secret. It shares the *same*
/// account-state store as `/me/entitlements` (one [`InMemoryAccountStateStore`],
/// cloned), so an event the webhook reconciles is visible to the next
/// entitlements read — the read-after-write reconcile contract.
pub fn app() -> Router {
    // No live Stripe secret in the default/dev app: the deterministic mock
    // verifier exercises the full ingest path without one.
    app_with_webhook_verifier(Arc::new(MockWebhookVerifier::new()))
}

/// Build the application router with the REAL Stripe webhook verifier, keyed by
/// the live `whsec_…` signing secret (issue #58). This is what the binary mounts
/// whenever `STRIPE_WEBHOOK_SECRET` is set, so a configured production deploy
/// verifies real HMAC signatures and never trusts the mock's constant. Identical
/// to [`app`] in every other respect.
pub fn app_with_stripe_secret(secret: impl Into<String>) -> Router {
    app_with_webhook_verifier(Arc::new(StripeWebhookVerifier::new(secret)))
}

/// The shared app builder: every route except the webhook verifier is fixed; the
/// verifier is injected so [`app`] (mock, dev) and [`app_with_stripe_secret`]
/// (real, production) share one wiring and can never drift apart.
fn app_with_webhook_verifier(verifier: Arc<dyn WebhookVerifier>) -> Router {
    let accounts = InMemoryAccountStateStore::dev_seed();
    app_with_store(Arc::new(store::InMemoryPrincipalStore::dev_seed()))
        .merge(me_entitlements::router(dev_entitlements_state_with(
            accounts.clone(),
        )))
        .merge(billing::router(dev_billing_state()))
        .merge(feature_gate::authenticated_router(dev_feature_gate_state(
            accounts.clone(),
        )))
        .merge(products::notes::route::router(dev_notes_state(
            accounts.clone(),
        )))
        .merge(webhook::router(WebhookState {
            accounts: Arc::new(accounts),
            processed: Arc::new(ProcessedEventStore::new()),
            verifier,
        }))
}

/// The walking-skeleton state for the Notes sample product (issue #37): the dev
/// principal store plus the *shared* dev account-state store, so the Notes paid
/// gate (`POST /notes/publish`) resolves the dev token's real billing state
/// end-to-end. Sharing the one account store means a webhook-reconciled billing
/// change (issue #32) is reflected by the next Notes publish call — the same
/// read-after-write contract `/me/entitlements` and the spine gate honor. This is
/// the only `app()` wiring the Notes product adds; it is a single additive
/// `.merge(...)`, touching no baseline route (the seam's one rule).
fn dev_notes_state(accounts: InMemoryAccountStateStore) -> products::notes::route::NotesState {
    products::notes::route::NotesState {
        principals: Arc::new(store::InMemoryPrincipalStore::dev_seed()),
        accounts: Arc::new(accounts),
    }
}

/// Build the `/webhooks/stripe` router with caller-supplied stores, dedup store,
/// and verifier. Kept separate so integration tests drive the endpoint via
/// `tower::ServiceExt::oneshot` with a chosen verifier (mock for tests, real
/// Stripe in production) and an inspectable account store, without binding a
/// socket.
pub fn webhook_app(state: WebhookState) -> Router {
    webhook::router(state)
}

/// The walking-skeleton billing state: the dev principal store plus the
/// deterministic [`MockBillingProvider`], so the dev server serves real
/// checkout/portal URLs for the dev token end-to-end without a Stripe key.
fn dev_billing_state() -> BillingState {
    BillingState {
        principals: Arc::new(store::InMemoryPrincipalStore::dev_seed()),
        provider: Arc::new(MockBillingProvider::new()),
    }
}

/// Build the `/billing/*` router with caller-supplied stores and provider. Kept
/// separate so integration tests drive the endpoints via
/// `tower::ServiceExt::oneshot` with a known principal and a chosen provider
/// (mock for tests, real Stripe in production), without binding a socket.
pub fn billing_app(
    principals: Arc<dyn PrincipalStore>,
    provider: Arc<dyn BillingProvider>,
) -> Router {
    billing::router(BillingState {
        principals,
        provider,
    })
}

/// The walking-skeleton state for `GET /me/entitlements`: the dev principal store
/// (resolves [`store::DEV_TOKEN`]) plus a caller-supplied account-state store, so
/// the desktop dev build loads real paid entitlements end-to-end (issue #29). The
/// store is supplied (not constructed here) so the runnable app shares *one*
/// account store between `/me/entitlements` and the webhook reconciler (issue
/// #32) rather than each holding an isolated copy. The durable Postgres-backed
/// stores replace both behind the same traits.
fn dev_entitlements_state_with(accounts: InMemoryAccountStateStore) -> EntitlementsState {
    EntitlementsState {
        principals: Arc::new(store::InMemoryPrincipalStore::dev_seed()),
        accounts: Arc::new(accounts),
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

/// The walking-skeleton state for the authenticated feature gate
/// (`POST /gated-feature/{feature}`): the dev principal store plus a
/// caller-supplied account-state store, so the dev server enforces a paid action
/// for the dev token end-to-end (issue #30). The store is supplied (not
/// constructed here) so the runnable app shares *one* account store across
/// `/me/entitlements`, the webhook reconciler (issue #32), and this gate rather
/// than each holding an isolated copy — a webhook-reconciled billing change is
/// then reflected by the next gated-feature call. The durable Postgres-backed
/// stores replace both behind the same traits.
fn dev_feature_gate_state(accounts: InMemoryAccountStateStore) -> FeatureGateState {
    FeatureGateState {
        principals: Arc::new(store::InMemoryPrincipalStore::dev_seed()),
        accounts: Arc::new(accounts),
    }
}

/// Build the authenticated feature-gate router with caller-supplied stores. Kept
/// separate so integration tests drive `POST /gated-feature/{feature}` via
/// `tower::ServiceExt::oneshot` with a known principal and account state, without
/// binding a socket. The trait objects let a test inject any backing — an
/// entitled Pro account and an unentitled free one through the same router.
pub fn feature_gate_app(
    principals: Arc<dyn PrincipalStore>,
    accounts: Arc<dyn AccountStateStore>,
) -> Router {
    feature_gate::authenticated_router(FeatureGateState {
        principals,
        accounts,
    })
}

/// Build the router including the authority routes that need backend state —
/// the auth-backed `GET /me` (via the dev store) plus `POST /license/refresh`,
/// which authenticates the caller, resolves their entitlements server-side, and
/// signs a short-lived token with the backend key held in [`LicenseState`]
/// (issue #56). The route reads no account/plan from the request body.
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
