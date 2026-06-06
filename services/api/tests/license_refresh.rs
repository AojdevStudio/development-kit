//! Behavior: `POST /license/refresh` issues a signed, short-lived token for the
//! AUTHENTICATED caller — never for a body-supplied account, never a hardcoded
//! plan (issue #56).
//!
//! Driven through the real router via `tower::ServiceExt::oneshot` — a real
//! request flows through real auth extraction, the real entitlement engine, and
//! the real signer, no socket bound. This is the test behind the hardened
//! acceptance criterion: the backend decides the account (from the token) and the
//! plan (from `resolve_entitlements`), so a free account can never obtain a Pro
//! token and a caller can never mint a token for an account it does not own.
//!
//! The cross-crate sign→verify round-trip is proven in `license-sign`'s
//! integration test; here we assert the HTTP/authority surface and that the
//! issued token's signature checks out under the backend's public key, using the
//! raw ed25519 primitive (the API crate must not depend on the desktop verifier —
//! ADR-0002).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use http_body_util::BodyExt;
use license_sign::LicenseSigner;
use shared::{
    FeatureKey, FeatureValue, LicenseRefreshResponse, LicenseToken, PlanTier, SubscriptionStatus,
};
use tower::ServiceExt;

use api::entitlement::{AccountState, InMemoryAccountStateStore};
use api::license::LicenseState;
use api::principal::{Principal, Role};
use api::store::{InMemoryPrincipalStore, DEV_TOKEN};

/// A second dev token bound to a FREE account, so the same router serves both an
/// entitled (Pro) and an unentitled (free) caller through real auth.
const FREE_TOKEN: &str = "tok_free";
const FREE_ACCOUNT: &str = "acct_free";

/// The license state under test: the dev principal (`DEV_TOKEN` → `acct_acme`)
/// seeded to active Pro, plus a free principal (`FREE_TOKEN` → `acct_free`) seeded
/// to free. Both flow through the real authenticated `/license/refresh`.
fn state() -> LicenseState {
    let principals = InMemoryPrincipalStore::dev_seed().with_token(FREE_TOKEN, free_principal());
    let accounts = InMemoryAccountStateStore::dev_seed().with_account(
        FREE_ACCOUNT,
        AccountState {
            plan: PlanTier::Free,
            status: SubscriptionStatus::Free,
            trial: false,
            current_period_end: None,
        },
    );
    LicenseState::new(
        LicenseSigner::from_secret_bytes(&[3u8; 32]).unwrap(),
        Arc::new(principals),
        Arc::new(accounts),
    )
}

fn free_principal() -> Principal {
    Principal {
        user_id: "user_free".into(),
        email: "free@example.com".into(),
        account_id: FREE_ACCOUNT.into(),
        account_name: "Free Co".into(),
        role: Role::Owner,
    }
}

