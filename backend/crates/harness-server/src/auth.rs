//! `X-Harness-Token` middleware.
//!
//! Every request must carry the session token announced on stdout at startup.
//! Missing or mismatched token → 401. Constant-time comparison guards against
//! the timing oracle on a fixed-length token; loopback already limits the
//! threat model, but the cost is negligible.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header::HeaderName, StatusCode},
    middleware::Next,
    response::Response,
};

/// HTTP header name used to carry the session token. Matches the OpenAPI
/// `securitySchemes.harnessToken` definition.
pub const TOKEN_HEADER: HeaderName = HeaderName::from_static("x-harness-token");

/// Wrapper over the per-process session token. Cheap to clone (Arc).
#[derive(Debug, Clone)]
pub struct SessionToken(Arc<str>);

impl SessionToken {
    pub fn new(token: impl Into<Arc<str>>) -> Self {
        Self(token.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn constant_time_eq(&self, candidate: &[u8]) -> bool {
        let expected = self.0.as_bytes();
        if expected.len() != candidate.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (a, b) in expected.iter().zip(candidate.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

/// Axum `from_fn_with_state` middleware: rejects requests without a valid
/// `X-Harness-Token` with `401 Unauthorized` and an empty body.
pub async fn require_token(
    State(token): State<SessionToken>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let provided = req
        .headers()
        .get(&TOKEN_HEADER)
        .and_then(|v| v.to_str().ok());

    match provided {
        Some(value) if token.constant_time_eq(value.as_bytes()) => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
