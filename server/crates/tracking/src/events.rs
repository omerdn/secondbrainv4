//! Domain events published by `tracking` (outbox, ADR-0007).

use chrono::NaiveDate;
use kernel::DomainEvent;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An entry was added with a note (T3). Published in the entry's transaction; `notebooks` adds
/// the note to the item's process page as a dated line and links it back with
/// [`crate::TrackerEntries::link_note`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryNoted {
    pub entry_id: Uuid,
    pub item_id: Uuid,
    pub user_id: Uuid,
    pub local_date: NaiveDate,
    pub note: String,
}

impl DomainEvent for EntryNoted {
    const EVENT_TYPE: &'static str = "tracking.entry_noted";
}
