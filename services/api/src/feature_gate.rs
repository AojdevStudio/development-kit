//! Backend feature-gate authorization (ADR-0001 server-side gates).
//!
//! A server-backed paid action must be decided by the backend, not the screen.
//! This module holds the decision as a pure function over the caller's
//! [`Entitlements`] plus the [`FeatureKey`] being exercised, and exposes a single
//! Axum route that turns that decision into a 403/200. Product endpoints reuse
//! [`require_feature`] rather than re-deriving access from plan names, so every
//! gated action asks the same question: "does this account have this feature?"
//!
//! The `/gated/{feature}` route exists so the platform spine ships a real,
//! exercised authority boundary the feature-key coverage gate (issue #25) can
//! count. Product modules add their own gated routes the same way.
//!
//! The **authenticated** gate `/gated-feature/{feature}` (issue #30) closes the
//! end-to-end loop: instead of trusting an entitlements body the caller supplies,
//! it resolves the caller's entitlement snapshot from their bearer token — auth →
//! account state → the entitlement engine — and gates against *that*. This is the
//! route the desktop calls to enforce a paid action on the server (ADR-0001); a
//! forged request body can never grant access because the body is never read.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};

use shared::{Entitlements, FeatureKey, ProductEntitlements, ProductFeatureKey};

use crate::auth::{resolve_principal, AuthError, PrincipalStore};
use crate::entitlement::{resolve_entitlements, AccountStateStore};

/// Parse a stable wire string back into a [`FeatureKey`], or `None` if it is not
/// one of the known keys. The backend only gates the explicit vocabulary — an
/// unknown string is never silently allowed.
pub fn parse_feature_key(token: &str) -> Option<FeatureKey> {
    FeatureKey::ALL.into_iter().find(|k| k.as_str() == token)
}

