//! Error taxonomy for the domain layer.
//!
//! Each port (provider, tool, sandbox, repo, secrets) has its own error
//! enum so that the orchestrator and HTTP layer can pattern-match on
//! the specific failure mode without conflating concerns. All variants
//! are `'static` to keep the futures returned by async traits `Send`.

use thiserror::Error;

/// Failures observable when invoking an `LlmProvider`.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// The provider configuration is missing or malformed (e.g. no API
    /// key). Surfaces in the UI as a "configure provider" prompt.
    #[error("provider not configured: {0}")]
    NotConfigured(String),

    /// The remote API rejected our request. `status` is the HTTP status
    /// when known.
    #[error("provider request failed (status {status:?}): {message}")]
    Request {
        status: Option<u16>,
        message: String,
    },

    /// Authentication failed (HTTP 401/403 or equivalent).
    #[error("provider authentication failed: {0}")]
    Unauthorized(String),

    /// The remote rate-limited us. `retry_after_secs` is provided when
    /// the upstream suggested one.
    #[error("provider rate limited (retry_after={retry_after_secs:?}s): {message}")]
    RateLimited {
        retry_after_secs: Option<u64>,
        message: String,
    },

    /// We failed to parse the streamed payload from the provider.
    #[error("provider stream decode error: {0}")]
    Decode(String),

    /// Network/transport error before any HTTP response was received.
    #[error("provider transport error: {0}")]
    Transport(String),

    /// Catch-all for anything else with an opaque message.
    #[error("provider error: {0}")]
    Other(String),
}

/// Failures observable when running a `Tool`.
#[derive(Debug, Error)]
pub enum ToolError {
    /// The tool is registered but the orchestrator could not run it
    /// because the conversation has no sandbox template attached and
    /// the tool requires sandboxing (fail-closed default per PLAN
    /// §2.4.1).
    #[error("tool '{tool}' refused: conversation has no sandbox template")]
    NoSandbox { tool: String },

    /// The input provided by the model did not match the tool's schema.
    #[error("invalid input for tool '{tool}': {message}")]
    InvalidInput { tool: String, message: String },

    /// The tool finished with a non-zero exit status.
    #[error("tool '{tool}' exited with status {status}: {stderr}")]
    NonZeroExit {
        tool: String,
        status: i32,
        stderr: String,
    },

    /// The tool was cancelled (run cancellation, timeout, …).
    #[error("tool '{tool}' cancelled")]
    Cancelled { tool: String },

    /// Sandbox layer failed while wrapping or executing the command.
    #[error("tool '{tool}' sandbox failure: {source}")]
    Sandbox {
        tool: String,
        #[source]
        source: SandboxError,
    },

    /// Catch-all.
    #[error("tool '{tool}' failed: {message}")]
    Other { tool: String, message: String },
}

/// Failures observable from the sandbox port.
#[derive(Debug, Error)]
pub enum SandboxError {
    /// `sandbox-exec` (or its eventual replacement) is unavailable.
    #[error("sandbox runtime unavailable: {0}")]
    RuntimeUnavailable(String),

    /// The provided SBPL profile failed to parse / load.
    #[error("invalid sandbox profile: {0}")]
    InvalidProfile(String),

    /// I/O error while materialising the profile or spawning.
    #[error("sandbox I/O error: {0}")]
    Io(String),

    /// Validation of a profile via dry-run failed. `stderr` is the
    /// captured stderr from the validator (typically `sandbox-exec`)
    /// so callers can surface the precise compiler diagnostic.
    #[error("sandbox profile validation failed: {stderr}")]
    ProfileInvalid { stderr: String },
}

/// Failures observable from any repository port.
#[derive(Debug, Error)]
pub enum RepoError {
    /// The requested entity does not exist.
    #[error("not found")]
    NotFound,

    /// A uniqueness or foreign-key constraint was violated.
    #[error("conflict: {0}")]
    Conflict(String),

    /// Underlying storage I/O failure (DB error, disk error, …). The
    /// adapter stringifies its driver error into this variant.
    #[error("storage error: {0}")]
    Storage(String),

    /// (De)serialisation between domain types and storage failed.
    #[error("serialisation error: {0}")]
    Serde(String),
}

/// Failures observable from the secrets port.
#[derive(Debug, Error)]
pub enum SecretsError {
    /// Underlying vault I/O error.
    #[error("secrets backend error: {0}")]
    Backend(String),

    /// Cryptographic operation failed (e.g. encryption with a wrong
    /// key).
    #[error("secrets crypto error: {0}")]
    Crypto(String),
}
