//! Opaque session tokens (ARCHITECTURE §15, ADR-0016).
//!
//! A token is 256 random bits from the operating system's CSPRNG, sent to the client as
//! base64url without padding (43 characters). The database stores only its SHA-256 digest
//! (`sessions.token_hash`), so a database leak does not expose usable tokens. Lookup is by
//! digest equality in an indexed column; no secret-dependent comparison happens in Rust.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

const TOKEN_BYTES: usize = 32;
/// Length of the encoded token.
pub const TOKEN_LEN: usize = 43;

/// SHA-256 digest of a token, as stored in `sessions.token_hash`.
pub type TokenHash = [u8; 32];

/// A freshly issued token. Shown to the client once and never stored or logged.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Generates a new random token.
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    /// The encoded token to hand to the client.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Digest to store.
    pub fn hash(&self) -> TokenHash {
        digest(&self.0)
    }
}

/// Never print the secret, even in debug output.
impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

fn digest(token: &str) -> TokenHash {
    Sha256::digest(token.as_bytes()).into()
}

/// CSRF token for cookie-authenticated web requests (T0.3.6): derived from the session token,
/// so it is bound to that session, needs no storage, and cannot be computed without the
/// (HttpOnly) session cookie.
pub fn csrf_for(session_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"secondbrain-csrf-v1\0");
    hasher.update(session_token.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// Equality that does not stop at the first differing byte.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Digest of a token presented by a client, or `None` if it is not well-formed (so garbage
/// never reaches the database).
pub fn hash_presented(token: &str) -> Option<TokenHash> {
    let well_formed = token.len() == TOKEN_LEN
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    well_formed.then(|| digest(token))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn generated_tokens_are_well_formed_and_unique() {
        let a = SessionToken::generate().unwrap();
        let b = SessionToken::generate().unwrap();
        assert_eq!(a.expose().len(), TOKEN_LEN);
        assert_ne!(a, b);
        assert_eq!(hash_presented(a.expose()), Some(a.hash()));
    }

    #[test]
    fn debug_does_not_leak_the_token() {
        let token = SessionToken::generate().unwrap();
        assert!(!format!("{token:?}").contains(token.expose()));
    }

    #[test]
    fn csrf_token_is_bound_to_the_session() {
        let a = SessionToken::generate().unwrap();
        let b = SessionToken::generate().unwrap();
        assert_eq!(csrf_for(a.expose()), csrf_for(a.expose()));
        assert_ne!(csrf_for(a.expose()), csrf_for(b.expose()));
        assert_ne!(csrf_for(a.expose()), a.expose());
        assert_eq!(csrf_for(a.expose()).len(), 43);
    }

    #[test]
    fn constant_time_eq_compares_whole_values() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn digest_is_sha256() {
        // printf 'abc' | shasum -a 256
        assert_eq!(digest("abc")[..4], [0xba, 0x78, 0x16, 0xbf]);
    }

    proptest! {
        #[test]
        fn malformed_tokens_are_rejected(s in "\\PC{0,80}") {
            let ok = s.len() == TOKEN_LEN
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            prop_assert_eq!(hash_presented(&s).is_some(), ok);
        }
    }
}
