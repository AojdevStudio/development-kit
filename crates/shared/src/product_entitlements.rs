//! Product-key entitlements (issue #36 — product module seam).
//!
//! [`Entitlements`](crate::Entitlements) carries the baseline
//! [`FeatureKey`](crate::FeatureKey)-keyed feature map the spine computes. A
//! product module declares its OWN gated capabilities as
//! [`ProductFeatureKey`]s, which by construction are not baseline keys — so they
//! cannot live in the baseline map. [`ProductEntitlements`] is the parallel,
//! product-namespaced snapshot that carries them, with the **same** allow
//! semantics as [`Entitlements::allows`](crate::Entitlements::allows):
//!
//! - a boolean key is allowed iff it is present and `true`,
//! - a limit key is allowed iff its ceiling is non-zero,
//! - an absent key is denied (no silent default-allow).
//!
//! Because the question is identical, a product gate can never be weaker than a
//! baseline gate. The backend computes this snapshot the same way it computes
//! [`Entitlements`](crate::Entitlements) — from plan + subscription + the
//! product's per-tier policy — and the desktop reads it; the desktop never
//! authors entitlement *values* (ADR-0001).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::entitlements::FeatureValue;
use crate::product_feature_key::ProductFeatureKey;

/// An account's resolved access to a product's [`ProductFeatureKey`] capabilities.
///
/// Parallel to [`Entitlements`](crate::Entitlements) but keyed on product keys.
/// The backend builds it; the gate (`require_product_feature`) and the desktop
/// read it. Defaulting `features` to empty means an account with no product
/// entitlements simply denies every product key.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProductEntitlements {
    /// The account these product entitlements were resolved for.
    #[serde(default)]
    pub account_id: String,
    /// The product namespace these keys belong to (e.g. `vault`). Lets a snapshot
    /// be scoped to one product and keeps the wire payload self-describing.
    #[serde(default)]
    pub namespace: String,
    /// The resolved value per product key. Same [`FeatureValue`] vocabulary as
    /// the baseline map, so the allow question is byte-for-byte the same.
    #[serde(default)]
    pub features: BTreeMap<ProductFeatureKey, FeatureValue>,
}

impl ProductEntitlements {
    /// Start an empty snapshot for `namespace` and `account_id`.
    pub fn new(account_id: impl Into<String>, namespace: impl Into<String>) -> Self {
        ProductEntitlements {
            account_id: account_id.into(),
            namespace: namespace.into(),
            features: BTreeMap::new(),
        }
    }

    /// Bind `key` to `value`, replacing any existing binding. Chainable for
    /// one-expression construction in the engine and in tests.
    pub fn with(mut self, key: ProductFeatureKey, value: FeatureValue) -> Self {
        self.features.insert(key, value);
        self
    }

    /// Whether a product `key` is allowed. Identical semantics to
    /// [`Entitlements::allows`](crate::Entitlements::allows): a boolean is allowed
    /// when `true`, a limit when non-zero, an absent key never. This is the single
    /// question every product gate asks, so a product gate can never drift weaker
    /// than the spine's.
    pub fn allows(&self, key: &ProductFeatureKey) -> bool {
        match self.features.get(key) {
            Some(FeatureValue::Enabled(b)) => *b,
            Some(FeatureValue::Limit(n)) => *n > 0,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> ProductFeatureKey {
        ProductFeatureKey::new("vault", name).expect("valid product key")
    }

    // --- ISC-8: an absent product key is denied ---
    #[test]
    fn absent_key_is_denied() {
        let ent = ProductEntitlements::new("acct_1", "vault");
        assert!(!ent.allows(&key("share_record")));
    }

    // --- ISC-9: a zero-limit product key is denied ---
    #[test]
    fn zero_limit_key_is_denied() {
        let ent = ProductEntitlements::new("acct_1", "vault")
            .with(key("max_vaults"), FeatureValue::Limit(0));
        assert!(!ent.allows(&key("max_vaults")));
    }

    // --- ISC-10: an enabled boolean product key is allowed ---
    #[test]
    fn enabled_boolean_key_is_allowed() {
        let ent = ProductEntitlements::new("acct_1", "vault")
            .with(key("share_record"), FeatureValue::Enabled(true));
        assert!(ent.allows(&key("share_record")));
    }

    #[test]
    fn nonzero_limit_key_is_allowed() {
        let ent = ProductEntitlements::new("acct_1", "vault")
            .with(key("max_vaults"), FeatureValue::Limit(10));
        assert!(ent.allows(&key("max_vaults")));
    }

    #[test]
    fn disabled_boolean_key_is_denied() {
        let ent = ProductEntitlements::new("acct_1", "vault")
            .with(key("share_record"), FeatureValue::Enabled(false));
        assert!(!ent.allows(&key("share_record")));
    }

    #[test]
    fn product_entitlements_round_trip_through_json() {
        let ent = ProductEntitlements::new("acct_1", "vault")
            .with(key("share_record"), FeatureValue::Enabled(true))
            .with(key("max_vaults"), FeatureValue::Limit(5));
        let json = serde_json::to_string(&ent).unwrap();
        let back: ProductEntitlements = serde_json::from_str(&json).unwrap();
        assert_eq!(ent, back);
    }
}
