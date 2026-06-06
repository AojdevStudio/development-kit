//! The Notes sample product — desktop Tauri-command half (issue #37, hardened #59).
//!
//! ADR-0001 requires a paid action to be refused at the Tauri-command layer too,
//! not just the screen (React is UX only). This module holds the LOCAL guard for
//! the Notes paid capability `notes.publish_note`: a pure decision over the
//! server-resolved [`ProductEntitlements`] snapshot plus the product key, and a
//! `#[tauri::command]` wrapper the desktop UI invokes before performing the local
//! publish action.
//!
//! Authority boundary (ADR-0001): the snapshot the command gates against is the
//! one the BACKEND computed. **Issue #59 hardening:** the command no longer
//! *accepts* that snapshot as an argument from the React frontend — a value the
//! frontend supplies could drift from (or lie about) the server's decision. The
//! command instead *fetches* the snapshot from a [`ProductEntitlementsSource`]
//! (the backend authority, injected as Tauri managed state), so the local
//! defense-in-depth guard reads the same value the server resolved and can never
//! be steered by a frontend-supplied one. The server `POST /notes/publish` remains
//! the real publish authority; this is hardening of the local guard only.
//!
//! This module decides nothing about *what* the account is entitled to; it only
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
/// upsell-friendly message rather than a generic failure. A fetch failure also
/// surfaces here as a denial of the same key — the guard fails *closed*, never
/// granting the action when it cannot confirm entitlement with the backend.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProductFeatureDenied {
    /// The `namespace.name` wire string of the product feature the caller lacked.
    pub feature: String,
}

impl ProductFeatureDenied {
    /// A denial of the Notes publish key — the single failure shape this guard
    /// returns whether the snapshot denied the key or could not be fetched.
    fn publish_note() -> Self {
        ProductFeatureDenied {
            feature: publish_note_key().as_str().to_string(),
        }
    }
}

/// The source of the authoritative Notes product-entitlements snapshot.
///
/// Issue #59: the local guard fetches the snapshot from here (the backend
/// authority) rather than trusting a value handed in by the React frontend.
/// Implementors talk to `GET /me/product-entitlements/notes` (or equivalent); the
/// gate logic depends only on this trait, so it stays unit-testable with a double
/// and the desktop crate needs NO http-client dependency in *this* module
/// (ADR-0002). `Send + Sync` so it can live in Tauri managed state.
pub trait ProductEntitlementsSource: Send + Sync {
    /// Fetch the backend-resolved Notes product entitlements for the current
    /// account. `Err` means the snapshot could not be obtained (offline, auth
    /// failure, backend error); the guard treats that as a denial, never a grant.
    fn fetch(&self) -> Result<ProductEntitlements, ProductEntitlementsFetchError>;
}

/// Why the authoritative product-entitlements snapshot could not be fetched. The
/// guard maps any of these to a denial (fail-closed) — the desktop never grants a
/// paid action it could not confirm with the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductEntitlementsFetchError {
    /// The backend could not be reached (offline / network error).
    Unreachable,
    /// The caller was not authenticated (no/invalid session).
    Unauthenticated,
    /// The backend returned an unexpected/error response.
    Backend(String),
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

/// The Notes publish guard over a snapshot *source*: fetch the backend-resolved
/// snapshot, then apply the pure [`decide_product_feature`]. Fails closed — a
/// fetch error becomes a denial of the publish key, never a grant.
///
/// Extracted from the `#[tauri::command]` wrapper so the fetch-then-decide flow is
/// unit-testable with a [`ProductEntitlementsSource`] double, without a running
/// Tauri app. This is where issue #59's "fetch, don't accept" lives.
pub fn publish_with_source(
    source: &dyn ProductEntitlementsSource,
) -> Result<String, ProductFeatureDenied> {
    let snapshot = source
        .fetch()
        .map_err(|_| ProductFeatureDenied::publish_note())?;
    decide_product_feature(&snapshot, &publish_note_key())?;
    // The real product would enqueue the publish (and the backend route is the
    // authority); the sample returns a deterministic payload so the allow path is
    // observable end-to-end.
    Ok("note:published".to_string())
}

