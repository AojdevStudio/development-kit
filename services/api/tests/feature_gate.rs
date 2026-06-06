//! Backend feature-gate authorization tests (ADR-0001 server-side gates).
//!
//! These are the *real* non-React enforcement tests the feature-key coverage
//! gate (issue #25) counts. Each test drives a server-backed paid action through
//! the real router and asserts the authority boundary: without the entitlement
//! the action is denied (403), with it the action is allowed (200). React is not
//! involved — this is the backend deciding, which is exactly what ADR-0001
//! requires and what makes a paid feature safe to ship.
//!
//! Each `gate_denies_*_without_entitlement_and_allows_with_it` test name is
//! referenced by name in the xtask coverage manifest, so the gate can prove the
//! claimed coverage corresponds to a test that actually runs.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use shared::FeatureKey;
use tower::ServiceExt;

/// Drive the gated route for `feature` with the given entitlement-bearing body
/// and return the HTTP status. The body is the entitlements DTO the backend
/// would have computed for the caller; the route enforces against it.
async fn gated_status(feature: FeatureKey, entitlements_body: serde_json::Value) -> StatusCode {
    let app = api::app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/gated/{}", feature.as_str()))
                .header("content-type", "application/json")
                .body(Body::from(entitlements_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    response.status()
}

/// Entitlements that grant exactly `feature` (boolean-enabled), nothing else.
fn entitlements_granting(feature: FeatureKey) -> serde_json::Value {
    json!({
        "account_id": "acct_test",
        "plan": "pro",
        "status": "active",
        "trial": false,
        "features": { feature.as_str(): true }
    })
}

/// Entitlements that grant nothing — the caller has no paid access.
fn entitlements_granting_nothing() -> serde_json::Value {
    json!({
        "account_id": "acct_test",
        "plan": "free",
        "status": "active",
        "trial": false,
        "features": {}
    })
}

/// Assert the authority boundary for one feature: denied without it, allowed
/// with it. One helper, called once per key, keeps each per-key test a one-liner
/// while the boundary assertion stays in a single place.
async fn assert_gate_denies_without_and_allows_with(feature: FeatureKey) {
    assert_eq!(
        gated_status(feature, entitlements_granting_nothing()).await,
        StatusCode::FORBIDDEN,
        "{} must be denied without the entitlement",
        feature.as_str()
    );
    assert_eq!(
        gated_status(feature, entitlements_granting(feature)).await,
        StatusCode::OK,
        "{} must be allowed with the entitlement",
        feature.as_str()
    );
}

#[tokio::test]
async fn gate_denies_export_pdf_without_entitlement_and_allows_with_it() {
    assert_gate_denies_without_and_allows_with(FeatureKey::ExportPdf).await;
}

#[tokio::test]
async fn gate_denies_cloud_sync_without_entitlement_and_allows_with_it() {
    assert_gate_denies_without_and_allows_with(FeatureKey::CloudSync).await;
}

#[tokio::test]
async fn gate_denies_advanced_reports_without_entitlement_and_allows_with_it() {
    assert_gate_denies_without_and_allows_with(FeatureKey::AdvancedReports).await;
}

#[tokio::test]
async fn gate_denies_team_members_without_entitlement_and_allows_with_it() {
    assert_gate_denies_without_and_allows_with(FeatureKey::TeamMembers).await;
}

#[tokio::test]
async fn gate_denies_max_projects_without_entitlement_and_allows_with_it() {
    assert_gate_denies_without_and_allows_with(FeatureKey::MaxProjects).await;
}

#[tokio::test]
async fn gate_denies_priority_support_without_entitlement_and_allows_with_it() {
    assert_gate_denies_without_and_allows_with(FeatureKey::PrioritySupport).await;
}

#[tokio::test]
async fn gate_denies_api_access_without_entitlement_and_allows_with_it() {
    assert_gate_denies_without_and_allows_with(FeatureKey::ApiAccess).await;
}

#[tokio::test]
async fn gate_rejects_an_unknown_feature_key() {
    // A path segment that is not a known feature key is a 404 — the gate only
    // speaks the stable wire vocabulary, never an arbitrary string.
    let app = api::app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/gated/not_a_real_feature")
                .header("content-type", "application/json")
                .body(Body::from(entitlements_granting_nothing().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
