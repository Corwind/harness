//! Behavior tests for `/v1/health` and the `X-Harness-Token` middleware.
//!
//! We exercise the router via `tower::ServiceExt::oneshot` so these tests do
//! not bind a socket; the loopback bind path is covered by
//! `tests/server_e2e.rs`.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use harness_server::{build_router, TOKEN_HEADER};
use http_body_util::BodyExt;
use pretty_assertions::assert_eq;
use tower::ServiceExt;

use common::{TestApp, TOKEN};

async fn router() -> (axum::Router, TestApp) {
    let app = TestApp::boot().await;
    let r = build_router(app.state.clone());
    (r, app)
}

#[tokio::test]
async fn health_returns_200_with_valid_token() {
    let (router, _app) = router().await;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .header(TOKEN_HEADER, TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("ok"));
    assert!(v.get("version").and_then(|s| s.as_str()).is_some());
}

#[tokio::test]
async fn missing_token_returns_401() {
    let (router, _app) = router().await;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_token_returns_401() {
    let (router, _app) = router().await;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .header(TOKEN_HEADER, "not-the-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_with_different_length_returns_401() {
    // Constant-time comparison still rejects different-length candidates.
    let (router, _app) = router().await;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .header(TOKEN_HEADER, "x")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unknown_route_with_valid_token_returns_404() {
    // The auth layer must run before route matching is finalised, but axum's
    // 404 still wins for a missing route. This test pins that behavior so
    // future endpoints don't accidentally start authenticating 404 paths.
    let (router, _app) = router().await;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/does-not-exist")
                .header(TOKEN_HEADER, TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
