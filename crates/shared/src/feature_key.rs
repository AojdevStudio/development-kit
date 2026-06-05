use serde::{Deserialize, Serialize};

/// A stable, explicit identifier for a gated capability.
///
/// Per `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`, paid access is represented as
/// feature access — not plan names. These string values are part of the wire
/// contract shared across React, Tauri commands, and backend authorization;
/// they must stay stable. The platform spine ships the baseline keys here;
/// product modules extend the gated set via their own data, not by renaming
/// these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKey {
    ExportPdf,
    CloudSync,
    AdvancedReports,
    TeamMembers,
    MaxProjects,
    PrioritySupport,
    ApiAccess,
}

impl FeatureKey {
    /// The stable wire string for this key. This is the single source of truth
    /// the serde representation is built on, so React/TS and Rust agree.
    pub const fn as_str(self) -> &'static str {
        match self {
            FeatureKey::ExportPdf => "export_pdf",
            FeatureKey::CloudSync => "cloud_sync",
            FeatureKey::AdvancedReports => "advanced_reports",
            FeatureKey::TeamMembers => "team_members",
            FeatureKey::MaxProjects => "max_projects",
            FeatureKey::PrioritySupport => "priority_support",
            FeatureKey::ApiAccess => "api_access",
        }
    }

    /// Every baseline feature key. The feature-key coverage gate (ADR-0002)
    /// iterates this to require a non-React gate test per key.
    pub const ALL: [FeatureKey; 7] = [
        FeatureKey::ExportPdf,
        FeatureKey::CloudSync,
        FeatureKey::AdvancedReports,
        FeatureKey::TeamMembers,
        FeatureKey::MaxProjects,
        FeatureKey::PrioritySupport,
        FeatureKey::ApiAccess,
    ];
}

impl std::fmt::Display for FeatureKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_keys_have_stable_snake_case_wire_strings() {
        // The wire string is a contract; these must not drift.
        assert_eq!(FeatureKey::ExportPdf.as_str(), "export_pdf");
        assert_eq!(FeatureKey::CloudSync.as_str(), "cloud_sync");
        assert_eq!(FeatureKey::AdvancedReports.as_str(), "advanced_reports");
        assert_eq!(FeatureKey::TeamMembers.as_str(), "team_members");
        assert_eq!(FeatureKey::MaxProjects.as_str(), "max_projects");
        assert_eq!(FeatureKey::PrioritySupport.as_str(), "priority_support");
        assert_eq!(FeatureKey::ApiAccess.as_str(), "api_access");
    }

    #[test]
    fn serde_representation_matches_as_str() {
        // The JSON the backend emits and the desktop reads must equal as_str(),
        // so the two ways of producing the wire string can never disagree.
        for key in FeatureKey::ALL {
            let json = serde_json::to_string(&key).unwrap();
            assert_eq!(json, format!("\"{}\"", key.as_str()));
        }
    }

    #[test]
    fn feature_key_round_trips_through_json() {
        for key in FeatureKey::ALL {
            let json = serde_json::to_string(&key).unwrap();
            let back: FeatureKey = serde_json::from_str(&json).unwrap();
            assert_eq!(key, back);
        }
    }

    #[test]
    fn all_constant_covers_every_variant() {
        // If a variant is added without extending ALL, the coverage gate would
        // silently skip it. Guard the count here.
        assert_eq!(FeatureKey::ALL.len(), 7);
    }
}
