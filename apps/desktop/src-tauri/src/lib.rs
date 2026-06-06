//! Tauri desktop shell — all application logic lives here (mobile-safe split).
//!
//! Authority boundary (ADR-0001/0002): this crate runs the product experience
//! and *verifies* license tokens via `license-verify`. It has no access to the
//! signing key, the database, or Stripe — those crates are not in its
//! dependency graph, so the boundary is a compile fact, not a convention.

// The shared and license-verify crates are part of the walking-skeleton
// dependency graph (ADR-0002). They are re-exported here so product code and
// gate tests can confirm the desktop tree links the verify capability — and
// only the verify capability.
pub use license_verify::{LicenseVerifier, VerifyError};
pub use shared::{Entitlements, FeatureKey, LicenseToken};

/// Local license token store: verifies and caches the short-lived token issued
/// by the backend for bounded offline paid access (issue #28).
pub mod license_store;

/// Local (Tauri-command) feature gate for one concrete paid feature (issue #30):
/// refuses a paid action when the server-resolved entitlement snapshot does not
/// grant it. UX gating in React is presentation only; this is the local guard.
pub mod feature_gate;

/// Plugged-in product modules (issue #37). The Notes sample product contributes
/// its local paid-action Tauri command here through the seam only — the desktop
/// shell merely registers the command in `invoke_handler!` below.
pub mod products;

/// Minimal IPC round-trip used by the walking-skeleton UI to confirm the
/// React <-> Tauri bridge works. Pure function so it is unit-testable without a
/// running app.
#[tauri::command]
fn ping() -> String {
    pong()
}

/// The value `ping` returns. Extracted so the behavior is testable directly.
fn pong() -> String {
    "pong".to_string()
}

/// The product-entitlements source the Notes guard fetches from (issue #59).
///
/// The Notes command no longer accepts the entitlements snapshot from the
/// frontend; it fetches it from this backend-authority source held in Tauri
/// managed state. Until the live `GET /me/product-entitlements/notes` HTTP adapter
/// lands (it needs no http-client dep in *this* module — the trait is the seam),
/// the shell registers a fail-closed placeholder so the guard DENIES rather than
/// granting on an unconfirmed entitlement. Swapping in the real adapter is a
/// one-line `.manage(...)` change with no edit to the command.
struct UnconfiguredEntitlementsSource;

impl products::notes::ProductEntitlementsSource for UnconfiguredEntitlementsSource {
    fn fetch(
        &self,
    ) -> Result<shared::ProductEntitlements, products::notes::ProductEntitlementsFetchError> {
        // No backend adapter wired yet: fail closed. The guard maps this to a
        // denial, so the local paid action is never granted without a real,
        // backend-confirmed entitlement (ADR-0001).
        Err(products::notes::ProductEntitlementsFetchError::Unreachable)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let notes_entitlements: std::sync::Arc<dyn products::notes::ProductEntitlementsSource> =
        std::sync::Arc::new(UnconfiguredEntitlementsSource);

    tauri::Builder::default()
        // The opener plugin lets the billing flow hand a backend-produced
        // checkout/portal URL to the OS default browser (issue #31). It opens
        // URLs only — it is not a billing authority and holds no Stripe secret
        // (ADR-0001/0002).
        .plugin(tauri_plugin_opener::init())
        // Issue #59: the Notes guard fetches its authoritative snapshot from this
        // managed source, not from a frontend argument. Held as the trait object
        // so the real backend adapter drops in without touching the command.
        .manage(notes_entitlements)
        .invoke_handler(tauri::generate_handler![
            ping,
            feature_gate::request_advanced_report,
            // Issue #37 — the Notes sample product's paid local action. Registered
            // additively here (one line), the only edit the product makes to the
            // desktop shell. The command is the local authority gate (ADR-0001).
            products::notes::request_publish_note
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_returns_pong() {
        assert_eq!(pong(), "pong");
    }

    #[test]
    fn desktop_links_the_verify_capability() {
        // Constructing a verifier from a public key is a compile+runtime proof
        // that the desktop tree carries `license-verify`. An invalid key length
        // is the cheapest call that exercises the type without real key material.
        // LicenseVerifier wraps a key and intentionally has no Debug, so match
        // rather than unwrap_err().
        match LicenseVerifier::from_public_bytes(&[0u8; 8]) {
            Err(VerifyError::InvalidKeyLength(8)) => {}
            Err(other) => panic!("expected InvalidKeyLength(8), got {other:?}"),
            Ok(_) => panic!("expected an error for an 8-byte key"),
        }
    }
}
