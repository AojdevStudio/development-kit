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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ping])
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
