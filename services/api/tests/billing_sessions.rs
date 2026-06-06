//! Behavior: `POST /billing/checkout` and `POST /billing/portal` return a
//! provider session URL for an authenticated caller, reject unauthenticated ones,
//! and bind the session to the *authenticated* account (issue #31 acceptance
//! criteria).
//!
//! Exercised through the real router via `tower::ServiceExt::oneshot` — real
//! requests flow through the real auth extraction, the real billing handler, and
//! the deterministic mock provider, no socket bound and NO Stripe key. Authority
//! is server-side (ADR-0001): the desktop opens the URL the backend produced; it
//! never constructs one itself.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use shared::{CheckoutSessionResponse, PortalSessionResponse};
use tower::ServiceExt;

use api::billing::MockBillingProvider;
use api::store::{dev_principal, InMemoryPrincipalStore, DEV_TOKEN};

/// The router under test: the dev principal store (resolves `DEV_TOKEN` to the
/// dev principal on `acct_acme`) plus the deterministic mock billing provider.
fn app() -> axum::Router {
    api::billing_app(
        Arc::new(InMemoryPrincipalStore::dev_seed()),
        Arc::new(MockBillingProvider::new()),
    )
}

async fn post(
    app: axum::Router,
    uri: &str,
    auth: Option<&str>,
    body: &str,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(value) = auth {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    let response = app
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

// --- /billing/checkout ---

#[tokio::test]
async fn checkout_returns_200_with_a_url_for_an_authenticated_caller() {
    let (status, bytes) = post(
        app(),
        "/billing/checkout",
        Some(&format!("Bearer {DEV_TOKEN}")),
        r#"{"plan":"pro"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: CheckoutSessionResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(
        resp.url.starts_with("https://checkout.stripe.com/"),
        "got {}",
        resp.url
    );
}

#[tokio::test]
async fn checkout_binds_the_url_to_the_authenticated_account_not_the_body() {
    // The request body names only the plan; the URL must encode the authenticated
    // account (acct_acme), proving the handler does not trust a client-supplied
    // account id (ADR-0001).
    let (status, bytes) = post(
        app(),
        "/billing/checkout",
        Some(&format!("Bearer {DEV_TOKEN}")),
        r#"{"plan":"pro"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: CheckoutSessionResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(
        resp.url.contains(&dev_principal().account_id),
        "URL must name the authenticated account, got {}",
        resp.url
    );
}

#[tokio::test]
async fn checkout_rejects_missing_credentials() {
    let (status, _) = post(app(), "/billing/checkout", None, r#"{"plan":"pro"}"#).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn checkout_rejects_unknown_token() {
    let (status, _) = post(
        app(),
        "/billing/checkout",
        Some("Bearer tok_does_not_exist"),
        r#"{"plan":"pro"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// --- /billing/portal ---

#[tokio::test]
async fn portal_returns_200_with_a_url_for_an_authenticated_caller() {
    let (status, bytes) = post(
        app(),
        "/billing/portal",
        Some(&format!("Bearer {DEV_TOKEN}")),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: PortalSessionResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(
        resp.url.starts_with("https://billing.stripe.com/"),
        "got {}",
        resp.url
    );
}

#[tokio::test]
async fn portal_rejects_missing_credentials() {
    let (status, _) = post(app(), "/billing/portal", None, "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn portal_rejects_unknown_token() {
    let (status, _) = post(
        app(),
        "/billing/portal",
        Some("Bearer tok_does_not_exist"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// --- mounted alongside existing routes (issue #31: additive, no regression) ---

#[tokio::test]
async fn default_app_serves_billing_alongside_existing_routes() {
    // The runnable dev server must serve the billing routes AND keep the existing
    // authority surface working on the same app: /health (public), /me and
    // /me/entitlements (authed). Proves the merge is additive.
    let auth = format!("Bearer {DEV_TOKEN}");

    // billing checkout works on the default app
    let (checkout_status, _) = post(
        api::app(),
        "/billing/checkout",
        Some(&auth),
        r#"{"plan":"pro"}"#,
    )
    .await;
    assert_eq!(checkout_status, StatusCode::OK);

    // billing portal works on the default app
    let (portal_status, _) = post(api::app(), "/billing/portal", Some(&auth), "").await;
    assert_eq!(portal_status, StatusCode::OK);

    // /health still responds (public, GET)
    let health = api::app()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    // /me still responds for the dev token
    let me = api::app()
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(me.status(), StatusCode::OK);

    // /me/entitlements still responds for the dev token
    let ent = api::app()
        .oneshot(
            Request::builder()
                .uri("/me/entitlements")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ent.status(), StatusCode::OK);
}
