//! Cross-crate behavior: the backend signs, the desktop verifies.
//!
//! This is the round-trip that proves the capability split is real — the
//! signing crate (backend) and the verifying crate (desktop) agree on the wire
//! format and the ed25519 scheme. `license-sign` carries `license-verify` only
//! as a dev-dependency, so the production dependency direction stays one-way.

use std::collections::BTreeMap;

use license_sign::LicenseSigner;
use license_verify::{LicenseVerifier, VerifyError};
use shared::{FeatureKey, FeatureValue, LicenseToken};

fn token(expires_at: u64) -> LicenseToken {
    let mut features = BTreeMap::new();
    features.insert(FeatureKey::ExportPdf, FeatureValue::Enabled(true));
    features.insert(FeatureKey::MaxProjects, FeatureValue::Limit(100));
    LicenseToken {
        token_id: "lic_roundtrip".into(),
        user_id: "user_1".into(),
        account_id: "acct_1".into(),
        plan: "pro".into(),
        features,
        issued_at: 1_700_000_000,
        expires_at,
    }
}

fn signer() -> LicenseSigner {
    LicenseSigner::from_secret_bytes(&[42u8; 32]).unwrap()
}

#[test]
fn desktop_verifies_a_token_the_backend_signed() {
    let signer = signer();
    let verifier = LicenseVerifier::from_public_bytes(&signer.verifying_key_bytes()).unwrap();

    let original = token(1_700_604_800);
    let signed = signer.sign(&original);

    let verified = verifier.verify(&signed.payload, &signed.signature).unwrap();
    assert_eq!(verified, original);
}

#[test]
fn desktop_rejects_a_tampered_payload() {
    let signer = signer();
    let verifier = LicenseVerifier::from_public_bytes(&signer.verifying_key_bytes()).unwrap();

    let signed = signer.sign(&token(1_700_604_800));

    // Flip a byte in the signed payload: the signature must no longer match.
    let mut tampered = signed.payload.clone();
    tampered[0] ^= 0xFF;

    let err = verifier.verify(&tampered, &signed.signature).unwrap_err();
    assert_eq!(err, VerifyError::BadSignature);
}

#[test]
fn desktop_rejects_a_token_signed_by_a_different_key() {
    let real = signer();
    let attacker = LicenseSigner::from_secret_bytes(&[99u8; 32]).unwrap();
    let verifier = LicenseVerifier::from_public_bytes(&real.verifying_key_bytes()).unwrap();

    let forged = attacker.sign(&token(1_700_604_800));

    let err = verifier
        .verify(&forged.payload, &forged.signature)
        .unwrap_err();
    assert_eq!(err, VerifyError::BadSignature);
}

#[test]
fn desktop_rejects_an_expired_token() {
    let signer = signer();
    let verifier = LicenseVerifier::from_public_bytes(&signer.verifying_key_bytes()).unwrap();

    let expires_at = 1_700_604_800;
    let signed = signer.sign(&token(expires_at));

    // A valid signature, but the clock is past expiry.
    let err = verifier
        .verify_at(&signed.payload, &signed.signature, expires_at + 1)
        .unwrap_err();
    assert_eq!(
        err,
        VerifyError::Expired {
            expires_at,
            now: expires_at + 1
        }
    );
}

#[test]
fn valid_unexpired_token_passes_the_full_runtime_check() {
    let signer = signer();
    let verifier = LicenseVerifier::from_public_bytes(&signer.verifying_key_bytes()).unwrap();

    let expires_at = 1_700_604_800;
    let signed = signer.sign(&token(expires_at));

    let verified = verifier
        .verify_at(&signed.payload, &signed.signature, expires_at - 1)
        .unwrap();
    assert_eq!(verified.token_id, "lic_roundtrip");
}
