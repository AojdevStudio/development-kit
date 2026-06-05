//! Desktop-side license store: hold and verify a short-lived license token.
//!
//! The backend issues a signed token via `POST /license/refresh`; the desktop
//! receives a [`LicenseRefreshResponse`], verifies it locally, and caches the
//! verified [`LicenseToken`] to permit bounded offline paid access (ADR-0001).
//!
//! Verification is the *only* license authority the desktop has: it confirms a
//! token was signed by the backend and has not expired. It cannot mint a token —
//! there is no private key here, and `license-sign` is absent from the desktop
//! dependency graph (ADR-0002). That absence is a compile fact, not a convention.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use license_verify::{LicenseVerifier, VerifyError};
use shared::{LicenseRefreshResponse, LicenseToken};

/// Why a refresh response could not be accepted into the store.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LicenseStoreError {
    /// The Base64 payload or signature field was not valid Base64.
    #[error("malformed base64 in license response field `{field}`")]
    MalformedEncoding { field: &'static str },
    /// The signature field decoded to the wrong number of bytes (not 64).
    #[error("signature is {got} bytes, expected 64")]
    BadSignatureLength { got: usize },
    /// The token failed cryptographic verification or the expiry check. Wraps
    /// the underlying [`VerifyError`] so callers can distinguish a tampered
    /// token from an expired one.
    #[error(transparent)]
    Verify(#[from] VerifyError),
}

/// Holds the backend's public verifying key and the most recently accepted
/// license token. The store starts empty; a token only lands here after it has
/// been cryptographically verified and found unexpired.
pub struct LicenseStore {
    verifier: LicenseVerifier,
    current: Option<LicenseToken>,
}

impl LicenseStore {
    /// Build a store from the backend's 32-byte ed25519 public key. This key
    /// ships with the app; it is the only license key material on the desktop.
    pub fn from_public_bytes(public: &[u8]) -> Result<Self, LicenseStoreError> {
        let verifier = LicenseVerifier::from_public_bytes(public)?;
        Ok(Self {
            verifier,
            current: None,
        })
    }

    /// Accept a `/license/refresh` response: decode it, verify its signature and
    /// expiry as of `now` (unix epoch seconds), and—only on success—cache the
    /// verified token. On any failure the previously stored token is left
    /// untouched, so a bad refresh never erases a still-valid offline license.
    pub fn accept(
        &mut self,
        response: &LicenseRefreshResponse,
        now: u64,
    ) -> Result<&LicenseToken, LicenseStoreError> {
        let payload = decode(&response.payload_b64, "payload")?;
        let signature = decode_signature(&response.signature_b64)?;

        let token = self.verifier.verify_at(&payload, &signature, now)?;
        self.current = Some(token);
        // `current` was just set to `Some`, so this expect cannot fire.
        Ok(self.current.as_ref().expect("token was just stored above"))
    }

    /// The currently cached verified token, if one has been accepted.
    pub fn current(&self) -> Option<&LicenseToken> {
        self.current.as_ref()
    }

    /// Whether the cached token is present and still unexpired as of `now`.
    /// This is the gate the desktop asks before granting offline paid access.
    pub fn is_valid_at(&self, now: u64) -> bool {
        self.current.as_ref().is_some_and(|t| !t.is_expired_at(now))
    }
}

/// Decode a Base64 field, attributing failures to the named field for clear
/// errors.
fn decode(value: &str, field: &'static str) -> Result<Vec<u8>, LicenseStoreError> {
    BASE64
        .decode(value)
        .map_err(|_| LicenseStoreError::MalformedEncoding { field })
}

/// Decode the Base64 signature and confirm it is exactly 64 bytes.
fn decode_signature(value: &str) -> Result<[u8; 64], LicenseStoreError> {
    let bytes = decode(value, "signature")?;
    let got = bytes.len();
    bytes
        .try_into()
        .map_err(|_| LicenseStoreError::BadSignatureLength { got })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pre-signed test vectors produced by the *backend* (`license-sign` with
    // seed [5u8; 32]). They are embedded as constants on purpose: the desktop
    // tree must hold NO signing capability — not `license-sign`, not even the
    // raw ed25519 signing primitive (ADR-0002). The desktop can only verify, so
    // its tests verify pre-issued fixtures rather than minting their own. The
    // backend→desktop sign→verify chain with live crates is proven separately in
    // `crates/license-sign/tests/sign_verify_roundtrip.rs`.
    //
    // To regenerate after a token-shape or seed change: sign the same token in
    // the backend (`license-sign`, seed [5u8; 32]) and Base64-encode the
    // resulting payload + signature — the same path `api::license::issue_license`
    // takes. The desktop must never gain that signing path; it only verifies.

    /// Public verifying key for backend seed `[5u8; 32]`.
    const BACKEND_PUBLIC_KEY: [u8; 32] = [
        110, 122, 28, 221, 41, 176, 183, 143, 209, 58, 244, 197, 89, 143, 239, 244, 239, 42, 151,
        22, 110, 60, 166, 242, 228, 251, 252, 205, 128, 80, 91, 241,
    ];

    /// A different, unrelated valid ed25519 public key (the verifying key for
    /// seed `[7u8; 32]`) — used to prove a wrong trust anchor rejects the token.
    const OTHER_PUBLIC_KEY: [u8; 32] = [
        234, 74, 108, 99, 226, 156, 82, 10, 190, 245, 80, 123, 19, 46, 197, 249, 149, 71, 118, 174,
        190, 190, 123, 146, 66, 30, 234, 105, 20, 70, 210, 44,
    ];

    /// Fixture token: account `acct_1`, issued 1000, expires 2000.
    const TOKEN_A_PAYLOAD_B64: &str = "eyJ0b2tlbl9pZCI6ImxpY190ZXN0IiwidXNlcl9pZCI6InVzZXJfdGVzdCIsImFjY291bnRfaWQiOiJhY2N0XzEiLCJwbGFuIjoicHJvIiwiZmVhdHVyZXMiOnsiZXhwb3J0X3BkZiI6dHJ1ZX0sImlzc3VlZF9hdCI6MTAwMCwiZXhwaXJlc19hdCI6MjAwMH0=";
    const TOKEN_A_SIG_B64: &str =
        "/JRKtk+E0d4QxdhMwd2iFyrvbtaZ6ImejpreE831WJOdr1fd9w/s/95TRKgBIgiGLeSzWeiq6G1o2WY1XVmsDA==";

    /// Fixture token: account `acct_1`, issued 1000, expires 5000.
    const TOKEN_B_PAYLOAD_B64: &str = "eyJ0b2tlbl9pZCI6ImxpY190ZXN0IiwidXNlcl9pZCI6InVzZXJfdGVzdCIsImFjY291bnRfaWQiOiJhY2N0XzEiLCJwbGFuIjoicHJvIiwiZmVhdHVyZXMiOnsiZXhwb3J0X3BkZiI6dHJ1ZX0sImlzc3VlZF9hdCI6MTAwMCwiZXhwaXJlc19hdCI6NTAwMH0=";
    const TOKEN_B_SIG_B64: &str =
        "WPBTnBhvB2c5l8LELL6u/EWpduKJYI1z8je94t3IXM5gukelgV4d73I5FmTW34C3Euqx8/dzdmkUR4+0A2AwAw==";

    fn token_a(expires_at: u64) -> LicenseRefreshResponse {
        LicenseRefreshResponse {
            payload_b64: TOKEN_A_PAYLOAD_B64.into(),
            signature_b64: TOKEN_A_SIG_B64.into(),
            expires_at,
        }
    }

    fn token_b(expires_at: u64) -> LicenseRefreshResponse {
        LicenseRefreshResponse {
            payload_b64: TOKEN_B_PAYLOAD_B64.into(),
            signature_b64: TOKEN_B_SIG_B64.into(),
            expires_at,
        }
    }

    fn store() -> LicenseStore {
        LicenseStore::from_public_bytes(&BACKEND_PUBLIC_KEY).unwrap()
    }

    #[test]
    fn accepts_and_caches_a_valid_token() {
        let mut store = store();

        let token = store.accept(&token_a(2_000), 1_500).unwrap();

        assert_eq!(token.account_id, "acct_1");
        assert_eq!(store.current().unwrap().account_id, "acct_1");
        assert!(store.is_valid_at(1_500));
    }

    #[test]
    fn rejects_an_expired_token() {
        let mut store = store();

        // The clock is past expiry: a perfectly-signed token must still be refused.
        let err = store.accept(&token_a(2_000), 2_000).unwrap_err();

        assert_eq!(
            err,
            LicenseStoreError::Verify(VerifyError::Expired {
                expires_at: 2_000,
                now: 2_000,
            })
        );
        // Nothing was cached — an expired refresh does not grant access.
        assert!(store.current().is_none());
        assert!(!store.is_valid_at(2_000));
    }

    #[test]
    fn rejects_a_tampered_payload() {
        let mut store = store();

        // Flip one byte of the signed payload: the signature no longer matches.
        let mut resp = token_a(2_000);
        let mut payload = BASE64.decode(&resp.payload_b64).unwrap();
        payload[0] ^= 0xFF;
        resp.payload_b64 = BASE64.encode(&payload);

        let err = store.accept(&resp, 1_500).unwrap_err();
        assert_eq!(err, LicenseStoreError::Verify(VerifyError::BadSignature));
        assert!(store.current().is_none());
    }

    #[test]
    fn rejects_a_token_signed_by_a_different_key() {
        // A store trusting a *different* public key than the one that signed the
        // fixture must reject it.
        let mut store = LicenseStore::from_public_bytes(&OTHER_PUBLIC_KEY).unwrap();

        let err = store.accept(&token_a(2_000), 1_500).unwrap_err();

        assert_eq!(err, LicenseStoreError::Verify(VerifyError::BadSignature));
        assert!(store.current().is_none());
    }

    #[test]
    fn rejects_malformed_base64() {
        let mut store = store();

        let mut resp = token_a(2_000);
        resp.signature_b64 = "not valid base64 !!!".into();

        let err = store.accept(&resp, 1_500).unwrap_err();
        assert_eq!(
            err,
            LicenseStoreError::MalformedEncoding { field: "signature" }
        );
    }

    #[test]
    fn rejects_a_signature_of_the_wrong_length() {
        let mut store = store();

        let mut resp = token_a(2_000);
        // A valid Base64 string that decodes to fewer than 64 bytes.
        resp.signature_b64 = BASE64.encode([0u8; 10]);

        let err = store.accept(&resp, 1_500).unwrap_err();
        assert_eq!(err, LicenseStoreError::BadSignatureLength { got: 10 });
    }

    #[test]
    fn a_failed_refresh_leaves_a_previously_valid_token_intact() {
        let mut store = store();

        // First, accept a good token (expires 5000).
        store.accept(&token_b(5_000), 1_500).unwrap();
        assert!(store.is_valid_at(1_500));

        // A subsequent tampered refresh fails — but must not erase the cached one.
        let mut bad = token_b(5_000);
        let mut payload = BASE64.decode(&bad.payload_b64).unwrap();
        payload[0] ^= 0xFF;
        bad.payload_b64 = BASE64.encode(&payload);

        assert!(store.accept(&bad, 1_600).is_err());
        // Still holding the original valid token.
        assert_eq!(store.current().unwrap().account_id, "acct_1");
        assert!(store.is_valid_at(1_600));
    }
}
