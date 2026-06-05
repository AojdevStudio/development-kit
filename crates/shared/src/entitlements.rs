use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::feature_key::FeatureKey;

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
    pub plan: String,
    pub status: String,
    #[serde(default)]
    pub trial: bool,
    #[serde(default)]
    pub features: BTreeMap<FeatureKey, FeatureValue>,
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

    fn pro_entitlements() -> Entitlements {
        let mut features = BTreeMap::new();
        features.insert(FeatureKey::ExportPdf, FeatureValue::Enabled(true));
        features.insert(FeatureKey::CloudSync, FeatureValue::Enabled(false));
        features.insert(FeatureKey::MaxProjects, FeatureValue::Limit(100));
        features.insert(FeatureKey::TeamMembers, FeatureValue::Limit(0));
        Entitlements {
            account_id: "acct_123".into(),
            plan: "pro".into(),
            status: "active".into(),
            trial: false,
            features,
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
}
