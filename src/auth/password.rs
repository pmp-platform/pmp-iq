//! Password hashing and random-secret generation, behind traits so they can be
//! mocked (and made deterministic) in tests.

use super::principal::AuthError;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand::Rng;
use rand::distributions::Alphanumeric;

/// Hashes and verifies passwords.
#[cfg_attr(test, mockall::automock)]
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: &str) -> Result<String, AuthError>;
    fn verify(&self, password: &str, hash: &str) -> Result<bool, AuthError>;
}

/// Generates random secrets (passwords, CSRF tokens).
#[cfg_attr(test, mockall::automock)]
pub trait SecretGenerator: Send + Sync {
    fn generate(&self, len: usize) -> String;
}

/// Argon2id-based password hasher.
pub struct Argon2Hasher;

impl PasswordHasher for Argon2Hasher {
    fn hash(&self, password: &str) -> Result<String, AuthError> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| AuthError::Hashing(e.to_string()))
    }

    fn verify(&self, password: &str, hash: &str) -> Result<bool, AuthError> {
        let parsed = PasswordHash::new(hash).map_err(|e| AuthError::Hashing(e.to_string()))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }
}

/// Cryptographically-random alphanumeric secret generator.
pub struct RandomSecretGenerator;

impl SecretGenerator for RandomSecretGenerator {
    fn generate(&self, len: usize) -> String {
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(len)
            .map(char::from)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_round_trip_verifies() {
        let hasher = Argon2Hasher;
        let hash = hasher.hash("s3cret").unwrap();
        assert!(hasher.verify("s3cret", &hash).unwrap());
        assert!(!hasher.verify("wrong", &hash).unwrap());
    }

    #[test]
    fn random_generator_length_and_charset() {
        let g = RandomSecretGenerator;
        let s = g.generate(24);
        assert_eq!(s.len(), 24);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
