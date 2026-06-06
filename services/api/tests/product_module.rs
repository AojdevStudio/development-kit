//! Behavior: the backend half of the product-module seam (issue #36).
//!
//! Proves the [`BackendModule`] trait composes a product's routes into the spine
//! **additively**: a product module mounts under its namespace, its routes are
//! reachable, every baseline route still answers, and two products with distinct
//! namespaces compose without collision. This is the keystone the issue requires
//! — a product plugs in WITHOUT editing the shared foundation, and the existing
//! `.merge(...)` chain in `lib.rs` is never disturbed.
//!
//! Exercised through the real router via `tower::ServiceExt::oneshot` — requests
//! flow through the real app, no socket bound. The product modules here are tiny
//! in-test doubles (NOT the #37 sample product): just enough to exercise the
//! trait's three guarantees.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;

use api::product_module::{mount, BackendModule};
use shared::{ProductEntitlements, ProductFeatureKey, ProductModuleMeta};

/// A tiny product module ("vault") with two routes: a public ping and a
/// product-feature-gated action. The gated action uses `require_product_feature`
/// so the same authority question the spine asks also gates a *product* key.
struct VaultModule;

impl BackendModule for VaultModule {
    fn meta(&self) -> ProductModuleMeta {
        ProductModuleMeta::new("Vault", "vault").expect("valid namespace")
    }

    fn router(&self) -> Router {
        Router::new()
            .route("/ping", get(|| async { "vault-pong" }))
            .route("/share", post(share_record))
    }
}

/// A product-gated action: returns 200 when the supplied product entitlements
/// grant `vault.share_record`, 403 otherwise. The snapshot is the body here only
/// to keep the test double simple; a real product resolves it from the caller's
/// token exactly as the authenticated spine gate does (ADR-0001).
async fn share_record(Json(ent): Json<ProductEntitlements>) -> Result<Json<Value>, StatusCode> {
    let key = ProductFeatureKey::new("vault", "share_record").expect("valid key");
    api::feature_gate::require_product_feature(&ent, &key)?;
    Ok(Json(json!({ "shared": true })))
}

/// A second tiny product ("ledger") to prove two namespaces compose without
/// collision.
struct LedgerModule;

impl BackendModule for LedgerModule {
    fn meta(&self) -> ProductModuleMeta {
        ProductModuleMeta::new("Ledger", "ledger").expect("valid namespace")
    }
    fn router(&self) -> Router {
        Router::new().route("/ping", get(|| async { "ledger-pong" }))
    }
}

async fn status_of(app: &Router, method: &str, path: &str) -> StatusCode {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

// --- ISC-18 + ISC-42: composition is additive; baseline routes still answer ---
#[tokio::test]
async fn mounting_a_product_module_leaves_baseline_routes_reachable() {
    // The real spine app, plus the vault product merged in additively.
    let app = api::app().merge(mount(&VaultModule));

    // A baseline route still answers after the product mounts.
    assert_eq!(status_of(&app, "GET", "/health").await, StatusCode::OK);
    // The authenticated spine gate route is still mounted (404 only for unknown
    // feature, here it's a known key with no auth → 401, never a routing 404).
    let gate = status_of(&app, "POST", "/gated-feature/export_pdf").await;
    assert_ne!(
        gate,
        StatusCode::NOT_FOUND,
        "baseline gate route still mounted"
    );
}

// --- ISC-19 + ISC-16 + ISC-21: the product route is reachable under its prefix ---
#[tokio::test]
async fn product_route_is_reachable_under_its_namespace_prefix() {
    let app = api::app().merge(mount(&VaultModule));
    // The product's `/ping` is reachable at `/vault/ping` — the prefix came from
    // meta, not a hardcoded namespace in the route.
    assert_eq!(status_of(&app, "GET", "/vault/ping").await, StatusCode::OK);
    // Without the prefix it must NOT be reachable (proves nesting, not a leak).
    assert_eq!(status_of(&app, "GET", "/ping").await, StatusCode::NOT_FOUND);
}

// --- ISC-20: two products with distinct namespaces compose without collision ---
#[tokio::test]
async fn two_product_modules_compose_without_collision() {
    let app = api::app()
        .merge(mount(&VaultModule))
        .merge(mount(&LedgerModule));
    // Both products' identically-named `/ping` routes are reachable under their
    // own namespaces — no collision.
    assert_eq!(status_of(&app, "GET", "/vault/ping").await, StatusCode::OK);
    assert_eq!(status_of(&app, "GET", "/ledger/ping").await, StatusCode::OK);
    // And the baseline still answers.
    assert_eq!(status_of(&app, "GET", "/health").await, StatusCode::OK);
}

// --- #36: a product feature gate denies/allows through the real route ---
// This is the named coverage test for the example product key `vault.share_record`.
#[tokio::test]
async fn product_gate_denies_share_without_entitlement_and_allows_with_it() {
    let app = api::app().merge(mount(&VaultModule));
    let key = ProductFeatureKey::new("vault", "share_record").unwrap();

    // Denied: an empty product snapshot grants nothing.
    let denied = ProductEntitlements::new("acct_free", "vault");
    let req = Request::builder()
        .method("POST")
        .uri("/vault/share")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&denied).unwrap()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );

    // Allowed: a snapshot granting the product key.
    let granted = ProductEntitlements::new("acct_pro", "vault")
        .with(key, shared::FeatureValue::Enabled(true));
    let req = Request::builder()
        .method("POST")
        .uri("/vault/share")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&granted).unwrap()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::OK
    );
}
