//! ChaCha20-Poly1305 AEAD wrapper used by [`SqliteProvidersConfigRepo`].
//!
//! On encrypt we generate a fresh 12-byte random nonce and prepend it
//! to the ciphertext. The on-disk blob layout is therefore:
//!
//! ```text
//! [ nonce (12 bytes) ][ ciphertext+tag (N+16 bytes) ]
//! ```
//!
//! The key is held in a [`Secret`] and never logged.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;

use crate::secret::Secret;

const NONCE_LEN: usize = 12;

/// Failures observable from the cipher.
#[derive(Debug, thiserror::Error)]
pub enum CipherError {
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed: ciphertext, key, or tag is invalid")]
    Decrypt,
    #[error("ciphertext blob is shorter than the nonce")]
    Truncated,
}

/// Encrypt `plaintext` with the given key. Returns `nonce || ciphertext`.
pub(crate) fn encrypt(secret: &Secret, plaintext: &[u8]) -> Result<Vec<u8>, CipherError> {
    let key = Key::from_slice(secret.as_bytes());
    let cipher = ChaCha20Poly1305::new(key);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CipherError::Encrypt)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a `nonce || ciphertext` blob produced by [`encrypt`].
pub(crate) fn decrypt(secret: &Secret, blob: &[u8]) -> Result<Vec<u8>, CipherError> {
    if blob.len() < NONCE_LEN {
        return Err(CipherError::Truncated);
    }
    let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
    let key = Key::from_slice(secret.as_bytes());
    let cipher = ChaCha20Poly1305::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ct).map_err(|_| CipherError::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = Secret::from_bytes([42u8; 32]);
        let pt = b"hello world";
        let ct = encrypt(&s, pt).unwrap();
        assert_ne!(&ct[NONCE_LEN..], pt);
        let back = decrypt(&s, &ct).unwrap();
        assert_eq!(back, pt);
    }

    #[test]
    fn each_encryption_uses_fresh_nonce() {
        let s = Secret::from_bytes([42u8; 32]);
        let a = encrypt(&s, b"same").unwrap();
        let b = encrypt(&s, b"same").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn wrong_key_fails() {
        let a = Secret::from_bytes([1u8; 32]);
        let b = Secret::from_bytes([2u8; 32]);
        let ct = encrypt(&a, b"x").unwrap();
        assert!(matches!(decrypt(&b, &ct).unwrap_err(), CipherError::Decrypt));
    }

    #[test]
    fn truncated_blob_errors() {
        let s = Secret::from_bytes([0u8; 32]);
        assert!(matches!(decrypt(&s, b"abc").unwrap_err(), CipherError::Truncated));
    }
}
