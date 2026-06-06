//! Billing sessions: `POST /billing/checkout` and `POST /billing/portal`
//! (issue #31).
//!
//! The backend is the sole billing authority (ADR-0001): the desktop never talks
//! to Stripe, never holds a Stripe key, and never constructs a session URL. It
//! asks the backend, which mints a provider session and returns the URL the
//! desktop opens in the system browser.
//!
//! The Stripe-ness lives entirely behind the [`BillingProvider`] trait. Routes
//! depend on the trait, never on Stripe types, so:
//!
//! - [`MockBillingProvider`] returns deterministic URLs with no network and no
//!   clock/RNG — dev and CI exercise the full flow with NO Stripe key (issue #31
//!   "mock billing mode works without a live provider").
//! - [`StripeBillingProvider`] is the real seam behind the *same* trait. It is a
//!   compile-only stub today: it names where the live Stripe Checkout / Customer
//!   Portal calls go, without pulling `async-stripe` into the build (which would
//!   trip the ADR-0002 desktop bans transitively and require a key to gate).
//!   Wiring the live crate is a follow-up issue; swapping it in changes one
//!   constructor line because the route only sees `dyn BillingProvider`.
//!
//! Account identity is taken from the authenticated [`Principal`], never the
//! request body — a client cannot start checkout against an account it does not
//! own.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};

use shared::{CheckoutSessionRequest, CheckoutSessionResponse, PlanTier, PortalSessionResponse};

use crate::auth::{resolve_principal, AuthError, PrincipalStore};

/// Why a billing-provider call could not produce a session.
///
/// The mock never fails; the real Stripe provider can (network, config). Kept as
/// a typed error so the handler maps provider failure onto a clean HTTP status
/// instead of leaking provider internals to the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillingError {
    /// The provider is not configured to mint sessions (e.g. the real Stripe
    /// provider with no key wired yet). The route maps this to `503`.
    NotConfigured,
}

/// The billing authority seam. A small interface over a deep implementation
/// (deep-module spirit): callers ask one of two questions — "give me a checkout
/// URL for this account+plan" or "give me a portal URL for this account" — and
/// never reason about Stripe. The mock and the real Stripe provider are
/// interchangeable behind this trait (DIP).
pub trait BillingProvider: Send + Sync {
    /// Create a checkout session for `account_id` upgrading to `plan`, returning
    /// the URL the desktop opens. `account_id` is the *authenticated* account,
    /// resolved server-side — never client-supplied.
    fn create_checkout_session(
        &self,
        account_id: &str,
        plan: PlanTier,
    ) -> Result<CheckoutSessionResponse, BillingError>;

    /// Create a customer-portal session for `account_id`, returning the URL the
    /// desktop opens for self-service billing management.
    fn create_portal_session(
        &self,
        account_id: &str,
    ) -> Result<PortalSessionResponse, BillingError>;
}

/// Deterministic, network-free billing provider for dev and tests.
///
/// Returns Stripe-shaped URLs (`https://checkout.stripe.com/...`,
/// `https://billing.stripe.com/...`) so the desktop's URL-open path is identical
/// whether the URL came from the mock or real Stripe — only the session id
/// (`mock_<account>`) differs. Pure over its inputs: same account+plan always
/// yields the same URL, with no clock, RNG, or I/O, so tests are reproducible.
#[derive(Debug, Clone, Default)]
pub struct MockBillingProvider;

impl MockBillingProvider {
    /// Construct the mock provider. No configuration, no key — that is the point.
    pub fn new() -> Self {
        Self
    }
}

impl BillingProvider for MockBillingProvider {
    fn create_checkout_session(
        &self,
        account_id: &str,
        plan: PlanTier,
    ) -> Result<CheckoutSessionResponse, BillingError> {
        // Deterministic: account + plan fully determine the URL. The host mirrors
        // a real Stripe Checkout URL; the session segment is an explicit mock id.
        Ok(CheckoutSessionResponse {
            url: format!(
                "https://checkout.stripe.com/c/pay/mock_{account_id}_{plan}",
                plan = plan.as_str()
            ),
        })
    }

    fn create_portal_session(
        &self,
        account_id: &str,
    ) -> Result<PortalSessionResponse, BillingError> {
        Ok(PortalSessionResponse {
            url: format!("https://billing.stripe.com/p/session/mock_{account_id}"),
        })
    }
}

/// The real Stripe billing provider — the live seam behind the same trait.
///
/// Today this is a compile-only stub: it carries the place where the secret key
/// and price catalog will live and returns [`BillingError::NotConfigured`] until
/// the live `async-stripe` integration lands in its own issue. It is deliberately
/// NOT wired into any default app or test, so the build needs no Stripe key and
/// the ADR-0002 desktop bans stay green (no Stripe crate enters the graph).
///
/// When the live integration lands: `create_checkout_session` calls
/// `stripe::CheckoutSession::create` with `mode: subscription`, the account's
/// Stripe customer, the selected Price, and success/cancel URLs;
/// `create_portal_session` calls `stripe::BillingPortalSession::create` with the
/// customer and a return URL. The route and DTOs do not change.
#[derive(Debug, Clone)]
pub struct StripeBillingProvider {
    /// The restricted Stripe API key (`rk_…`). Held only here, in the backend,
    /// never serialized, never sent to the desktop (ADR-0001/0002).
    _secret_key: String,
}

impl StripeBillingProvider {
    /// Construct the real provider from a restricted Stripe key. Present so the
    /// production wiring has a constructor to call; the live API calls land in a
    /// follow-up issue.
    pub fn new(secret_key: impl Into<String>) -> Self {
        Self {
            _secret_key: secret_key.into(),
        }
    }
}

