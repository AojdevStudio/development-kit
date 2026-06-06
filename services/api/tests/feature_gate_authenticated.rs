//! Behavior: the authenticated server-side feature gate (issue #30).
//!
//! This is the keystone the issue requires: a backend route that resolves the
//! caller's entitlement snapshot FROM THEIR TOKEN (not from a client-supplied
//! body) via the real entitlement engine, then gates one concrete paid feature —
//! `AdvancedReports`. Denied (403) for a free/unentitled account, allowed (200)
//! for an entitled active-Pro account, 401 for an unauthenticated caller, 404 for
//! an unknown feature key. Authority is server-side (ADR-0001): the desktop calls
//! this and reads the verdict; it never decides access itself.
//!
//! Exercised through the real router via `tower::ServiceExt::oneshot` — the
//! request flows through the real auth extraction, the real entitlement engine,
//! and the real gate, no socket bound. `gate_denies_advanced_reports_*` is the
//! name the xtask coverage manifest references for the authenticated backend
//! coverage of `AdvancedReports`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use shared::{FeatureKey, PlanTier, SubscriptionStatus};
use tower::ServiceExt;

use api::entitlement::{AccountState, InMemoryAccountStateStore};
use api::principal::{Principal, Role};
use api::store::{InMemoryPrincipalStore, DEV_TOKEN};

/// A second dev token bound to a free account, so the same router can serve both
/// an entitled (Pro) and an unentitled (free) caller through real auth.
const FREE_TOKEN: &str = "tok_free";
const FREE_ACCOUNT: &str = "acct_free";

/// The router under test: the dev principal (`DEV_TOKEN` → `acct_acme`) seeded to
/// active Pro, plus a free principal (`FREE_TOKEN` → `acct_free`) seeded to free.
/// Both flow through the real authenticated gate.
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
    api::feature_gate_app(Arc::new(principals), Arc::new(accounts))
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

/// POST the authenticated gate route for `feature` with the given auth header and
/// return the HTTP status. No request body carries entitlements — the gate
/// resolves them from the token.
async fn gate_status(app: axum::Router, feature: &str, auth: Option<&str>) -> StatusCode {
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/gated-feature/{feature}"));
    if let Some(value) = auth {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    response.status()
}

/// ISC-2/ISC-3/ISC-13: the named backend coverage test for `AdvancedReports`.
/// Denied for a free account, allowed for an entitled Pro account — both resolved
/// server-side from the caller's token.
#[tokio::test]
async fn gate_denies_advanced_reports_without_entitlement_and_allows_with_it() {
    let key = FeatureKey::AdvancedReports.as_str();
    // Free account: no AdvancedReports → 403.
    assert_eq!(
        gate_status(app(), key, Some(&format!("Bearer {FREE_TOKEN}"))).await,
        StatusCode::FORBIDDEN,
        "a free account must be denied the paid feature"
    );
    // Active Pro account: AdvancedReports granted → 200.
    assert_eq!(
        gate_status(app(), key, Some(&format!("Bearer {DEV_TOKEN}"))).await,
        StatusCode::OK,
        "an entitled Pro account must be allowed the paid feature"
    );
}

/// ISC-4: an unauthenticated caller is rejected before any entitlement work.
#[tokio::test]
async fn authenticated_gate_rejects_missing_and_invalid_tokens() {
    let key = FeatureKey::AdvancedReports.as_str();
    assert_eq!(
        gate_status(app(), key, None).await,
        StatusCode::UNAUTHORIZED,
        "no bearer token → 401"
    );
    assert_eq!(
        gate_status(app(), key, Some("Bearer tok_nope")).await,
        StatusCode::UNAUTHORIZED,
        "unknown token → 401"
    );
}

/// ISC-5: an unknown feature-key path segment is a 404 — the gate only speaks the
/// stable wire vocabulary.
#[tokio::test]
async fn authenticated_gate_rejects_unknown_feature_key() {
    assert_eq!(
        gate_status(
            app(),
            "not_a_real_feature",
            Some(&format!("Bearer {DEV_TOKEN}"))
        )
        .await,
        StatusCode::NOT_FOUND,
        "unknown feature key → 404"
    );
}

/// ISC-3 (cross-check): a free caller is denied AdvancedReports but the SAME free
/// caller is allowed a free-tier capability — proving the verdict is per-feature
/// and resolved from the real per-account snapshot, not blanket.
#[tokio::test]
async fn authenticated_gate_is_per_feature_against_the_resolved_snapshot() {
    // Free accounts get MaxProjects (a non-zero limit) but not AdvancedReports.
    assert_eq!(
        gate_status(
            app(),
            FeatureKey::MaxProjects.as_str(),
            Some(&format!("Bearer {FREE_TOKEN}"))
        )
        .await,
        StatusCode::OK,
        "free tier allows max_projects (non-zero limit)"
    );
    assert_eq!(
        gate_status(
            app(),
            FeatureKey::AdvancedReports.as_str(),
            Some(&format!("Bearer {FREE_TOKEN}"))
        )
        .await,
        StatusCode::FORBIDDEN,
        "free tier denies advanced_reports"
    );
}

/// ISC-16 (anti): the route must NOT trust a client-supplied entitlements body.
/// Even a body claiming full access is ignored — the verdict comes from the
/// token's resolved snapshot. A free caller stays denied no matter the body.
#[tokio::test]
async fn authenticated_gate_ignores_a_lying_request_body() {
    let lying_body = serde_json::json!({
        "account_id": "acct_free",
        "plan": "enterprise",
        "status": "active",
        "trial": false,
        "features": { "advanced_reports": true }
    })
    .to_string();
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/gated-feature/{}",
                    FeatureKey::AdvancedReports.as_str()
                ))
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
        "a forged entitlements body must not grant access — the token decides"
    );
}

/// ISC-1: the default runnable app also mounts the authenticated gate, so the dev
/// server enforces it for the dev token end-to-end.
#[tokio::test]
async fn default_app_enforces_the_authenticated_gate_for_the_dev_token() {
    assert_eq!(
        gate_status(
            api::app(),
            FeatureKey::AdvancedReports.as_str(),
            Some(&format!("Bearer {DEV_TOKEN}"))
        )
        .await,
        StatusCode::OK,
        "the dev Pro account is allowed AdvancedReports through the real binary's router"
    );
}
