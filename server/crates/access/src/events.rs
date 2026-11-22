//! Domain events published by `access` (outbox, ADR-0007).

use kernel::DomainEvent;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Users who could view `item_id` before a recompute and cannot any more (V10). Published in the
/// recompute's transaction; the realtime layer ends their subscriptions to the item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessRevoked {
    pub item_id: Uuid,
    /// Sorted.
    pub user_ids: Vec<Uuid>,
}

impl DomainEvent for AccessRevoked {
    const EVENT_TYPE: &'static str = "access.revoked";
}
