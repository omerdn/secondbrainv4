//! `identity`: Users, authentication, sessions, friendships and blocking.
//!
//! Owns the `users`, `auth_identities` and `sessions` tables (ADR-0007). Other modules use
//! [`IdentityService`] and the [`http::Authenticated`] extractor, never these tables.
//!
//! - [`password`]: argon2id hashing (ADR-0016)
//! - [`token`]: opaque 256-bit session tokens, stored as SHA-256 digests
//! - [`service`]: register, login, authenticate, session list and revocation
//! - [`email_flow`]: e-mail verification and password reset
//! - [`oauth`]: Google / Apple sign-in and account linking
//! - [`http`]: `/auth/*` endpoints

pub mod email_flow;
pub mod http;
pub mod model;
pub mod oauth;
pub mod password;
pub mod rate_limit;
pub mod service;
pub mod token;

pub use model::{SessionId, UserId};
pub use service::{AuthContext, EmailSettings, IdentityService};
