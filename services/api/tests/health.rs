//! Behavior: the API answers a health check with 200.
//!
//! Exercised through the real router via `tower::ServiceExt::oneshot` — a real
//! request flows through the real handler, no socket bound. This is the test
//! behind the "API responds 200 on a health route" acceptance criterion.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn health_route_returns_200() {
    let app = api::app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_route_returns_ok_status_body() {
    let app = api::app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let app = api::app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
