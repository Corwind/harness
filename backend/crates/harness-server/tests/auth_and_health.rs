//! Behavior tests for `/v1/health` and the `X-Harness-Token` middleware.
//!
//! We exercise the router via `tower::ServiceExt::oneshot` so these tests do
//! not bind a socket; the loopback bind path is covered by
//! `tests/server_e2e.rs`.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use harness_server::{build_router, ServerConfig, SessionToken, TOKEN_HEADER};
use http_body_util::BodyExt;
use pretty_assertions::assert_eq;
use tower::ServiceExt;

const TOKEN: &str = "test-token-please-ignore";

fn router() -> axum::Router {
    build_router(ServerConfig::new(SessionToken::new(TOKEN)))
}

#[tokio::test]
async fn health_returns_200_with_valid_token() {
    let response = router()
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
    let response = router()
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
    let response = router()
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
    let response = router()
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
    let response = router()
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
