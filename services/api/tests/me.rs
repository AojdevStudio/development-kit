//! Behavior: `GET /me` resolves the caller for an authenticated request and
//! rejects unauthenticated ones (issue #27 acceptance criteria 1 and 2).
//!
//! Exercised through the real router via `tower::ServiceExt::oneshot` — real
//! requests flow through the real auth extraction and handler, no socket bound.
//! The router is built with a seeded in-memory principal store so the test is
//! about HTTP + auth behavior, not about where identity is durably stored
//! (Postgres lands in a later issue).

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// The seed token wired into the test app's store. Resolves to a known
/// principal so the authenticated path returns a concrete identity.
const ALICE_TOKEN: &str = "tok_alice";

fn app() -> axum::Router {
    api::test_support::app_with_seeded_store()
}

#[tokio::test]
async fn me_returns_resolved_principal_for_authenticated_request() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(header::AUTHORIZATION, format!("Bearer {ALICE_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // The resolved principal: who is calling and which account they belong to.
    assert_eq!(body["user"]["id"], "user_alice");
    assert_eq!(body["user"]["email"], "alice@example.com");
    assert_eq!(body["account"]["id"], "acct_acme");
    assert_eq!(body["account"]["name"], "Acme");
    assert_eq!(body["membership"]["role"], "owner");
}

#[tokio::test]
async fn me_rejects_request_with_no_authorization_header() {
    let response = app()
        .oneshot(Request::builder().uri("/me").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_rejects_request_with_unknown_token() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(header::AUTHORIZATION, "Bearer tok_does_not_exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn default_app_resolves_the_dev_seed_token() {
    // The runnable dev server (`api::app`) must resolve the walking-skeleton dev
    // token, so the desktop can demonstrate loading the current account in dev
    // (issue #27 acceptance criterion 3). Without this, the desktop's dev token
    // would 401 against the real binary even though the handler is correct.
    let response = api::app()
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(header::AUTHORIZATION, format!("Bearer {ALICE_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn me_rejects_request_with_malformed_authorization_header() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(header::AUTHORIZATION, "NotBearer whatever")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
