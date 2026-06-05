//! Public-key license-token verification. **Desktop-safe.**
//!
//! Per ADR-0002 this crate holds only the public verifying key and can confirm
//! that a license token was signed by the backend and has not been tampered
//! with. It cannot mint tokens — there is no private key here, and the signing
//! crate is deliberately absent from the desktop dependency graph.

#![forbid(unsafe_code)]

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use shared::LicenseToken;

/// Why a license token was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    #[error("invalid verifying key length: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),
    #[error("malformed verifying key")]
    MalformedKey,
    #[error("signature does not match payload (tampered or wrong key)")]
    BadSignature,
    #[error("token payload is not a valid license token")]
    MalformedPayload,
    #[error("token expired at {expires_at}, now is {now}")]
    Expired { expires_at: u64, now: u64 },
}

/// The desktop-side license verifier. Wraps the public verifying key.
pub struct LicenseVerifier {
    verifying_key: VerifyingKey,
}

impl LicenseVerifier {
    /// Build a verifier from the 32-byte ed25519 public key the backend ships
    /// with the app.
    pub fn from_public_bytes(public: &[u8]) -> Result<Self, VerifyError> {
        let bytes: [u8; 32] = public
            .try_into()
            .map_err(|_| VerifyError::InvalidKeyLength(public.len()))?;
        let verifying_key =
            VerifyingKey::from_bytes(&bytes).map_err(|_| VerifyError::MalformedKey)?;
        Ok(Self { verifying_key })
    }

    /// Verify the signature over the payload bytes and parse the token. Does not
    /// check expiry — call [`LicenseVerifier::verify_at`] for the full check
    /// used at runtime.
    pub fn verify(
        &self,
        payload: &[u8],
        signature: &[u8; 64],
    ) -> Result<LicenseToken, VerifyError> {
        let sig = Signature::from_bytes(signature);
        self.verifying_key
            .verify(payload, &sig)
            .map_err(|_| VerifyError::BadSignature)?;
        let token: LicenseToken =
            serde_json::from_slice(payload).map_err(|_| VerifyError::MalformedPayload)?;
        Ok(token)
    }

    /// Verify signature *and* check the token has not expired as of `now` (unix
    /// epoch seconds). This is the gate the desktop uses to grant offline paid
    /// access.
    pub fn verify_at(
        &self,
        payload: &[u8],
        signature: &[u8; 64],
        now: u64,
    ) -> Result<LicenseToken, VerifyError> {
        let token = self.verify(payload, signature)?;
        if token.is_expired_at(now) {
            return Err(VerifyError::Expired {
                expires_at: token.expires_at,
                now,
            });
        }
        Ok(token)
    }
}
