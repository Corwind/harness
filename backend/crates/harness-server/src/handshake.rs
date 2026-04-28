//! Stdout handshake — `{"port": N, "token": "..."}` printed once at startup.
//!
//! The Swift parent reads the first line of stdout and drains the rest as
//! free-form text. Logs MUST go to stderr to keep that line clean.

use serde::{Deserialize, Serialize};

/// Wire shape announced on stdout at startup. Field order is fixed: `port`
/// then `token`. Tests rely on the JSON object being a single line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handshake {
    pub port: u16,
    pub token: String,
}

impl Handshake {
    pub fn new(port: u16, token: impl Into<String>) -> Self {
        Self {
            port,
            token: token.into(),
        }
    }

    /// Serialise to a single JSON line (no trailing newline).
    pub fn to_json_line(&self) -> String {
        serde_json::to_string(self).expect("Handshake serialises infallibly")
    }
}
