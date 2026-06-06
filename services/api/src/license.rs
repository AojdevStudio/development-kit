//! `POST /license/refresh` — short-lived license-token issuance.
//!
//! This is the backend half of the offline-paid-access scheme (ADR-0001):
//! the cloud service mints a signed, short-lived [`LicenseToken`] that the
//! desktop app verifies locally with `license-verify`. The private signing key
//! lives only here, in the backend — never in the desktop tree (ADR-0002).
//!
//! The handler is split from its pure core so the issuance logic is testable
//! without binding a socket or building a `Router`:
//!
//! - [`issue_license`] is the pure function: given a signer, a clock, a TTL and
//!   the requested account, it produces the wire response. No I/O, fully
//!   deterministic under a fixed clock.
//! - [`refresh`] is the thin Axum adapter that pulls the signer out of app state
//!   and calls [`issue_license`].

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use license_sign::LicenseSigner;
use shared::{LicenseRefreshRequest, LicenseRefreshResponse, LicenseToken};

/// How long an issued license token stays valid. Short by design: the desktop
/// must re-check in with the backend on this cadence, which is what bounds
/// offline access and lets revocation take effect (ADR-0001).
pub const DEFAULT_TOKEN_TTL_SECS: u64 = 7 * 24 * 60 * 60; // 7 days

/// Shared, cloneable handle to the backend's license signer.
///
/// `Arc` so the single signer (and its private key) is shared across all
/// request handlers without copying key material. This is the *only* place the
/// private key is reachable; it is never serialized into a response.
#[derive(Clone)]
pub struct LicenseState {
    signer: Arc<LicenseSigner>,
    ttl_secs: u64,
}

impl LicenseState {
    /// Build the license state from the backend signing key.
    pub fn new(signer: LicenseSigner) -> Self {
        Self {
            signer: Arc::new(signer),
            ttl_secs: DEFAULT_TOKEN_TTL_SECS,
        }
    }

    /// The public verifying-key bytes the desktop needs to verify issued tokens.
    /// This is the only key material that may leave the backend.
    pub fn verifying_key_bytes(&self) -> [u8; 32] {
        self.signer.verifying_key_bytes()
    }
}

/// Issue a signed, short-lived license token for `account_id`.
///
/// Pure over its inputs: `now` (unix epoch seconds) is supplied rather than
/// read from the system clock, so the function is deterministic and testable.
/// The returned response carries the Base64-encoded canonical payload bytes and
/// detached signature, plus the absolute expiry the desktop schedules against.
pub fn issue_license(
    signer: &LicenseSigner,
    account_id: &str,
    now: u64,
    ttl_secs: u64,
) -> LicenseRefreshResponse {
    let expires_at = now.saturating_add(ttl_secs);
    let token = LicenseToken {
        token_id: format!("lic_{account_id}_{now}"),
        // The authenticated user is resolved by the auth layer in a later issue;
        // until then issuance is keyed on the account the request names.
        user_id: format!("user_{account_id}"),
        account_id: account_id.to_string(),
        plan: "pro".to_string(),
        features: Default::default(),
        issued_at: now,
        expires_at,
    };

    let signed = signer.sign(&token);
    LicenseRefreshResponse {
        payload_b64: BASE64.encode(&signed.payload),
        signature_b64: BASE64.encode(signed.signature),
        expires_at,
    }
}

/// `POST /license/refresh` handler. Pulls the signer from app state, reads the
/// system clock once, and delegates to [`issue_license`].
///
/// SECURITY (issue #28 scope): this handler does not yet authenticate the
/// caller or validate that they own `account_id` — auth (`/me`, #20) and the
/// entitlement engine land separately. It MUST be mounted behind auth before a
/// production deploy; see the security note in `main.rs`. The plan/features the
/// token grants are placeholders until the entitlement engine supplies them.
pub async fn refresh(
    State(state): State<LicenseState>,
    Json(req): Json<LicenseRefreshRequest>,
) -> Json<LicenseRefreshResponse> {
    let now = now_unix_secs();
    Json(issue_license(
        &state.signer,
        &req.account_id,
        now,
        state.ttl_secs,
    ))
}

/// The current wall-clock time in unix epoch seconds. Isolated so the pure
/// issuance path never reads the clock directly.
fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Verify with the raw ed25519 primitive — NOT `license-verify`. The backend
    // crate may not depend on the desktop verifier (ADR-0002); the cross-crate
    // sign→verify round-trip is covered in `license-sign`'s integration test.
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    fn signer() -> LicenseSigner {
        LicenseSigner::from_secret_bytes(&[7u8; 32]).unwrap()
    }

    fn decode_signature(resp: &LicenseRefreshResponse) -> Signature {
        let sig_vec = BASE64.decode(&resp.signature_b64).unwrap();
        let sig: [u8; 64] = sig_vec.try_into().unwrap();
        Signature::from_bytes(&sig)
    }

    #[test]
    fn issued_token_has_a_valid_signature_for_the_signer_key() {
        let signer = signer();
        let verifying = VerifyingKey::from_bytes(&signer.verifying_key_bytes()).unwrap();

        let resp = issue_license(&signer, "acct_123", 1_700_000_000, DEFAULT_TOKEN_TTL_SECS);

        let payload = BASE64.decode(&resp.payload_b64).unwrap();
        let signature = decode_signature(&resp);

        verifying
            .verify(&payload, &signature)
            .expect("issued token must verify under the signer's public key");
    }
}
