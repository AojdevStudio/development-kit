//! Request / response DTOs for the billing session surface.
//!
//! These are the wire shapes for `POST /billing/checkout` and
//! `POST /billing/portal`. The desktop sends a [`CheckoutSessionRequest`] naming
//! the plan it wants, and the backend returns a [`CheckoutSessionResponse`] or
//! [`PortalSessionResponse`] carrying a single `url` the desktop opens in the
//! system browser.
//!
//! Per ADR-0002 this file is **types only**: no `async-stripe`, no `sqlx`, no
//! secret loaders. The provider that mints these URLs (mock or real Stripe) lives
//! in `services/api`, behind a trait — `shared` never learns whether the URL came
//! from a deterministic mock or a live Stripe call.
//!
//! Authority note (ADR-0001): the response carries the account-bound URL the
//! *backend* produced. The request deliberately does NOT carry an account id —
//! the backend binds the session to the authenticated principal's account, so a
//! client can never start checkout against an account it does not own.

use serde::{Deserialize, Serialize};

use crate::plan::PlanTier;

// ---------------------------------------------------------------------------
// POST /billing/checkout
// ---------------------------------------------------------------------------

/// Request body for `POST /billing/checkout`.
///
/// Carries only the plan the user chose. The account is resolved server-side from
/// the bearer credential — never trusted from the client (ADR-0001). The `plan`
/// is the typed [`PlanTier`] so an invalid tier cannot be expressed on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutSessionRequest {
    /// The plan tier the user is upgrading to. Typed to make magic-string plans
    /// unrepresentable (invalid-state guard).
    pub plan: PlanTier,
}

/// Response body for `POST /billing/checkout`.
///
/// A single URL the desktop opens in the system browser to complete checkout. The
/// desktop never constructs this URL — it is the backend's authoritative output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutSessionResponse {
    /// The provider checkout URL (Stripe Checkout in production; a deterministic
    /// mock URL in dev/test).
    pub url: String,
}

// ---------------------------------------------------------------------------
// POST /billing/portal
// ---------------------------------------------------------------------------

/// Response body for `POST /billing/portal`.
///
/// A single URL the desktop opens in the system browser so the user can manage
/// payment methods, invoices, cancellation, or plan changes. Backend-produced and
/// account-bound, like the checkout URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalSessionResponse {
    /// The provider customer-portal URL (Stripe Customer Portal in production; a
    /// deterministic mock URL in dev/test).
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_request_round_trips_through_json_with_typed_plan() {
        let req = CheckoutSessionRequest {
            plan: PlanTier::Pro,
        };
        let json = serde_json::to_string(&req).unwrap();
        // The typed plan serializes to its stable wire string.
        assert!(json.contains("\"pro\""));
        let back: CheckoutSessionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
        assert_eq!(back.plan, PlanTier::Pro);
    }

    #[test]
    fn checkout_request_accepts_every_paid_tier() {
        for plan in [
            PlanTier::Starter,
            PlanTier::Pro,
            PlanTier::Team,
            PlanTier::Enterprise,
        ] {
            let req = CheckoutSessionRequest { plan };
            let back: CheckoutSessionRequest =
                serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
            assert_eq!(back.plan, plan);
        }
    }

    #[test]
    fn checkout_response_round_trips_through_json() {
        let resp = CheckoutSessionResponse {
            url: "https://checkout.stripe.com/c/pay/mock_acct_acme".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CheckoutSessionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
        assert_eq!(back.url, "https://checkout.stripe.com/c/pay/mock_acct_acme");
    }

    #[test]
    fn portal_response_round_trips_through_json() {
        let resp = PortalSessionResponse {
            url: "https://billing.stripe.com/p/session/mock_acct_acme".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: PortalSessionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
        assert_eq!(
            back.url,
            "https://billing.stripe.com/p/session/mock_acct_acme"
        );
    }
}
