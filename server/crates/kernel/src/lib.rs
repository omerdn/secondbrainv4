//! `kernel`: shared primitives used by every module (ARCHITECTURE §3).
//!
//! - [`Id`]: typed UUIDv7 identifiers
//! - [`AppError`]: API error type and HTTP mapping
//! - [`Clock`]: injectable time source
//! - [`pagination`]: cursor pagination
//! - [`validation`]: input validation helpers
//! - [`redact`]: masking personal data in free-form text before logging
//! - [`DomainEvent`]: events published through the outbox
//! - [`retry`]: backoff delays

pub mod clock;
pub mod error;
pub mod event;
pub mod id;
pub mod pagination;
pub mod redact;
pub mod retry;
pub mod validation;

pub use clock::{Clock, FixedClock, SharedClock, SystemClock};
pub use error::{AppError, BoxError, ErrorCode};
pub use event::DomainEvent;
pub use id::Id;
pub use pagination::{Cursor, Page, PageParams, PageRequest};
pub use validation::{FieldError, FieldErrorCode, ValidationErrors};
