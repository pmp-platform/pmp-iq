//! Symmetric encryption for secrets at rest, behind the [`Encryptor`] trait.

use aes_gcm::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;

const NONCE_LEN: usize = 12;

/// Errors from encryption/decryption.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("invalid key: {0}")]
    Key(String),
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed")]
    Decrypt,
}

/// Encrypts and decrypts byte payloads.
#[cfg_attr(test, mockall::automock)]
pub trait Encryptor: Send + Sync {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError>;
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError>;
}

/// AES-256-GCM encryptor. Stored payloads are `nonce (12 bytes) || ciphertext`.
pub struct AesGcmEncryptor {
    cipher: Aes256Gcm,
}

impl AesGcmEncryptor {
    /// Build from a base64-encoded 32-byte key.
    pub fn from_base64(key_b64: &str) -> Result<Self, CryptoError> {
        let key = base64::engine::general_purpose::STANDARD
            .decode(key_b64.trim())
            .map_err(|e| CryptoError::Key(e.to_string()))?;
        if key.len() != 32 {
            return Err(CryptoError::Key(format!(
                "expected 32 bytes, got {}",
                key.len()
            )));
        }
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| CryptoError::Key(e.to_string()))?;
        Ok(Self { cipher })
    }
}

impl Encryptor for AesGcmEncryptor {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| CryptoError::Encrypt)?;
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    fn decrypt(&self, payload: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if payload.len() < NONCE_LEN {
            return Err(CryptoError::Decrypt);
        }
        let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| CryptoError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> String {
        base64::engine::general_purpose::STANDARD.encode([7u8; 32])
    }

    #[test]
    fn round_trips_plaintext() {
        let enc = AesGcmEncryptor::from_base64(&key()).unwrap();
        let ct = enc.encrypt(b"ghp_secret").unwrap();
        assert_ne!(ct, b"ghp_secret");
        assert_eq!(enc.decrypt(&ct).unwrap(), b"ghp_secret");
    }

    #[test]
    fn distinct_nonces_yield_distinct_ciphertexts() {
        let enc = AesGcmEncryptor::from_base64(&key()).unwrap();
        assert_ne!(enc.encrypt(b"x").unwrap(), enc.encrypt(b"x").unwrap());
    }

    #[test]
    fn rejects_bad_key_length() {
        let short = base64::engine::general_purpose::STANDARD.encode([0u8; 16]);
        assert!(AesGcmEncryptor::from_base64(&short).is_err());
    }

    #[test]
    fn tampered_payload_fails() {
        let enc = AesGcmEncryptor::from_base64(&key()).unwrap();
        let mut ct = enc.encrypt(b"data").unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0xff;
        assert!(enc.decrypt(&ct).is_err());
    }
}
