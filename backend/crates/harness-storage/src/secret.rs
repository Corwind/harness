//! `Secret` — a 32-byte at-rest encryption key newtype.
//!
//! The key is supplied to the backend at startup (PLAN §2.3). Wrapping
//! the bytes in a newtype ensures we never accidentally log them
//! (`Debug` is opaque) and zeroes them on drop.

use std::sync::Arc;

use zeroize::Zeroizing;

/// Encryption key handed to the storage layer at construction time.
///
/// Cloning is cheap (the underlying buffer is `Arc`'d) and the buffer
/// is zeroed when the last reference is dropped.
#[derive(Clone)]
pub struct Secret {
    bytes: Arc<Zeroizing<[u8; 32]>>,
}

impl Secret {
    /// Wrap a 32-byte key.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            bytes: Arc::new(Zeroizing::new(bytes)),
        }
    }

    /// Build a key from a hex string. Returns `None` if the input is
    /// not exactly 64 hex chars.
    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let hi = hex_nibble(chunk[0])?;
            let lo = hex_nibble(chunk[1])?;
            out[i] = (hi << 4) | lo;
        }
        Some(Self::from_bytes(out))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret").finish_non_exhaustive()
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
