//! `GET /v1/health` — liveness probe.
//!
//! Shape mirrors `Health` in `spec/api.openapi.yaml`:
//! `{ "status": "ok", "version": "<semver>" }`. The version is the
//! `harness-server` crate version baked at compile time.

use axum::{routing::get, Json, Router};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
}

pub async fn handler() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route("/v1/health", get(handler))
}
