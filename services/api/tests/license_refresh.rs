//! Behavior: `POST /license/refresh` issues a signed, short-lived token.
//!
//! Driven through the real router via `tower::ServiceExt::oneshot` — a real
//! request flows through the real handler, no socket bound. This is the test
//! behind the "Backend issues a signed short-lived token via /license/refresh"
//! acceptance criterion (issue #28).
//!
//! The cross-crate sign→verify round-trip (backend signs, desktop verifies) is
//! proven in `license-sign`'s integration test; here we assert the HTTP surface
//! and that the issued token's signature checks out under the backend's public
//! key, using the raw ed25519 primitive (the API crate must not depend on the
//! desktop verifier — ADR-0002).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use http_body_util::BodyExt;
use license_sign::LicenseSigner;
use shared::{LicenseRefreshResponse, LicenseToken};
use tower::ServiceExt;

use api::license::LicenseState;

fn state() -> LicenseState {
    LicenseState::new(LicenseSigner::from_secret_bytes(&[3u8; 32]).unwrap())
}

async fn post_refresh(state: LicenseState, account_id: &str) -> (StatusCode, Vec<u8>) {
    let app = api::app_with_license(state);
    let body = format!(r#"{{"account_id":"{account_id}"}}"#);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/license/refresh")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn refresh_returns_200_with_a_signed_token_response() {
    let (status, bytes) = post_refresh(state(), "acct_xyz").await;
    assert_eq!(status, StatusCode::OK);

    let resp: LicenseRefreshResponse = serde_json::from_slice(&bytes).unwrap();
    // The response carries a non-empty Base64 payload and a 64-byte signature.
    assert!(!resp.payload_b64.is_empty());
    let sig = BASE64.decode(&resp.signature_b64).unwrap();
    assert_eq!(sig.len(), 64, "ed25519 signature is 64 bytes");
    // The token is short-lived: it expires in the future.
    assert!(resp.expires_at > 0);
}

#[tokio::test]
async fn issued_token_is_signed_by_the_backend_key_and_names_the_account() {
    let state = state();
    let verifying = VerifyingKey::from_bytes(&state.verifying_key_bytes()).unwrap();

    let (_status, bytes) = post_refresh(state, "acct_xyz").await;
    let resp: LicenseRefreshResponse = serde_json::from_slice(&bytes).unwrap();

    let payload = BASE64.decode(&resp.payload_b64).unwrap();
    let sig_bytes: [u8; 64] = BASE64
        .decode(&resp.signature_b64)
        .unwrap()
        .try_into()
        .unwrap();
    let signature = Signature::from_bytes(&sig_bytes);

    // The signature verifies under the backend's public key.
    verifying
        .verify(&payload, &signature)
        .expect("issued token verifies under backend public key");

    // The payload is the requested account's token, and expiry is consistent.
    let token: LicenseToken = serde_json::from_slice(&payload).unwrap();
    assert_eq!(token.account_id, "acct_xyz");
    assert_eq!(token.expires_at, resp.expires_at);
    assert!(token.expires_at > token.issued_at, "token is short-lived");
}
