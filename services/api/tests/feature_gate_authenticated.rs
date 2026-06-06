//! Behavior: the authenticated server-side feature gate (issues #30, #57).
//!
//! This is the ONLY backend coverage the feature-key gate counts: a route that
//! resolves the caller's entitlement snapshot FROM THEIR TOKEN (auth → account
//! state → the entitlement engine), then gates the requested feature. It never
//! reads an entitlements body — a forged body can never grant access (ADR-0001).
//! Issue #57 closes the false-coverage hole by requiring EVERY baseline
//! `FeatureKey` to be backed by one of these authenticated tests, not a
//! body-trusting `/gated/{feature}` test.
//!
//! Per key: a free/unbilled account is denied (403) and an entitled active-Pro
//! account is allowed (200), both resolved server-side. The
//! `gate_denies_<key>_authenticated_*` test names are referenced by the xtask
//! coverage manifest, so the gate proves the claimed coverage corresponds to a
//! test that actually runs through the real authority boundary.
//!
//! Exercised through the real router via `tower::ServiceExt::oneshot` — the
//! request flows through real auth extraction, the real entitlement engine, and
//! the real gate, no socket bound.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use shared::{FeatureKey, PlanTier, SubscriptionStatus};
use tower::ServiceExt;

use api::entitlement::{AccountState, InMemoryAccountStateStore};
use api::principal::{Principal, Role};
use api::store::{InMemoryPrincipalStore, DEV_TOKEN};

/// A token bound to an account with NO billing state on record. The authenticated
/// gate resolves no entitlement for it and returns 403 for EVERY feature key —
/// including limit keys a free plan would otherwise allow — so it is the uniform
/// "denied" case for the per-key boundary test.
const UNBILLED_TOKEN: &str = "tok_unbilled";
const UNBILLED_ACCOUNT: &str = "acct_unbilled";

/// The router under test: the dev principal (`DEV_TOKEN` → `acct_acme`) seeded to
/// active Pro (grants all seven baseline keys), plus an unbilled principal
/// (`UNBILLED_TOKEN` → `acct_unbilled`) with no account state (grants nothing).
/// Both flow through the real authenticated gate.
fn app() -> axum::Router {
    let principals =
        InMemoryPrincipalStore::dev_seed().with_token(UNBILLED_TOKEN, unbilled_principal());
    // dev_seed seeds acct_acme to active Pro; acct_unbilled is intentionally absent.
    let accounts = InMemoryAccountStateStore::dev_seed();
    api::feature_gate_app(Arc::new(principals), Arc::new(accounts))
}

