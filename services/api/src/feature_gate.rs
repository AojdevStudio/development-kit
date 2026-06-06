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

use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};

use shared::{Entitlements, FeatureKey};

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
}
