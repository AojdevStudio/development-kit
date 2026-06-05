use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::feature_key::FeatureKey;
use crate::plan::{PlanTier, SubscriptionStatus};

/// The value a feature key resolves to for an account.
///
/// Boolean features are on/off; limit features carry a numeric ceiling
/// (e.g. `team_members: 5`, `max_projects: 100`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FeatureValue {
    Enabled(bool),
    Limit(u64),
}

/// The app-facing expression of an account's paid access.
///
/// The backend computes this from plan + subscription + trial + usage; the app
/// reads it and never derives access from raw Stripe objects. This is a DTO —
/// the entitlement *engine* lives in `services/api`, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entitlements {
    pub account_id: String,
    /// The subscription plan tier for this account. Typed to prevent
    /// callers from comparing against magic strings (invalid-state guard).
    pub plan: PlanTier,
    /// The current subscription lifecycle state. Typed to make impossible
    /// states (e.g. Active with Free plan) visible at the type level.
    pub status: SubscriptionStatus,
    #[serde(default)]
    pub trial: bool,
    #[serde(default)]
    pub features: BTreeMap<FeatureKey, FeatureValue>,
    /// Unix epoch seconds when the current offline license token expires, if
    /// one has been issued. `None` means no offline token has been granted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_expires_at: Option<u64>,
}

impl Entitlements {
    /// Whether a boolean feature is allowed. Limit features are "allowed" when
    /// their ceiling is non-zero. This is the question gate code should ask —
    /// "does this account have X?" — rather than reasoning about plan names.
    pub fn allows(&self, feature: FeatureKey) -> bool {
        match self.features.get(&feature) {
            Some(FeatureValue::Enabled(b)) => *b,
            Some(FeatureValue::Limit(n)) => *n > 0,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{PlanTier, SubscriptionStatus};

    fn pro_entitlements() -> Entitlements {
        let mut features = BTreeMap::new();
        features.insert(FeatureKey::ExportPdf, FeatureValue::Enabled(true));
        features.insert(FeatureKey::CloudSync, FeatureValue::Enabled(false));
        features.insert(FeatureKey::MaxProjects, FeatureValue::Limit(100));
        features.insert(FeatureKey::TeamMembers, FeatureValue::Limit(0));
        Entitlements {
            account_id: "acct_123".into(),
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Active,
            trial: false,
            features,
            license_expires_at: None,
        }
    }

    #[test]
    fn allows_reflects_enabled_boolean_features() {
        let ent = pro_entitlements();
        assert!(ent.allows(FeatureKey::ExportPdf));
        assert!(!ent.allows(FeatureKey::CloudSync));
    }

    #[test]
    fn allows_treats_nonzero_limit_as_allowed_and_zero_as_denied() {
        let ent = pro_entitlements();
        assert!(ent.allows(FeatureKey::MaxProjects));
        assert!(!ent.allows(FeatureKey::TeamMembers));
    }

    #[test]
    fn allows_denies_unknown_feature() {
        let ent = pro_entitlements();
        assert!(!ent.allows(FeatureKey::ApiAccess));
    }

    #[test]
    fn entitlements_round_trip_through_json() {
        let ent = pro_entitlements();
        let json = serde_json::to_string(&ent).unwrap();
        let back: Entitlements = serde_json::from_str(&json).unwrap();
        assert_eq!(ent, back);
    }

    #[test]
    fn entitlements_license_expires_at_omitted_when_none() {
        // When no offline token has been issued, the field should not appear in
        // the JSON payload — saves bytes on the wire.
        let ent = pro_entitlements(); // license_expires_at = None
        let json = serde_json::to_string(&ent).unwrap();
        assert!(!json.contains("license_expires_at"));
    }

    #[test]
    fn entitlements_license_expires_at_round_trips_when_set() {
        let mut ent = pro_entitlements();
        ent.license_expires_at = Some(1_700_604_800);
        let json = serde_json::to_string(&ent).unwrap();
        let back: Entitlements = serde_json::from_str(&json).unwrap();
        assert_eq!(back.license_expires_at, Some(1_700_604_800));
    }

    #[test]
    fn trial_entitlements_round_trip_with_typed_plan_and_status() {
        // Trial accounts use PlanTier::Pro (or higher) with
        // SubscriptionStatus::Trialing — the type system prevents accidentally
        // mixing free plan with active paid status.
        let mut features = BTreeMap::new();
        features.insert(FeatureKey::ExportPdf, FeatureValue::Enabled(true));
        features.insert(FeatureKey::CloudSync, FeatureValue::Enabled(true));
        let ent = Entitlements {
            account_id: "acct_trial".into(),
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Trialing,
            trial: true,
            features,
            license_expires_at: None,
        };
        let json = serde_json::to_string(&ent).unwrap();
        let back: Entitlements = serde_json::from_str(&json).unwrap();
        assert_eq!(ent, back);
        assert_eq!(back.plan, PlanTier::Pro);
        assert_eq!(back.status, SubscriptionStatus::Trialing);
        assert!(back.trial);
        assert!(back.status.grants_paid_access());
    }

    #[test]
    fn free_plan_entitlements_round_trip() {
        let ent = Entitlements {
            account_id: "acct_free".into(),
            plan: PlanTier::Free,
            status: SubscriptionStatus::Free,
            trial: false,
            features: BTreeMap::new(),
            license_expires_at: None,
        };
        let json = serde_json::to_string(&ent).unwrap();
        let back: Entitlements = serde_json::from_str(&json).unwrap();
        assert_eq!(ent, back);
        assert!(!back.plan.is_paid());
        assert!(!back.status.grants_paid_access());
    }
}
