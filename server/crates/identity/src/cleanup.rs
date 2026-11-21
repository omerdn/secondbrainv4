//! Daily removal of expired identity rows (T0.3.8, ARCHITECTURE §14).
//!
//! Expired rows no longer authenticate anything (every lookup checks `expires_at`), so this only
//! keeps the tables small: `sessions`, `email_tokens` and `oauth_states`, each through its
//! `expires_at` index. Rows are deleted in batches of [`BATCH`] with one short statement per
//! batch, so no long lock is held. Running it twice, or on two instances at once, is harmless.

use chrono::{DateTime, Utc};
use kernel::AppError;
use serde::{Deserialize, Serialize};

use crate::service::IdentityService;

/// Rows deleted per statement.
pub const BATCH: i64 = 1000;

/// Cron expression for [`CleanupExpired`] (every day 03:45 UTC, after `idempotency.cleanup`).
pub const CLEANUP_CRON: &str = "0 45 3 * * *";

/// Schedule name of [`CleanupExpired`].
pub const SCHEDULE_NAME: &str = "identity-cleanup";

/// Job payload for the daily cleanup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanupExpired {}

impl jobs::Job for CleanupExpired {
    const KIND: &'static str = "identity.cleanup";
}

/// Rows deleted by one cleanup run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub sessions: u64,
    pub email_tokens: u64,
    pub oauth_states: u64,
}

/// The tables cleaned, as fixed SQL (never built from input).
#[derive(Debug, Clone, Copy)]
enum Table {
    Sessions,
    EmailTokens,
    OauthStates,
}

impl Table {
    fn delete_batch_sql(self) -> &'static str {
        // `ctid` addresses the physical row, so the batch works for any primary key.
        match self {
            Self::Sessions => {
                "DELETE FROM sessions WHERE ctid IN (
                     SELECT ctid FROM sessions WHERE expires_at <= $1 LIMIT $2)"
            }
            Self::EmailTokens => {
                "DELETE FROM email_tokens WHERE ctid IN (
                     SELECT ctid FROM email_tokens WHERE expires_at <= $1 LIMIT $2)"
            }
            Self::OauthStates => {
                "DELETE FROM oauth_states WHERE ctid IN (
                     SELECT ctid FROM oauth_states WHERE expires_at <= $1 LIMIT $2)"
            }
        }
    }
}

/// Whether another batch is needed after one that deleted `deleted` rows.
fn more_batches(deleted: u64, batch: i64) -> bool {
    u64::try_from(batch).is_ok_and(|batch| deleted >= batch)
}

impl IdentityService {
    /// Deletes every session, e-mail token and OAuth state whose `expires_at` is not after the
    /// service clock's `now`.
    pub async fn delete_expired(&self) -> Result<CleanupReport, AppError> {
        let now = self.now();
        Ok(CleanupReport {
            sessions: self.delete_expired_in(Table::Sessions, now).await?,
            email_tokens: self.delete_expired_in(Table::EmailTokens, now).await?,
            oauth_states: self.delete_expired_in(Table::OauthStates, now).await?,
        })
    }

    async fn delete_expired_in(&self, table: Table, now: DateTime<Utc>) -> Result<u64, AppError> {
        let mut total = 0;
        loop {
            let deleted = sqlx::query(table.delete_batch_sql())
                .bind(now)
                .bind(BATCH)
                .execute(self.pool())
                .await
                .map_err(AppError::internal)?
                .rows_affected();
            total += deleted;
            if !more_batches(deleted, BATCH) {
                return Ok(total);
            }
        }
    }
}

/// Registers the cleanup handler on `worker` and its daily schedule on `scheduler`.
pub fn register(
    service: IdentityService,
    worker: &mut jobs::Worker,
    scheduler: &mut jobs::Scheduler,
) -> Result<(), jobs::ScheduleError> {
    worker.register(move |_: CleanupExpired, _| {
        let service = service.clone();
        async move {
            let report = service.delete_expired().await?;
            tracing::info!(
                sessions = report.sessions,
                email_tokens = report.email_tokens,
                oauth_states = report.oauth_states,
                "expired identity rows deleted"
            );
            Ok(())
        }
    });
    scheduler.add(SCHEDULE_NAME, CLEANUP_CRON, CleanupExpired {})?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn a_full_batch_asks_for_another() {
        assert!(more_batches(1000, 1000));
        assert!(!more_batches(999, 1000));
        assert!(!more_batches(0, 1000));
    }

    #[test]
    fn cron_runs_once_a_day() {
        let schedule = cron::Schedule::from_str(CLEANUP_CRON).unwrap();
        let mut upcoming = schedule.upcoming(Utc).take(2);
        let (a, b) = (upcoming.next().unwrap(), upcoming.next().unwrap());
        assert_eq!((b - a).num_hours(), 24);
    }
}
