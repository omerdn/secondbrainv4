//! `kernel`: shared primitives used by every module (ARCHITECTURE §3).
//!
//! - [`Id`]: typed UUIDv7 identifiers
//! - [`AppError`]: API error type and HTTP mapping
//! - [`Clock`]: injectable time source
//! - [`pagination`]: cursor pagination
//! - [`validation`]: input validation helpers
//!
//! The domain event bus is added with the outbox (T0.2.5).

pub mod clock;
pub mod error;
pub mod id;
pub mod pagination;
pub mod validation;

pub use clock::{Clock, FixedClock, SharedClock, SystemClock};
pub use error::{AppError, BoxError};
pub use id::Id;
pub use pagination::{Cursor, Page, PageParams, PageRequest};
pub use validation::{FieldError, FieldErrorCode, ValidationErrors};