/// The local Notes paid action guarded by `notes.publish_note`.
///
/// The desktop calls this Tauri command before publishing a note to the cloud.
/// **Issue #59:** the entitlements snapshot is fetched from the backend authority
/// (the [`ProductEntitlementsSource`] in Tauri managed state), NOT accepted as a
/// command argument from React — so the local guard cannot be steered by a
/// frontend-supplied value. The command refuses the action when the fetched
/// snapshot does not grant `notes.publish_note` (or cannot be fetched), and allows
/// it (returning an acknowledgement) when it does. `Err(ProductFeatureDenied)`
/// becomes a rejected promise the UI handles.
#[tauri::command]
pub fn request_publish_note(
    source: tauri::State<'_, std::sync::Arc<dyn ProductEntitlementsSource>>,
) -> Result<String, ProductFeatureDenied> {
    publish_with_source(source.inner().as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::FeatureValue;

    /// A test double standing in for the backend authority. Returns a configured
    /// snapshot (or a fetch error) so the guard's fetch-then-decide flow is
    /// exercised without a running app or a real network — the desktop never
    /// authors entitlement *values*, it only reads what this source yields.
    struct FakeSource(Result<ProductEntitlements, ProductEntitlementsFetchError>);

    impl ProductEntitlementsSource for FakeSource {
        fn fetch(&self) -> Result<ProductEntitlements, ProductEntitlementsFetchError> {
            self.0.clone()
        }
    }

    /// A server-shaped Notes snapshot granting (or not) the publish key, built the
    /// way the desktop would receive it from the backend.
    fn snapshot(granted: bool) -> ProductEntitlements {
        ProductEntitlements::new("acct_test", NAMESPACE)
            .with(publish_note_key(), FeatureValue::Enabled(granted))
    }

    fn granting_source() -> FakeSource {
        FakeSource(Ok(snapshot(true)))
    }

    fn denying_source() -> FakeSource {
        FakeSource(Ok(snapshot(false)))
    }

    // --- ISC-23: a source granting the key → allow ---
    #[test]
    fn allows_publish_when_the_fetched_snapshot_grants_it() {
        let ok = publish_with_source(&granting_source()).unwrap();
        assert_eq!(ok, "note:published");
    }

    // --- ISC-24: a source NOT granting the key → deny with the key named ---
    #[test]
    fn denies_publish_when_the_fetched_snapshot_lacks_the_entitlement() {
        // The whole point of the command gate: an unentitled account is refused the
        // local paid action — the screen alone does not protect it.
        let denied = publish_with_source(&denying_source()).unwrap_err();
        assert_eq!(denied.feature, publish_note_key().as_str());
    }

    // --- ISC-25: a fetch failure → deny (fail closed), never default-allow ---
    #[test]
    fn fails_closed_when_the_backend_snapshot_cannot_be_fetched() {
        for err in [
            ProductEntitlementsFetchError::Unreachable,
            ProductEntitlementsFetchError::Unauthenticated,
            ProductEntitlementsFetchError::Backend("500".into()),
        ] {
            let source = FakeSource(Err(err.clone()));
            let denied = publish_with_source(&source)
                .expect_err("a guard that cannot confirm entitlement must DENY, not grant");
            assert_eq!(
                denied.feature,
                publish_note_key().as_str(),
                "fail-closed denial names the publish key (fetch error: {err:?})"
            );
        }
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

    #[test]
    fn decide_allows_when_key_granted() {
        // The pure guard's allow path is unchanged by #59.
        assert_eq!(
            decide_product_feature(&snapshot(true), &publish_note_key()),
            Ok(())
        );
    }
}