/// POST `/license/refresh` with the given auth header and optional body, returning
/// the status and raw body bytes.
async fn post_refresh(
    state: LicenseState,
    auth: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let app = api::app_with_license(state);
    let mut builder = Request::builder()
        .method("POST")
        .uri("/license/refresh")
        .header("content-type", "application/json");
    if let Some(value) = auth {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    let body = body.map(str::to_string).unwrap_or_else(|| "{}".to_string());
    let response = app
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

fn decode_token(bytes: &[u8]) -> LicenseToken {
    let resp: LicenseRefreshResponse = serde_json::from_slice(bytes).unwrap();
    let payload = BASE64.decode(&resp.payload_b64).unwrap();
    serde_json::from_slice(&payload).unwrap()
}

/// Whether the token grants a boolean-enabled feature (same semantics as
/// `Entitlements::allows` for the on/off keys this suite checks).
fn token_enables(token: &LicenseToken, key: FeatureKey) -> bool {
    matches!(token.features.get(&key), Some(FeatureValue::Enabled(true)))
}

// --- ISC-1: auth is required; no principal is read from the body ---

#[tokio::test]
async fn refresh_without_a_token_is_unauthorized() {
    let (status, _) = post_refresh(state(), None, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "no bearer token → 401; the body can never name the principal"
    );
}

#[tokio::test]
async fn refresh_with_an_unknown_token_is_unauthorized() {
    let (status, _) = post_refresh(state(), Some("Bearer tok_nope"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "unknown token → 401");
}

// --- ISC-2/ISC-3: account_id comes from the principal, never the body ---

#[tokio::test]
async fn issued_token_names_the_authenticated_account_not_the_body() {
    // The body names a DIFFERENT account than the token's principal. The issued
    // token must name the principal's account (acct_acme), ignoring the body.
    let lying_body = r#"{"account_id":"acct_victim"}"#;
    let (status, bytes) = post_refresh(
        state(),
        Some(&format!("Bearer {DEV_TOKEN}")),
        Some(lying_body),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = decode_token(&bytes);
    assert_eq!(
        token.account_id, "acct_acme",
        "account_id must come from the authenticated principal, not the body"
    );
    assert_ne!(
        token.account_id, "acct_victim",
        "a body-supplied account_id must never become the token's account"
    );
}

// --- ISC-2: the issued token is signed by the backend key ---

#[tokio::test]
async fn issued_token_is_signed_by_the_backend_key() {
    let state = state();
    let verifying = VerifyingKey::from_bytes(&state.verifying_key_bytes()).unwrap();
    let (status, bytes) = post_refresh(state, Some(&format!("Bearer {DEV_TOKEN}")), None).await;
    assert_eq!(status, StatusCode::OK);

    let resp: LicenseRefreshResponse = serde_json::from_slice(&bytes).unwrap();
    let payload = BASE64.decode(&resp.payload_b64).unwrap();
    let sig_bytes: [u8; 64] = BASE64
        .decode(&resp.signature_b64)
        .unwrap()
        .try_into()
        .unwrap();
    verifying
        .verify(&payload, &Signature::from_bytes(&sig_bytes))
        .expect("issued token verifies under backend public key");
}

// --- ISC-4: a FREE account cannot get a Pro token ---

#[tokio::test]
async fn free_account_receives_a_free_token_not_a_pro_one() {
    let (status, bytes) = post_refresh(state(), Some(&format!("Bearer {FREE_TOKEN}")), None).await;
    assert_eq!(status, StatusCode::OK);
    let token = decode_token(&bytes);
    assert_eq!(token.account_id, FREE_ACCOUNT);
    assert_eq!(
        token.plan, "free",
        "a free account's token plan must be resolved as free, never hardcoded pro"
    );
    // The Pro-only feature must NOT be enabled in a free account's token.
    assert!(
        !token_enables(&token, FeatureKey::AdvancedReports),
        "a free account's token must not enable advanced_reports"
    );
}

// --- ISC-5: a PRO account gets a Pro token from the engine ---

#[tokio::test]
async fn pro_account_receives_a_pro_token_resolved_from_the_engine() {
    let (status, bytes) = post_refresh(state(), Some(&format!("Bearer {DEV_TOKEN}")), None).await;
    assert_eq!(status, StatusCode::OK);
    let token = decode_token(&bytes);
    assert_eq!(token.plan, "pro", "active Pro account → pro plan");
    assert!(
        token_enables(&token, FeatureKey::AdvancedReports),
        "a Pro account's token enables advanced_reports, resolved by the engine"
    );
    // Short-lived: it expires in the future, after it was issued.
    assert!(token.expires_at > token.issued_at);
}

// --- ISC-6: an authenticated account with no billing state is not granted Pro ---

#[tokio::test]
async fn account_without_billing_state_is_not_granted_a_paid_token() {
    // A principal whose account has NO billing state on record. The engine has
    // nothing to grant, so the token must not enable paid features.
    const NOBILL_TOKEN: &str = "tok_nobill";
    let principals = InMemoryPrincipalStore::dev_seed().with_token(
        NOBILL_TOKEN,
        Principal {
            user_id: "user_nb".into(),
            email: "nb@example.com".into(),
            account_id: "acct_nobill".into(),
            account_name: "NoBill".into(),
            role: Role::Owner,
        },
    );
    let accounts = InMemoryAccountStateStore::dev_seed(); // acct_nobill absent
    let state = LicenseState::new(
        LicenseSigner::from_secret_bytes(&[5u8; 32]).unwrap(),
        Arc::new(principals),
        Arc::new(accounts),
    );
    let (status, bytes) = post_refresh(state, Some(&format!("Bearer {NOBILL_TOKEN}")), None).await;
    // Either denied outright, or issued a strictly-free token — never Pro.
    if status == StatusCode::OK {
        let token = decode_token(&bytes);
        assert_ne!(
            token.plan, "pro",
            "no billing state must never resolve to pro"
        );
        assert!(
            !token_enables(&token, FeatureKey::AdvancedReports),
            "no billing state must not enable a paid feature"
        );
    } else {
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "no billing state → free token or 403, never a paid token"
        );
    }
}