/// The authority decision: may this account perform an action gated on
/// `feature`? Pure over the entitlements DTO the backend computed for the
/// caller, so it is unit-testable without a router and reusable by every gated
/// endpoint.
pub fn require_feature(entitlements: &Entitlements, feature: FeatureKey) -> Result<(), StatusCode> {
    if entitlements.allows(feature) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// The authority decision for a **product** feature key (issue #36 — product
/// module seam). The product analogue of [`require_feature`]: pure over the
/// product entitlements snapshot the backend computed, so a product module's
/// gated routes ask exactly the same question (`does this account have X?`)
/// against the same allow semantics. A product gate therefore can never be
/// weaker than the spine's — and a product never re-derives access from plan
/// names.
pub fn require_product_feature(
    entitlements: &ProductEntitlements,
    feature: &ProductFeatureKey,
) -> Result<(), StatusCode> {
    if entitlements.allows(feature) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Routes for the backend feature gate. Merged into the app router by `app()`.
pub fn router() -> Router {
    Router::new().route("/gated/{feature}", post(gated_action))
}

/// A server-backed paid action guarded by a feature key. Returns 200 when the
/// caller's entitlements allow the feature, 403 when they do not, and 404 when
/// the path segment is not a known feature key.
async fn gated_action(
    Path(feature): Path<String>,
    Json(entitlements): Json<Entitlements>,
) -> StatusCode {
    let Some(key) = parse_feature_key(&feature) else {
        return StatusCode::NOT_FOUND;
    };
    match require_feature(&entitlements, key) {
        Ok(()) => StatusCode::OK,
        Err(status) => status,
    }
}

/// Shared state for the authenticated feature gate: the principal store (who is
/// calling) and the account-state store (what their account is entitled to). Both
/// behind `Arc<dyn …>` so the durable Postgres-backed stores drop in without
/// touching the handler — the same seam `me_entitlements` uses.
#[derive(Clone)]
pub struct FeatureGateState {
    pub principals: Arc<dyn PrincipalStore>,
    pub accounts: Arc<dyn AccountStateStore>,
}

/// Routes for the authenticated feature gate `POST /gated-feature/{feature}`,
/// carrying their own [`FeatureGateState`].
///
/// Returned as a `Router<()>` (state already applied) so it merges cleanly into
/// the app router alongside the other stateful routes, none of which it touches.
pub fn authenticated_router(state: FeatureGateState) -> Router {
    Router::new()
        .route("/gated-feature/{feature}", post(authenticated_gate))
        .with_state(state)
}

/// `POST /gated-feature/{feature}` — the authenticated server-side gate.
///
/// Resolves the caller's entitlement snapshot from their bearer token (auth →
/// account state → the entitlement engine), then gates the requested feature.
/// Returns 200 when the resolved snapshot allows it, 403 when it does not, 401 on
/// any auth failure, 404 when the path segment is not a known feature key. The
/// request body is intentionally NOT read: the verdict is the backend's, computed
/// from the account's real billing state, so a forged body can never grant access
/// (ADR-0001).
async fn authenticated_gate(
    State(state): State<FeatureGateState>,
    Path(feature): Path<String>,
    headers: HeaderMap,
) -> StatusCode {
    let Some(key) = parse_feature_key(&feature) else {
        return StatusCode::NOT_FOUND;
    };

    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let principal = match resolve_principal(state.principals.as_ref(), auth_header) {
        Ok(p) => p,
        Err(AuthError::MissingCredentials) | Err(AuthError::InvalidToken) => {
            return StatusCode::UNAUTHORIZED
        }
    };

    let Some(account_state) = state.accounts.account_state(&principal.account_id) else {
        // Authenticated, but the account has no billing state on record: nothing
        // grants the feature, so the paid action is forbidden.
        return StatusCode::FORBIDDEN;
    };

    let entitlements = resolve_entitlements(principal.account_id, &account_state, now_unix());
    match require_feature(&entitlements, key) {
        Ok(()) => StatusCode::OK,
        Err(status) => status,
    }
}

/// The current wall-clock instant in unix epoch seconds, used as the engine's
/// `now`. The engine takes `now` as a parameter (pure/testable); only this
/// boundary reads the clock.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{FeatureValue, PlanTier, SubscriptionStatus};
    use std::collections::BTreeMap;

    fn entitlements_with(feature: FeatureKey, value: FeatureValue) -> Entitlements {
        let mut features = BTreeMap::new();
        features.insert(feature, value);
        Entitlements {
            account_id: "acct_test".into(),
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Active,
            trial: false,
            features,
            license_expires_at: None,
        }
    }

    #[test]
    fn require_feature_allows_an_enabled_feature() {
        let ent = entitlements_with(FeatureKey::ExportPdf, FeatureValue::Enabled(true));
        assert_eq!(require_feature(&ent, FeatureKey::ExportPdf), Ok(()));
    }

    #[test]
    fn require_feature_denies_a_missing_feature() {
        let ent = entitlements_with(FeatureKey::ExportPdf, FeatureValue::Enabled(true));
        assert_eq!(
            require_feature(&ent, FeatureKey::CloudSync),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn require_feature_denies_a_zero_limit() {
        let ent = entitlements_with(FeatureKey::TeamMembers, FeatureValue::Limit(0));
        assert_eq!(
            require_feature(&ent, FeatureKey::TeamMembers),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn parse_feature_key_round_trips_every_key() {
        for key in FeatureKey::ALL {
            assert_eq!(parse_feature_key(key.as_str()), Some(key));
        }
        assert_eq!(parse_feature_key("not_a_key"), None);
    }

    // --- #36: product-key gate mirrors the baseline gate ---
    fn product_key(name: &str) -> ProductFeatureKey {
        ProductFeatureKey::new("vault", name).expect("valid product key")
    }

    #[test]
    fn require_product_feature_allows_an_enabled_product_key() {
        let key = product_key("share_record");
        let ent = ProductEntitlements::new("acct_test", "vault")
            .with(key.clone(), FeatureValue::Enabled(true));
        assert_eq!(require_product_feature(&ent, &key), Ok(()));
    }

    #[test]
    fn require_product_feature_denies_a_missing_product_key() {
        let ent = ProductEntitlements::new("acct_test", "vault");
        assert_eq!(
            require_product_feature(&ent, &product_key("share_record")),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn require_product_feature_denies_a_zero_limit_product_key() {
        let key = product_key("max_vaults");
        let ent = ProductEntitlements::new("acct_test", "vault")
            .with(key.clone(), FeatureValue::Limit(0));
        assert_eq!(
            require_product_feature(&ent, &key),
            Err(StatusCode::FORBIDDEN)
        );
    }
}
