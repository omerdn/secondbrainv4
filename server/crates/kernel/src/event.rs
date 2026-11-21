//! Domain events (ARCHITECTURE §4.5).
//!
//! A module announces that something happened (`ItemCreated`, `LinkAdded`, ...) by writing a
//! [`DomainEvent`] to the outbox **in the same transaction** as the change itself
//! (`db::outbox::publish`). The dispatcher later delivers it to subscribers in other modules.
//! This is how modules react to each other without touching each other's tables (ADR-0007).
//!
//! ```
//! use kernel::DomainEvent;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Serialize, Deserialize)]
//! struct ItemArchived {
//!     item_id: String,
//! }
//!
//! impl DomainEvent for ItemArchived {
//!     const EVENT_TYPE: &'static str = "item.archived";
//! }
//! ```

use serde::Serialize;
use serde::de::DeserializeOwned;

/// An event persisted in the outbox and delivered to subscribers at least once.
///
/// The payload is stored as JSON. Evolve it compatibly (add optional fields); events written by
/// older code may still be waiting in the outbox when new code starts.
pub trait DomainEvent: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Stable identifier stored with each event, e.g. `item.created`. Never rename a published
    /// type; introduce a new one instead.
    const EVENT_TYPE: &'static str;
}
