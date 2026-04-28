//! HTTP error mapping.
//!
//! Domain errors from the ports flow through `ApiError` and become
//! `application/json` problem-style responses. We do not yet emit the full
//! RFC 7807 envelope (T1.E will tighten this); for now the body is
//! `{"error": "<code>", "message": "..."}` which matches what the tests
//! and the early Swift client need.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use harness_core::error::{RepoError, SandboxError};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiError {
    #[serde(skip)]
    pub status: StatusCode,
    pub error: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            error: code,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        (status, Json(self)).into_response()
    }
}

impl From<RepoError> for ApiError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound => ApiError::not_found("entity not found"),
            RepoError::Conflict(msg) => ApiError::conflict(msg),
            RepoError::Storage(msg) => ApiError::internal(format!("storage error: {msg}")),
            RepoError::Serde(msg) => ApiError::internal(format!("serde error: {msg}")),
        }
    }
}

impl From<SandboxError> for ApiError {
    fn from(e: SandboxError) -> Self {
        // Validation flows do NOT use this conversion — they unpack
        // `ProfileInvalid` into a 200 body explicitly. This impl handles
        // unexpected sandbox failures (runtime missing, I/O, …).
        match e {
            SandboxError::RuntimeUnavailable(msg) => {
                ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "sandbox_unavailable", msg)
            }
            SandboxError::Io(msg) => ApiError::internal(format!("sandbox I/O: {msg}")),
            SandboxError::InvalidProfile(msg) => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "sandbox_invalid_profile",
                msg,
            ),
            SandboxError::ProfileInvalid { stderr } => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "sandbox_profile_invalid",
                stderr,
            ),
        }
    }
}
