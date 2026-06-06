//! The Notes sample product — desktop Tauri-command half (issue #37).
//!
//! ADR-0001 requires a paid action to be refused at the Tauri-command layer too,
//! not just the screen (React is UX only). This module holds the LOCAL guard for
//! the Notes paid capability `notes.publish_note`: a pure decision over the
//! server-resolved [`ProductEntitlements`] snapshot plus the product key, and a
//! `#[tauri::command]` wrapper the desktop UI invokes before performing the local
//! publish action.
//!
//! Authority boundary (ADR-0001): the snapshot the command gates against is the
//! one the BACKEND computed and the desktop *fetched* — the desktop never
//! constructs an authoritative product-entitlements snapshot of its own. This
//! module decides nothing about *what* the account is entitled to; it only
//! enforces the entitlement the backend already decided, using the same product
//! key the React guard and the backend gate use. The free local actions (create /
//! list notes) live in `local-store`; only the paid publish action is gated here.

use shared::{ProductEntitlements, ProductFeatureKey};

/// The Notes product namespace — matches the backend and local halves.
pub const NAMESPACE: &str = "notes";

/// The Notes paid capability key: `notes.publish_note`. Built through the
/// validated constructor so it is always the well-formed `namespace.name` key.
pub fn publish_note_key() -> ProductFeatureKey {
    ProductFeatureKey::new(NAMESPACE, "publish_note").expect("valid notes product key")
}

/// Why a local Notes paid action was refused at the command layer.
///
/// Carries the product feature key that was denied so the UI can show a precise,
/// upsell-friendly message rather than a generic failure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProductFeatureDenied {
    /// The `namespace.name` wire string of the product feature the caller lacked.
    pub feature: String,
}

/// Decide whether `entitlements` permit the `key`-gated local product action.
///
/// Pure over the server-resolved product snapshot and the typed product key, so
/// it is unit testable without a running app. Mirrors the backend
/// `require_product_feature`: it asks the same question ("does this account have
/// this product key?") against the same `allows` semantics, so the local guard can
/// never drift from the server's decision.
pub fn decide_product_feature(
    entitlements: &ProductEntitlements,
    key: &ProductFeatureKey,
) -> Result<(), ProductFeatureDenied> {
    if entitlements.allows(key) {
        Ok(())
    } else {
        Err(ProductFeatureDenied {
            feature: key.as_str().to_string(),
        })
    }
}

/// The local Notes paid action guarded by `notes.publish_note`.
///
/// The desktop calls this Tauri command before publishing a note to the cloud.
/// `entitlements` is the Notes product snapshot the desktop fetched from the
/// backend authority; the command refuses the action when that snapshot does not
/// grant `notes.publish_note`, and allows it (returning an acknowledgement) when it
/// does. `Err(ProductFeatureDenied)` becomes a rejected promise the UI handles.
#[tauri::command]
pub fn request_publish_note(
    entitlements: ProductEntitlements,
) -> Result<String, ProductFeatureDenied> {
    decide_product_feature(&entitlements, &publish_note_key())?;
    // The real product would enqueue the publish (and the backend route is the
    // authority); the sample returns a deterministic payload so the allow path is
    // observable end-to-end.
    Ok("note:published".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::FeatureValue;

    /// A server-shaped Notes snapshot granting (or not) the publish key, built the
    /// way the desktop would receive it from the backend — the desktop never
    /// authors entitlement *values*, it only reads them.
    fn snapshot(granted: bool) -> ProductEntitlements {
        ProductEntitlements::new("acct_test", NAMESPACE)
            .with(publish_note_key(), FeatureValue::Enabled(granted))
    }

    #[test]
    fn command_denies_publish_without_entitlement() {
        // The whole point of the command gate: an unentitled account is refused the
        // local paid action — the screen alone does not protect it.
        let denied = request_publish_note(snapshot(false)).unwrap_err();
        assert_eq!(denied.feature, publish_note_key().as_str());
    }

    #[test]
    fn command_allows_publish_with_entitlement() {
        // An entitled account is allowed the local paid action.
        let ok = request_publish_note(snapshot(true)).unwrap();
        assert_eq!(ok, "note:published");
    }

    #[test]
    fn decide_denies_when_key_absent_entirely() {
        // A snapshot that does not even mention the key is denied — `allows` treats
        // a missing product key as not granted (no silent default-allow).
        let bare = ProductEntitlements::new("acct_test", NAMESPACE);
        assert_eq!(
            decide_product_feature(&bare, &publish_note_key()),
            Err(ProductFeatureDenied {
                feature: publish_note_key().as_str().to_string()
            })
        );
    }
}