fn unbilled_principal() -> Principal {
    Principal {
        user_id: "user_unbilled".into(),
        email: "unbilled@example.com".into(),
        account_id: UNBILLED_ACCOUNT.into(),
        account_name: "Unbilled Co".into(),
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

/// The authority boundary for one key, resolved SERVER-SIDE from the token:
/// denied for the unbilled account (403), allowed for the active-Pro account
/// (200). One helper, called once per key, keeps each per-key test a one-liner
/// while the boundary assertion stays in a single place. This is exactly the
/// shape #57 requires the coverage manifest to point at — no entitlements body.
async fn assert_authenticated_gate_denies_without_and_allows_with(feature: FeatureKey) {
    let key = feature.as_str();
    assert_eq!(
        gate_status(app(), key, Some(&format!("Bearer {UNBILLED_TOKEN}"))).await,
        StatusCode::FORBIDDEN,
        "{key} must be denied for an account with no entitlement (resolved server-side)"
    );
    assert_eq!(
        gate_status(app(), key, Some(&format!("Bearer {DEV_TOKEN}"))).await,
        StatusCode::OK,
        "{key} must be allowed for the entitled Pro account (resolved from its token)"
    );
}

// --- #57: every baseline FeatureKey has an AUTHENTICATED backend gate test ---

#[tokio::test]
async fn gate_denies_export_pdf_authenticated_without_entitlement_and_allows_with_it() {
    assert_authenticated_gate_denies_without_and_allows_with(FeatureKey::ExportPdf).await;
}

#[tokio::test]
async fn gate_denies_cloud_sync_authenticated_without_entitlement_and_allows_with_it() {
    assert_authenticated_gate_denies_without_and_allows_with(FeatureKey::CloudSync).await;
}

#[tokio::test]
async fn gate_denies_advanced_reports_authenticated_without_entitlement_and_allows_with_it() {
    assert_authenticated_gate_denies_without_and_allows_with(FeatureKey::AdvancedReports).await;
}

#[tokio::test]
async fn gate_denies_team_members_authenticated_without_entitlement_and_allows_with_it() {
    assert_authenticated_gate_denies_without_and_allows_with(FeatureKey::TeamMembers).await;
}

#[tokio::test]
async fn gate_denies_max_projects_authenticated_without_entitlement_and_allows_with_it() {
    assert_authenticated_gate_denies_without_and_allows_with(FeatureKey::MaxProjects).await;
}

#[tokio::test]
async fn gate_denies_priority_support_authenticated_without_entitlement_and_allows_with_it() {
    assert_authenticated_gate_denies_without_and_allows_with(FeatureKey::PrioritySupport).await;
}

#[tokio::test]
async fn gate_denies_api_access_authenticated_without_entitlement_and_allows_with_it() {
    assert_authenticated_gate_denies_without_and_allows_with(FeatureKey::ApiAccess).await;
}

// --- the authority properties of the gate (kept from #30) ---

/// An unauthenticated caller is rejected before any entitlement work.
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

/// An unknown feature-key path segment is a 404 — the gate only speaks the stable
/// wire vocabulary.
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

/// The verdict is per-feature against the resolved snapshot. A free account is
/// allowed a free-tier capability (max_projects, a non-zero limit) but denied
/// AdvancedReports — proving the gate reads the real per-account snapshot, not a
/// blanket allow/deny.
#[tokio::test]
async fn authenticated_gate_is_per_feature_against_the_resolved_snapshot() {
    const FREE_TOKEN: &str = "tok_free";
    const FREE_ACCOUNT: &str = "acct_free";
    let principals = InMemoryPrincipalStore::dev_seed().with_token(
        FREE_TOKEN,
        Principal {
            user_id: "user_free".into(),
            email: "free@example.com".into(),
            account_id: FREE_ACCOUNT.into(),
            account_name: "Free Co".into(),
            role: Role::Owner,
        },
    );
    let accounts = InMemoryAccountStateStore::dev_seed().with_account(
        FREE_ACCOUNT,
        AccountState {
            plan: PlanTier::Free,
            status: SubscriptionStatus::Free,
            trial: false,
            current_period_end: None,
        },
    );
    let app = api::feature_gate_app(Arc::new(principals), Arc::new(accounts));
    assert_eq!(
        gate_status(
            app.clone(),
            FeatureKey::MaxProjects.as_str(),
            Some(&format!("Bearer {FREE_TOKEN}"))
        )
        .await,
        StatusCode::OK,
        "free tier allows max_projects (non-zero limit)"
    );
    assert_eq!(
        gate_status(
            app,
            FeatureKey::AdvancedReports.as_str(),
            Some(&format!("Bearer {FREE_TOKEN}"))
        )
        .await,
        StatusCode::FORBIDDEN,
        "free tier denies advanced_reports"
    );
}

/// The route must NOT trust a client-supplied entitlements body. Even a body
/// claiming full access is ignored — the verdict comes from the token's resolved
/// snapshot. An unbilled caller stays denied no matter the body.
#[tokio::test]
async fn authenticated_gate_ignores_a_lying_request_body() {
    let lying_body = serde_json::json!({
        "account_id": "acct_unbilled",
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
                .header(header::AUTHORIZATION, format!("Bearer {UNBILLED_TOKEN}"))
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

/// The default runnable app also mounts the authenticated gate, so the dev server
/// enforces it for the dev token end-to-end.
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

// --- #57: the body-trusting route is GONE from the live app ---

/// The body-trusting `/gated/{feature}` route must no longer exist on the live
/// `api::app()` router: it was never an authority boundary (a pure function over
/// HTTP), and crediting it as backend coverage was the false invariant #57 closes.
/// A POST to it now 404s.
#[tokio::test]
async fn body_trusting_gated_route_is_not_mounted_on_the_live_app() {
    let lying_body = serde_json::json!({
        "account_id": "acct_x",
        "plan": "pro",
        "status": "active",
        "trial": false,
        "features": { "advanced_reports": true }
    })
    .to_string();
    let response = api::app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/gated/{}", FeatureKey::AdvancedReports.as_str()))
                .header("content-type", "application/json")
                .body(Body::from(lying_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "the body-trusting /gated/{{feature}} route must not be reachable on the live app"
    );
}
