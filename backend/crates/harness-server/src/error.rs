//! HTTP error mapping.
//!
//! Domain errors from the ports flow through `ApiError` and become
//! `application/json` problem-style responses. The wire shape is
//! intentionally compact:
//!
//! ```json
//! { "code": "provider.unauthorized", "message": "..." }
//! ```
//!
//! Tightening to a full RFC 7807 envelope (`type`, `title`, `status`,
//! `detail`) is left to a later task; the `code`/`message` shape is
//! what the Swift client parses today.
//!
//! Some 4xx responses carry headers in addition to the JSON body — the
//! canonical example is `Retry-After` for 429 rate-limit responses.
//! `ApiError::with_header` lets a constructor attach those.

use axum::{
    http::{
        header::{HeaderName, HeaderValue},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};
use harness_core::error::{ProviderError, RepoError, SandboxError};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiError {
    #[serde(skip)]
    pub status: StatusCode,
    #[serde(skip)]
    pub extra_headers: Vec<(HeaderName, HeaderValue)>,
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            extra_headers: Vec::new(),
            code: code.into(),
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

    /// Attach an additional response header (e.g. `Retry-After` for 429).
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.extra_headers.push((name, value));
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        let mut headers = HeaderMap::new();
        for (n, v) in &self.extra_headers {
            headers.insert(n.clone(), v.clone());
        }
        (status, headers, Json(self)).into_response()
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

/// Map a `ProviderError` to the most semantically faithful HTTP status.
///
/// The wire shape preserves the variant via a stable `code` string so
/// the UI (or any other consumer) can branch without parsing the
/// human-readable message.
///
/// Mapping (per the T1.followup brief):
/// * `Unauthorized`              → 401 `provider.unauthorized`
/// * `RateLimited`               → 429 `provider.rate_limited` + `Retry-After` header
/// * `NotConfigured`             → 409 `provider.unconfigured`
/// * `Request { 4xx }` (other)   → original 4xx code `provider.request`
/// * `Request { 5xx }`           → 502 `provider.upstream`
/// * `Request { None }`          → 502 `provider.error`
/// * `Transport`                 → 502 `provider.unavailable`
/// * `Decode`                    → 502 `provider.decode`
/// * `Other`                     → 502 `provider.error`
impl From<ProviderError> for ApiError {
    fn from(e: ProviderError) -> Self {
        match e {
            ProviderError::Unauthorized(msg) => {
                ApiError::new(StatusCode::UNAUTHORIZED, "provider.unauthorized", msg)
            }
            ProviderError::RateLimited {
                retry_after_secs,
                message,
            } => {
                let mut err = ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "provider.rate_limited",
                    message,
                );
                if let Some(secs) = retry_after_secs {
                    if let Ok(value) = HeaderValue::from_str(&secs.to_string()) {
                        err = err.with_header(axum::http::header::RETRY_AFTER, value);
                    }
                }
                err
            }
            ProviderError::NotConfigured(msg) => {
                ApiError::new(StatusCode::CONFLICT, "provider.unconfigured", msg)
            }
            ProviderError::Request {
                status: Some(s),
                message,
            } => {
                if (400..500).contains(&s) {
                    let code = StatusCode::from_u16(s).unwrap_or(StatusCode::BAD_GATEWAY);
                    ApiError::new(code, "provider.request", message)
                } else if (500..600).contains(&s) {
                    ApiError::new(
                        StatusCode::BAD_GATEWAY,
                        "provider.upstream",
                        format!("upstream {s}: {message}"),
                    )
                } else {
                    ApiError::new(StatusCode::BAD_GATEWAY, "provider.error", message)
                }
            }
            ProviderError::Request {
                status: None,
                message,
            } => ApiError::new(StatusCode::BAD_GATEWAY, "provider.error", message),
            ProviderError::Transport(msg) => {
                ApiError::new(StatusCode::BAD_GATEWAY, "provider.unavailable", msg)
            }
            ProviderError::Decode(msg) => {
                ApiError::new(StatusCode::BAD_GATEWAY, "provider.decode", msg)
            }
            ProviderError::Other(msg) => {
                ApiError::new(StatusCode::BAD_GATEWAY, "provider.error", msg)
            }
        }
    }
}
