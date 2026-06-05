use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::entitlements::FeatureValue;
use crate::feature_key::FeatureKey;

/// The payload of a short-lived signed license token.
///
/// The cloud backend (`license-sign`) signs this; the desktop Tauri layer
/// (`license-verify`) verifies the signature and reads it for bounded offline
/// paid access. Both sides depend on this one struct so the signed bytes have a
/// single canonical shape. It is a pure DTO — neither signing nor verification
/// logic lives here (ADR-0002 keeps `shared` crypto-free).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseToken {
    pub token_id: String,
    pub user_id: String,
    pub account_id: String,
    pub plan: String,
    #[serde(default)]
    pub features: BTreeMap<FeatureKey, FeatureValue>,
    /// Unix epoch seconds when the token was issued.
    pub issued_at: u64,
    /// Unix epoch seconds when the token stops being valid.
    pub expires_at: u64,
}

impl LicenseToken {
    /// The canonical byte representation that gets signed and verified. Both the
    /// signer and the verifier must serialize identically, so this is the single
    /// definition of "the bytes". JSON with sorted keys (`BTreeMap`) gives a
    /// stable, deterministic encoding.
    pub fn signing_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("LicenseToken is always serializable")
    }

    /// Whether the token is expired relative to `now` (unix epoch seconds).
    pub fn is_expired_at(&self, now: u64) -> bool {
        now >= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> LicenseToken {
        let mut features = BTreeMap::new();
        features.insert(FeatureKey::ExportPdf, FeatureValue::Enabled(true));
        LicenseToken {
            token_id: "lic_123".into(),
            user_id: "user_123".into(),
            account_id: "acct_123".into(),
            plan: "pro".into(),
            features,
            issued_at: 1_700_000_000,
            expires_at: 1_700_604_800,
        }
    }

    #[test]
    fn signing_bytes_are_deterministic() {
        let a = token();
        let b = token();
        assert_eq!(a.signing_bytes(), b.signing_bytes());
    }

    #[test]
    fn token_round_trips_through_json() {
        let t = token();
        let json = serde_json::to_vec(&t).unwrap();
        let back: LicenseToken = serde_json::from_slice(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn is_expired_at_uses_expiry_boundary() {
        let t = token();
        assert!(!t.is_expired_at(t.expires_at - 1));
        assert!(t.is_expired_at(t.expires_at));
        assert!(t.is_expired_at(t.expires_at + 1));
    }
}