impl BillingProvider for StripeBillingProvider {
    fn create_checkout_session(
        &self,
        _account_id: &str,
        _plan: PlanTier,
    ) -> Result<CheckoutSessionResponse, BillingError> {
        // Live Stripe Checkout Session creation lands in a follow-up issue.
        Err(BillingError::NotConfigured)
    }

    fn create_portal_session(
        &self,
        _account_id: &str,
    ) -> Result<PortalSessionResponse, BillingError> {
        // Live Stripe Customer Portal Session creation lands in a follow-up issue.
        Err(BillingError::NotConfigured)
    }
}

/// Shared state for the billing routes: the principal store (who is calling) and
/// the billing provider (mock in dev/test, real Stripe in production). Both
/// behind `Arc<dyn …>` so the durable principal store and the real provider drop
/// in without touching the handlers.
#[derive(Clone)]
pub struct BillingState {
    pub principals: Arc<dyn PrincipalStore>,
    pub provider: Arc<dyn BillingProvider>,
}

/// Routes for `POST /billing/checkout` and `POST /billing/portal`, carrying their
/// own [`BillingState`]. Returned as a `Router<()>` (state applied) so it merges
/// into the app router alongside the other stateful routes.
pub fn router(state: BillingState) -> Router {
    Router::new()
        .route("/billing/checkout", post(create_checkout))
        .route("/billing/portal", post(create_portal))
        .with_state(state)
}

/// `POST /billing/checkout` handler. Authenticates the caller, then asks the
/// provider for a checkout URL bound to the *authenticated* account. `401` on any
/// auth failure; `503` if the provider is not configured.
async fn create_checkout(
    State(state): State<BillingState>,
    headers: HeaderMap,
    Json(req): Json<CheckoutSessionRequest>,
) -> Result<Json<CheckoutSessionResponse>, StatusCode> {
    let principal = authenticate(&state, &headers)?;
    state
        .provider
        .create_checkout_session(&principal.account_id, req.plan)
        .map(Json)
        .map_err(billing_status)
}

/// `POST /billing/portal` handler. Authenticates the caller, then asks the
/// provider for a portal URL bound to the authenticated account.
async fn create_portal(
    State(state): State<BillingState>,
    headers: HeaderMap,
) -> Result<Json<PortalSessionResponse>, StatusCode> {
    let principal = authenticate(&state, &headers)?;
    state
        .provider
        .create_portal_session(&principal.account_id)
        .map(Json)
        .map_err(billing_status)
}

/// Resolve the bearer credential to a principal, mapping any auth failure to
/// `401`. Both "no/malformed credential" and "unknown token" are `401` — the
/// endpoint never reveals which, matching the other authed routes.
fn authenticate(
    state: &BillingState,
    headers: &HeaderMap,
) -> Result<crate::principal::Principal, StatusCode> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    match resolve_principal(state.principals.as_ref(), auth_header) {
        Ok(p) => Ok(p),
        Err(AuthError::MissingCredentials) | Err(AuthError::InvalidToken) => {
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

/// Map a provider failure onto an HTTP status. A not-configured provider is a
/// server-side gap, not a client error, so it is `503`.
fn billing_status(err: BillingError) -> StatusCode {
    match err {
        BillingError::NotConfigured => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_checkout_url_has_stripe_checkout_host_and_encodes_account() {
        let provider = MockBillingProvider::new();
        let resp = provider
            .create_checkout_session("acct_acme", PlanTier::Pro)
            .unwrap();
        assert!(
            resp.url.starts_with("https://checkout.stripe.com/"),
            "checkout URL must be Stripe-Checkout-shaped, got {}",
            resp.url
        );
        assert!(
            resp.url.contains("acct_acme"),
            "URL must encode the account"
        );
    }

    #[test]
    fn mock_portal_url_has_stripe_billing_host_and_encodes_account() {
        let provider = MockBillingProvider::new();
        let resp = provider.create_portal_session("acct_acme").unwrap();
        assert!(
            resp.url.starts_with("https://billing.stripe.com/"),
            "portal URL must be Stripe-portal-shaped, got {}",
            resp.url
        );
        assert!(resp.url.contains("acct_acme"));
    }

    #[test]
    fn mock_checkout_is_deterministic_across_calls() {
        // Purity: same inputs → same URL, no clock/RNG/network.
        let provider = MockBillingProvider::new();
        let a = provider
            .create_checkout_session("acct_acme", PlanTier::Pro)
            .unwrap();
        let b = provider
            .create_checkout_session("acct_acme", PlanTier::Pro)
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn mock_checkout_distinguishes_accounts() {
        let provider = MockBillingProvider::new();
        let acme = provider
            .create_checkout_session("acct_acme", PlanTier::Pro)
            .unwrap();
        let other = provider
            .create_checkout_session("acct_other", PlanTier::Pro)
            .unwrap();
        assert_ne!(acme.url, other.url, "two accounts get distinct URLs");
    }

    #[test]
    fn mock_checkout_distinguishes_plans() {
        let provider = MockBillingProvider::new();
        let pro = provider
            .create_checkout_session("acct_acme", PlanTier::Pro)
            .unwrap();
        let team = provider
            .create_checkout_session("acct_acme", PlanTier::Team)
            .unwrap();
        assert_ne!(pro.url, team.url);
    }

    #[test]
    fn real_stripe_provider_is_not_configured_until_wired() {
        // The real seam exists behind the SAME trait and compiles without a key,
        // but is inert until the live integration lands — it must never silently
        // succeed.
        let provider = StripeBillingProvider::new("rk_test_placeholder");
        assert_eq!(
            provider.create_checkout_session("acct_acme", PlanTier::Pro),
            Err(BillingError::NotConfigured)
        );
        assert_eq!(
            provider.create_portal_session("acct_acme"),
            Err(BillingError::NotConfigured)
        );
    }
}
