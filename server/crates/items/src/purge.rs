//! Daily permanent deletion of items whose 30 days in the trash are over (N4).

use kernel::SharedClock;
use serde::{Deserialize, Serialize};

use crate::service::ItemsService;

/// Every day at 04:15 UTC.
pub const PURGE_CRON: &str = "0 15 4 * * *";
pub const SCHEDULE_NAME: &str = "items-purge-trash";

/// The purge job (`items.purge_trash`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeTrash {}

impl jobs::Job for PurgeTrash {
    const KIND: &'static str = "items.purge_trash";
}

/// Registers the purge handler on `worker` and its daily schedule on `scheduler`.
///
/// # Errors
/// An invalid schedule.
pub fn register(
    service: ItemsService,
    clock: SharedClock,
    worker: &mut jobs::Worker,
    scheduler: &mut jobs::Scheduler,
) -> Result<(), jobs::ScheduleError> {
    worker.register(move |_: PurgeTrash, _| {
        let (service, clock) = (service.clone(), clock.clone());
        async move {
            let purged = service.purge_expired(clock.now()).await?;
            tracing::info!(purged, "expired items purged from the trash");
            Ok(())
        }
    });
    scheduler.add(SCHEDULE_NAME, PURGE_CRON, PurgeTrash {})?;
    Ok(())
}
