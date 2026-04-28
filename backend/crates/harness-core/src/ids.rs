//! Typed ID newtypes for domain entities.
//!
//! IDs are opaque strings in the wire/storage layers. Wrapping them as
//! distinct types prevents accidental cross-use (e.g. passing a
//! `MessageId` where a `ConversationId` is expected) and gives each type
//! one canonical place to centralise generation, parsing, and display.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Macro: declare a string-newtype ID with serde transparency and
/// helpers to construct one from a fresh UUID v4 or an existing string.
macro_rules! declare_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Generate a new ID from a fresh UUID v4.
            pub fn generate() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            /// Wrap an existing string (from storage, the wire, etc.).
            pub fn from_string(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            /// Borrow the inner string.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume and return the inner string.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

declare_id!(
    /// Identifier for a conversation.
    ConversationId
);
declare_id!(
    /// Identifier for a stored message row.
    MessageId
);
declare_id!(
    /// Identifier for a sandbox template.
    SandboxTemplateId
);
declare_id!(
    /// Identifier for a streaming run (one user-turn → assistant turn).
    RunId
);

/// Stable string identifier for a registered provider (e.g. "claude").
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ProviderId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}
