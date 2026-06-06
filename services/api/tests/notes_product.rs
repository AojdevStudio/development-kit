//! Behavior: the Notes sample product's server-side paid gate (issue #37).
//!
//! This is the capstone proof. The Notes product plugs into the spine through the
//! seam ONLY, and its one paid capability — publishing a note (`notes.publish_note`)
//! — is gated SERVER-SIDE: the backend resolves the caller's product entitlements
//! from their bearer token (auth → account state → the Notes policy) and gates
//! against that. A free/unentitled account is DENIED (403); an entitled active-Pro
//! account is ALLOWED (200); an unauthenticated caller is 401; and — the binding
//! ADR-0001 constraint — a lying request body claiming entitlement does NOT grant
//! access, because the body is never the authority.
//!
//! Exercised through the REAL router via `tower::ServiceExt::oneshot` — the request
//! flows through real auth extraction, the real account-state store, the real Notes
//! entitlement policy, and the real product-key gate, no socket bound.
//! `notes_publish_gate_denies_without_entitlement_and_allows_with_it` is the name
//! the xtask product coverage manifest references for `notes.publish_note`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use shared::{PlanTier, SubscriptionStatus};
use tower::ServiceExt;

use api::entitlement::{AccountState, InMemoryAccountStateStore};
use api::principal::{Principal, Role};
use api::products::notes::route::{router as notes_router, NotesState};
use api::store::{InMemoryPrincipalStore, DEV_TOKEN};

/// A second dev token bound to a free account, so the same router serves both an
/// entitled (Pro) and an unentitled (free) caller through real auth.
const FREE_TOKEN: &str = "tok_free";
const FREE_ACCOUNT: &str = "acct_free";

/// The Notes router under test: the dev principal (`DEV_TOKEN` → `acct_acme`)
/// seeded to active Pro, plus a free principal (`FREE_TOKEN` → `acct_free`) seeded
/// to free. Both flow through the real server-side Notes gate.
fn app() -> axum::Router {
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
    notes_router(NotesState {
        principals: Arc::new(principals),
        accounts: Arc::new(accounts),
    })
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

/// POST `/notes/publish` with the given auth header (and optional body) and return
/// the HTTP status. No body carries entitlements — the gate resolves them from the
/// token.
async fn publish_status(app: axum::Router, auth: Option<&str>, body: Body) -> StatusCode {
    let mut builder = Request::builder().method("POST").uri("/notes/publish");
    if let Some(value) = auth {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    app.oneshot(builder.body(body).unwrap())
        .await
        .unwrap()
        .status()
}

/// The named product-coverage test for `notes.publish_note`: denied for a free
/// account, allowed for an entitled Pro account — both resolved server-side from
/// the caller's token. This is the end-to-end gated-access acceptance test the
/// issue requires.
#[tokio::test]
async fn notes_publish_gate_denies_without_entitlement_and_allows_with_it() {
    // Free account: Notes paid key not granted → 403.
    assert_eq!(
        publish_status(app(), Some(&format!("Bearer {FREE_TOKEN}")), Body::empty()).await,
        StatusCode::FORBIDDEN,
        "a free account must be denied publishing a note"
    );
    // Active Pro account: Notes paid key granted → 200.
    assert_eq!(
        publish_status(app(), Some(&format!("Bearer {DEV_TOKEN}")), Body::empty()).await,
        StatusCode::OK,
        "an entitled Pro account must be allowed to publish a note"
    );
}

/// An unauthenticated caller is rejected before any entitlement work.
#[tokio::test]
async fn notes_publish_rejects_missing_and_invalid_tokens() {
    assert_eq!(
        publish_status(app(), None, Body::empty()).await,
        StatusCode::UNAUTHORIZED,
        "no bearer token → 401"
    );
    assert_eq!(
        publish_status(app(), Some("Bearer tok_nope"), Body::empty()).await,
        StatusCode::UNAUTHORIZED,
        "unknown token → 401"
    );
}

/// The binding ADR-0001 constraint: the route must NOT trust a client-supplied
/// body. Even a body claiming full Notes entitlement is ignored — the verdict
/// comes from the token's resolved snapshot. A free caller stays denied no matter
/// the body. This is what proves the gate is resolved server-side, not from the
/// body (unlike the seam's test scaffold).
#[tokio::test]
async fn notes_publish_ignores_a_lying_request_body() {
    let lying_body = serde_json::json!({
        "account_id": "acct_free",
        "namespace": "notes",
        "features": { "notes.publish_note": true }
    })
    .to_string();
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/notes/publish")
                .header(header::AUTHORIZATION, format!("Bearer {FREE_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(lying_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a forged product-entitlements body must not grant access — the token decides"
    );
}

/// The default runnable binary also mounts the Notes product, so the dev server
/// enforces the paid gate for the dev token end-to-end.
#[tokio::test]
async fn default_app_enforces_the_notes_gate_for_the_dev_token() {
    assert_eq!(
        publish_status(
            api::app(),
            Some(&format!("Bearer {DEV_TOKEN}")),
            Body::empty()
        )
        .await,
        StatusCode::OK,
        "the dev Pro account is allowed to publish a note through the real binary's router"
    );
}
