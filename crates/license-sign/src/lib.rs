//! Private-key license-token signing. **Backend-only.**
//!
//! Per ADR-0002 this crate holds the private-key half of the ed25519 scheme and
//! must never appear in the desktop dependency graph. "Desktop can only verify,
//! never issue" is enforced as a compile fact: `apps/desktop` depends on
//! `license-verify` only, so this crate cannot be linked there.

#![forbid(unsafe_code)]

use ed25519_dalek::{Signer, SigningKey};
use shared::LicenseToken;

/// A signed license token: the canonical payload bytes plus the detached
/// ed25519 signature over them. The verifier re-derives the bytes from the
/// payload and checks the signature.
#[derive(Debug, Clone)]
pub struct SignedLicense {
    /// The canonical signed bytes (`LicenseToken::signing_bytes`).
    pub payload: Vec<u8>,
    /// The 64-byte ed25519 signature over `payload`.
    pub signature: [u8; 64],
}

/// Errors raised while signing.
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("invalid signing key length: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),
}

/// The cloud-side license signer. Wraps the ed25519 private key; only the
/// backend ever constructs one.
pub struct LicenseSigner {
    signing_key: SigningKey,
}

impl LicenseSigner {
    /// Build a signer from a 32-byte ed25519 secret key seed.
    pub fn from_secret_bytes(secret: &[u8]) -> Result<Self, SignError> {
        let seed: [u8; 32] = secret
            .try_into()
            .map_err(|_| SignError::InvalidKeyLength(secret.len()))?;
        Ok(Self {
            signing_key: SigningKey::from_bytes(&seed),
        })
    }

    /// Wrap an existing `SigningKey` (e.g. one freshly generated in tests or at
    /// backend startup).
    pub fn new(signing_key: SigningKey) -> Self {
        Self { signing_key }
    }

    /// The public verifying key bytes the desktop side needs. This is the only
    /// key material that may leave the backend.
    pub fn verifying_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Sign a license token, producing the detached signature over its canonical
    /// bytes.
    pub fn sign(&self, token: &LicenseToken) -> SignedLicense {
        let payload = token.signing_bytes();
        let signature = self.signing_key.sign(&payload).to_bytes();
        SignedLicense { payload, signature }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample_token() -> LicenseToken {
        LicenseToken {
            token_id: "lic_1".into(),
            user_id: "user_1".into(),
            account_id: "acct_1".into(),
            plan: "pro".into(),
            features: BTreeMap::new(),
            issued_at: 1_700_000_000,
            expires_at: 1_700_604_800,
        }
    }

    #[test]
    fn from_secret_bytes_rejects_wrong_length() {
        // LicenseSigner intentionally does not derive Debug (it wraps a private
        // key), so match on the error rather than unwrap_err().
        match LicenseSigner::from_secret_bytes(&[0u8; 16]) {
            Err(SignError::InvalidKeyLength(16)) => {}
            other => panic!("expected InvalidKeyLength(16), got {:?}", other.err()),
        }
    }

    #[test]
    fn sign_produces_payload_matching_token_signing_bytes() {
        let signer = LicenseSigner::from_secret_bytes(&[7u8; 32]).unwrap();
        let token = sample_token();
        let signed = signer.sign(&token);
        assert_eq!(signed.payload, token.signing_bytes());
    }

    #[test]
    fn deterministic_key_yields_stable_verifying_key() {
        let a = LicenseSigner::from_secret_bytes(&[7u8; 32]).unwrap();
        let b = LicenseSigner::from_secret_bytes(&[7u8; 32]).unwrap();
        assert_eq!(a.verifying_key_bytes(), b.verifying_key_bytes());
    }
}
