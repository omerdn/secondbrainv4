//! Password hashing with argon2id (ARCHITECTURE §15, ADR-0016).
//!
//! Parameters are the argon2 crate defaults, which follow the OWASP recommendation for
//! argon2id (m = 19 MiB, t = 2, p = 1). Hashes are stored as PHC strings, so the parameters
//! travel with each hash and can be raised later without invalidating existing passwords.
//!
//! Hashing is deliberately slow (tens of milliseconds) and CPU bound: async callers must run
//! these functions on the blocking pool (`tokio::task::spawn_blocking`).

use std::sync::OnceLock;

use argon2::{Algorithm, Argon2, PasswordHasher, PasswordVerifier};

/// Shortest accepted password, in characters.
pub const MIN_PASSWORD_CHARS: usize = 8;
/// Longest accepted password, in characters. Bounds the work done per login attempt.
pub const MAX_PASSWORD_CHARS: usize = 128;

/// Hashing failed (only on internal errors such as an unavailable random source).
#[derive(Debug, thiserror::Error)]
#[error("password hashing failed: {0}")]
pub struct HashError(String);

fn hasher() -> Argon2<'static> {
    let argon2 = Argon2::default();
    debug_assert!(matches!(Algorithm::default(), Algorithm::Argon2id));
    argon2
}

/// Hashes `password` with a fresh random salt and returns the PHC string
/// (`$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`).
pub fn hash(password: &str) -> Result<String, HashError> {
    hasher()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|err| HashError(err.to_string()))
}

/// Whether `password` matches the PHC string `hash`. A malformed hash never matches.
pub fn verify(password: &str, hash: &str) -> bool {
    hasher().verify_password(password.as_bytes(), hash).is_ok()
}

/// Spends the same time as a real verification without matching anything. Called when the
/// account does not exist, so response times do not reveal which e-mails are registered.
pub fn verify_dummy(password: &str) {
    static DUMMY: OnceLock<Option<String>> = OnceLock::new();
    if let Some(dummy) = DUMMY.get_or_init(|| hash("dummy password for timing").ok()) {
        let _ = verify(password, dummy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_argon2id_phc_with_owasp_parameters() {
        let phc = hash("correct horse battery staple").unwrap();
        assert!(phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{phc}");
    }

    #[test]
    fn verify_accepts_only_the_right_password() {
        let phc = hash("şifre-1234").unwrap();
        assert!(verify("şifre-1234", &phc));
        assert!(!verify("şifre-1235", &phc));
        assert!(!verify("", &phc));
    }

    #[test]
    fn salts_differ_between_hashes() {
        assert_ne!(hash("same").unwrap(), hash("same").unwrap());
    }

    #[test]
    fn malformed_hash_never_matches() {
        assert!(!verify("x", "not a phc string"));
        assert!(!verify("x", ""));
        verify_dummy("x");
    }
}
