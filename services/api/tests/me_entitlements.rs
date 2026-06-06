//! Behavior: `GET /me/entitlements` returns the backend-computed entitlements for
//! an authenticated caller, rejects unauthenticated ones, and 404s when the
//! account has no billing state (issue #29 acceptance criteria).
//!
//! Exercised through the real router via `tower::ServiceExt::oneshot` — real
//! requests flow through the real auth extraction, the real entitlement engine,
//! and the real handler, no socket bound. Authority is server-side (ADR-0001):
//! the response is the authoritative word on paid access; the desktop only reads
//! it.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use shared::{EntitlementsResponse, FeatureKey, PlanTier, SubscriptionStatus};
use tower::ServiceExt;

use api::entitlement::{AccountState, InMemoryAccountStateStore};
use api::store::{dev_principal, InMemoryPrincipalStore, DEV_TOKEN};

/// The router under test: the dev principal store (resolves `DEV_TOKEN` to the
/// dev principal on `acct_acme`) plus an account-state store seeding that account
/// to an active Pro subscription.
fn app() -> axum::Router {
    let accounts = InMemoryAccountStateStore::new().with_account(
        dev_principal().account_id,
        AccountState {
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Active,
            trial: false,
            current_period_end: None,
        },
    );
    api::entitlements_app(
        Arc::new(InMemoryPrincipalStore::dev_seed()),
        Arc::new(accounts),
    )
}

async fn get(app: axum::Router, auth: Option<&str>) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().uri("/me/entitlements");
    if let Some(value) = auth {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn entitlements_returns_200_for_authenticated_request() {
    let (status, bytes) = get(app(), Some(&format!("Bearer {DEV_TOKEN}"))).await;
    assert_eq!(status, StatusCode::OK);

    // The body is the typed EntitlementsResponse, and it round-trips (ISC-23).
    let resp: EntitlementsResponse = serde_json::from_slice(&bytes).unwrap();
    // It names the caller's account (ISC-19).
    assert_eq!(resp.entitlements.account_id, dev_principal().account_id);
    // Active Pro: paid features are granted — the desktop reads this, never
    // computes it (ADR-0001).
    assert!(resp.entitlements.allows(FeatureKey::ExportPdf));
    assert!(resp.entitlements.allows(FeatureKey::CloudSync));
    assert!(resp.entitlements.allows(FeatureKey::AdvancedReports));
}

#[tokio::test]
async fn entitlements_rejects_unauthenticated() {
    let (status, _) = get(app(), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn entitlements_rejects_unknown_token() {
    let (status, _) = get(app(), Some("Bearer tok_does_not_exist")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn entitlements_404_when_account_state_missing() {
    // An authenticated caller whose account has no billing state on record: the
    // identity resolves, but there is nothing to compute entitlements from.
    let app = api::entitlements_app(
        Arc::new(InMemoryPrincipalStore::dev_seed()),
        Arc::new(InMemoryAccountStateStore::new()), // empty: no account seeded
    );
    let (status, _) = get(app, Some(&format!("Bearer {DEV_TOKEN}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn default_app_serves_entitlements_for_the_dev_token() {
    // The runnable dev server (`api::app`) must serve real entitlements for the
    // walking-skeleton dev token, so the desktop dev build can load paid access
    // end-to-end. Without this, the route would 404/401 against the real binary.
    let (status, bytes) = get(api::app(), Some(&format!("Bearer {DEV_TOKEN}"))).await;
    assert_eq!(status, StatusCode::OK);
    let resp: EntitlementsResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(resp.entitlements.allows(FeatureKey::ExportPdf));
}
