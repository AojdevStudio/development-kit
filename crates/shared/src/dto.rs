//! Request / response DTOs for the cloud API surface.
//!
//! These are the wire shapes used by `GET /me/entitlements`,
//! `POST /license/refresh`, and related endpoints. Keeping them in `shared`
//! lets the desktop app and the backend both depend on the same struct
//! definitions — no hand-rolled JSON parsing on either side.
//!
//! Per ADR-0002, this file is **types only**: no sqlx, no Stripe, no secret
//! loaders. The entitlement engine that produces these responses lives in
//! `services/api`.

use serde::{Deserialize, Serialize};

use crate::entitlements::Entitlements;

// ---------------------------------------------------------------------------
// GET /me/entitlements
// ---------------------------------------------------------------------------

/// Response body for `GET /me/entitlements`.
///
/// The desktop app calls this endpoint after authentication to learn the
/// account's current paid access. The response is the authoritative word from
/// the backend; the desktop never derives access from local state alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitlementsResponse {
    pub entitlements: Entitlements,
}

// ---------------------------------------------------------------------------
// POST /license/refresh
// ---------------------------------------------------------------------------

/// Request body for `POST /license/refresh`.
///
/// The desktop sends this to ask the backend to issue (or re-issue) a
/// short-lived signed license token for offline paid access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseRefreshRequest {
    /// The account for which the token is being requested. The backend
    /// validates that the authenticated user belongs to this account before
    /// issuing a token.
    pub account_id: String,
}

/// Response body for `POST /license/refresh`.
///
/// The backend returns the raw signed token bytes (Base64-encoded) and the
/// verifying public key so the desktop can store and verify the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseRefreshResponse {
    /// Base64-encoded canonical payload bytes (`LicenseToken::signing_bytes`).
    pub payload_b64: String,
    /// Base64-encoded 64-byte ed25519 signature over `payload_b64`.
    pub signature_b64: String,
    /// Unix epoch seconds when this token expires. The desktop should schedule
    /// a refresh before this point.
    pub expires_at: u64,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::entitlements::FeatureValue;
    use crate::feature_key::FeatureKey;
    use crate::plan::{PlanTier, SubscriptionStatus};

    fn sample_entitlements() -> Entitlements {
        let mut features = BTreeMap::new();
        features.insert(FeatureKey::ExportPdf, FeatureValue::Enabled(true));
        features.insert(FeatureKey::MaxProjects, FeatureValue::Limit(50));
        Entitlements {
            account_id: "acct_abc".into(),
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Active,
            trial: false,
            features,
            license_expires_at: Some(1_700_604_800),
        }
    }

    // --- EntitlementsResponse ---

    #[test]
    fn entitlements_response_round_trips_through_json() {
        let resp = EntitlementsResponse {
            entitlements: sample_entitlements(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: EntitlementsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn entitlements_response_preserves_feature_access() {
        let resp = EntitlementsResponse {
            entitlements: sample_entitlements(),
        };
        // The key behavior: after a round-trip the allows() gate still works.
        let json = serde_json::to_string(&resp).unwrap();
        let back: EntitlementsResponse = serde_json::from_str(&json).unwrap();
        assert!(back.entitlements.allows(FeatureKey::ExportPdf));
        assert!(back.entitlements.allows(FeatureKey::MaxProjects));
        assert!(!back.entitlements.allows(FeatureKey::CloudSync));
    }

    // --- LicenseRefreshRequest ---

    #[test]
    fn license_refresh_request_round_trips_through_json() {
        let req = LicenseRefreshRequest {
            account_id: "acct_abc".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: LicenseRefreshRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    // --- LicenseRefreshResponse ---

    #[test]
    fn license_refresh_response_round_trips_through_json() {
        let resp = LicenseRefreshResponse {
            payload_b64: "dGVzdA==".into(),
            signature_b64: "c2lnbmF0dXJl".into(),
            expires_at: 1_700_604_800,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: LicenseRefreshResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn license_refresh_response_expires_at_is_preserved() {
        // The expires_at drives the desktop's re-refresh schedule; it must
        // survive serialization without loss.
        let resp = LicenseRefreshResponse {
            payload_b64: "dGVzdA==".into(),
            signature_b64: "c2lnbmF0dXJl".into(),
            expires_at: u64::MAX,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: LicenseRefreshResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.expires_at, u64::MAX);
    }
}
