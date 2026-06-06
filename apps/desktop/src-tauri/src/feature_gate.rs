//! Local (Tauri-command) feature gate for one concrete paid feature (issue #30).
//!
//! ADR-0001 requires a paid action to be refused at the Tauri-command layer too,
//! not just at the screen — the React guard is UX only. This module holds the
//! local guard for `FeatureKey::AdvancedReports`: a pure decision over the
//! server-resolved [`Entitlements`] snapshot plus the typed [`FeatureKey`], and a
//! `#[tauri::command]` wrapper the desktop UI invokes before performing the local
//! paid action.
//!
//! Authority boundary (ADR-0001): the snapshot the command gates against is the
//! one the backend computed and the desktop *fetched* (via `GET /me/entitlements`)
//! — the desktop never constructs an authoritative snapshot of its own. This
//! module decides nothing about *what* the account is entitled to; it only
//! enforces the entitlement the backend already decided, using the same feature
//! key the React guard and the backend gate use.

use shared::{Entitlements, FeatureKey};

/// Why a local paid action was refused at the command layer.
///
/// Carries the feature key that was denied so the UI can show a precise,
/// upsell-friendly message rather than a generic failure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FeatureDenied {
    /// The stable wire string of the feature the caller lacked.
    pub feature: String,
}

/// Decide whether `entitlements` permit a `feature`-gated local action.
///
/// Pure over the server-resolved snapshot and the typed key, so it is unit
/// testable without a running app and reusable by every local gated command.
/// Mirrors the backend `require_feature`: it asks the same question
/// ("does this account have this feature?") against the same `allows` semantics,
/// so the local guard can never drift from the server's decision.
pub fn decide_feature(
    entitlements: &Entitlements,
    feature: FeatureKey,
) -> Result<(), FeatureDenied> {
    if entitlements.allows(feature) {
        Ok(())
    } else {
        Err(FeatureDenied {
            feature: feature.as_str().to_string(),
        })
    }
}

/// The local paid action guarded by `FeatureKey::AdvancedReports`.
///
/// The desktop calls this Tauri command before generating an advanced report
/// locally. `entitlements` is the snapshot the desktop fetched from the backend
/// authority (`GET /me/entitlements`); the command refuses the action when that
/// snapshot does not grant `AdvancedReports`, and allows it (returning the report
/// payload) when it does. The result is serialized back to React as the command's
/// `Result` — `Err(FeatureDenied)` becomes a rejected promise the UI handles.
#[tauri::command]
pub fn request_advanced_report(entitlements: Entitlements) -> Result<String, FeatureDenied> {
    decide_feature(&entitlements, FeatureKey::AdvancedReports)?;
    // The real product would compute the report here; the walking skeleton returns
    // a deterministic payload so the allow path is observable end-to-end.
    Ok("advanced-report:ready".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{FeatureValue, PlanTier, SubscriptionStatus};
    use std::collections::BTreeMap;

    /// A server-shaped snapshot granting exactly the listed features. Built the
    /// way the desktop would receive it from `GET /me/entitlements` — the desktop
    /// never authors entitlement *values*, it only reads them.
    fn snapshot(plan: PlanTier, features: &[(FeatureKey, FeatureValue)]) -> Entitlements {
        let mut map = BTreeMap::new();
        for (k, v) in features {
            map.insert(*k, v.clone());
        }
        Entitlements {
            account_id: "acct_test".into(),
            plan,
            status: SubscriptionStatus::Active,
            trial: false,
            features: map,
            license_expires_at: None,
        }
    }

    fn pro_snapshot() -> Entitlements {
        snapshot(
            PlanTier::Pro,
            &[(FeatureKey::AdvancedReports, FeatureValue::Enabled(true))],
        )
    }

    fn free_snapshot() -> Entitlements {
        // A free snapshot: advanced_reports explicitly off, exactly as the engine
        // would resolve it for a free account.
        snapshot(
            PlanTier::Free,
            &[(FeatureKey::AdvancedReports, FeatureValue::Enabled(false))],
        )
    }

    #[test]
    fn command_denies_advanced_report_without_entitlement() {
        // The whole point of the command gate: a free (unentitled) account is
        // refused the local paid action — the screen alone does not protect it.
        let denied = request_advanced_report(free_snapshot()).unwrap_err();
        assert_eq!(denied.feature, FeatureKey::AdvancedReports.as_str());
    }

    #[test]
    fn command_allows_advanced_report_with_entitlement() {
        // An entitled Pro account is allowed the local paid action.
        let ok = request_advanced_report(pro_snapshot()).unwrap();
        assert_eq!(ok, "advanced-report:ready");
    }

    #[test]
    fn decide_feature_denies_when_key_absent_entirely() {
        // A snapshot that does not even mention the key is denied — `allows`
        // treats a missing feature as not granted (no silent default-allow).
        let bare = snapshot(PlanTier::Free, &[]);
        assert_eq!(
            decide_feature(&bare, FeatureKey::AdvancedReports),
            Err(FeatureDenied {
                feature: FeatureKey::AdvancedReports.as_str().to_string()
            })
        );
    }

    #[test]
    fn decide_feature_uses_typed_keys_not_strings() {
        // Belt-and-suspenders: the gate is driven by the typed FeatureKey enum,
        // the same vocabulary the backend and React use. A Pro snapshot grants it.
        assert_eq!(
            decide_feature(&pro_snapshot(), FeatureKey::AdvancedReports),
            Ok(())
        );
    }
}
