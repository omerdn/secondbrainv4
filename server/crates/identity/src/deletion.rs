//! Account deletion (PRD HS2, ARCHITECTURE §14, T4.3.1).
//!
//! A user asks for their account to be deleted; `users.deletion_requested_at` records when.
//! For [`GRACE`] (30 days) they can still sign in and change their mind. Meanwhile the client
//! guides them to hand over the shared items they own (HS2.1). When the grace period is over,
//! the daily [`DeleteDueAccounts`] job runs every module's [`AccountDeletionHook`] (shared items
//! go to their oldest member, the rest is removed) and then deletes the `users` row: sessions,
//! sign-in methods, e-mail tokens, devices, push subscriptions, notifications, friendships and
//! the like go with it through `ON DELETE CASCADE`.
//!
//! Hooks run before the row is deleted and must be idempotent: a failed run is retried by
//! `jobs` and starts over.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use kernel::AppError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::UserId;
use crate::service::IdentityService;

/// How long a deletion request can be cancelled (HS2.2).
pub const GRACE: Duration = Duration::days(30);

/// Accounts deleted per job run; the rest wait for the next day.
pub const BATCH: i64 = 100;

/// Every day at 04:30 UTC, after the trash purge.
pub const DELETION_CRON: &str = "0 30 4 * * *";
pub const SCHEDULE_NAME: &str = "identity-account-deletion";

/// When an account asked for deletion on `requested_at` is deleted.
#[must_use]
pub fn deletes_at(requested_at: DateTime<Utc>) -> DateTime<Utc> {
    requested_at + GRACE
}

/// Whether the grace period of a request is over at `now`.
#[must_use]
pub fn is_due(requested_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    deletes_at(requested_at) <= now
}

/// `GET /account/deletion` and the answer of a request or a cancel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct AccountDeletion {
    /// When the user asked; `null` when no deletion is pending.
    #[ts(type = "string | null")]
    pub requested_at: Option<DateTime<Utc>>,
    /// When the account will be deleted; `null` when no deletion is pending.
    #[ts(type = "string | null")]
    pub deletes_at: Option<DateTime<Utc>>,
}

impl AccountDeletion {
    fn of(requested_at: Option<DateTime<Utc>>) -> Self {
        Self {
            requested_at,
            deletes_at: requested_at.map(deletes_at),
        }
    }
}

/// Boxed future of [`AccountDeletionHook::before_delete`].
pub type HookFuture<'a> = Pin<Box<dyn Future<Output = Result<(), AppError>> + Send + 'a>>;

/// A module's part of deleting an account: hand over or remove what it keeps about `user`
/// without a foreign key to `users`. Must be idempotent.
pub trait AccountDeletionHook: Send + Sync {
    /// A stable name for logs.
    fn name(&self) -> &'static str;
    /// Runs before the `users` row is deleted.
    fn before_delete(&self, user: Uuid) -> HookFuture<'_>;
}

pub type SharedDeletionHook = Arc<dyn AccountDeletionHook>;

impl IdentityService {
    /// The caller's pending deletion, if any.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an unknown user, [`AppError::Internal`] on database errors.
    pub async fn account_deletion(&self, user: UserId) -> Result<AccountDeletion, AppError> {
        let requested: Option<Option<DateTime<Utc>>> =
            sqlx::query_scalar("SELECT deletion_requested_at FROM users WHERE id = $1")
                .bind(user.as_uuid())
                .fetch_optional(self.pool())
                .await
                .map_err(AppError::internal)?;
        requested.map(AccountDeletion::of).ok_or(AppError::NotFound)
    }

    /// Asks for the caller's account to be deleted after [`GRACE`]. Asking again keeps the
    /// first request's date.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an unknown user, [`AppError::Internal`] on database errors.
    pub async fn request_deletion(&self, user: UserId) -> Result<AccountDeletion, AppError> {
        let requested: Option<Option<DateTime<Utc>>> = sqlx::query_scalar(
            "UPDATE users SET deletion_requested_at = COALESCE(deletion_requested_at, $2)
             WHERE id = $1 RETURNING deletion_requested_at",
        )
        .bind(user.as_uuid())
        .bind(self.now())
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?;
        let deletion = requested
            .map(AccountDeletion::of)
            .ok_or(AppError::NotFound)?;
        tracing::info!(target: "identity", "account deletion requested");
        Ok(deletion)
    }

    /// Cancels the caller's pending deletion; nothing changes when there is none.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an unknown user, [`AppError::Internal`] on database errors.
    pub async fn cancel_deletion(&self, user: UserId) -> Result<AccountDeletion, AppError> {
        let found = sqlx::query("UPDATE users SET deletion_requested_at = NULL WHERE id = $1")
            .bind(user.as_uuid())
            .execute(self.pool())
            .await
            .map_err(AppError::internal)?
            .rows_affected();
        if found == 0 {
            return Err(AppError::NotFound);
        }
        tracing::info!(target: "identity", "account deletion cancelled");
        Ok(AccountDeletion::of(None))
    }

    /// Deletes up to [`BATCH`] accounts whose grace period is over at the service clock's now:
    /// runs `hooks` for each, then deletes the user. Returns how many were deleted.
    ///
    /// # Errors
    /// The first hook or database error; accounts deleted before it stay deleted.
    pub async fn delete_due_accounts(
        &self,
        hooks: &[SharedDeletionHook],
    ) -> Result<usize, AppError> {
        let cutoff = self.now() - GRACE;
        let due: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM users WHERE deletion_requested_at <= $1
             ORDER BY deletion_requested_at, id LIMIT $2",
        )
        .bind(cutoff)
        .bind(BATCH)
        .fetch_all(self.pool())
        .await
        .map_err(AppError::internal)?;
        let mut deleted = 0;
        for user in due {
            for hook in hooks {
                hook.before_delete(user).await.inspect_err(|_| {
                    tracing::warn!(target: "identity", hook = hook.name(), "deletion hook failed");
                })?;
            }
            // Re-checked: a cancel since the hooks ran keeps the account (whatever the hooks
            // already handed over stays handed over).
            let gone =
                sqlx::query("DELETE FROM users WHERE id = $1 AND deletion_requested_at <= $2")
                    .bind(user)
                    .bind(cutoff)
                    .execute(self.pool())
                    .await
                    .map_err(AppError::internal)?
                    .rows_affected();
            deleted += usize::try_from(gone).unwrap_or(0);
        }
        Ok(deleted)
    }
}

/// The daily deletion job (`identity.delete_due_accounts`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeleteDueAccounts {}

impl jobs::Job for DeleteDueAccounts {
    const KIND: &'static str = "identity.delete_due_accounts";
}

/// Registers the deletion job and its daily schedule (call from `app::run` once every module's
/// hook exists).
///
/// # Errors
/// An invalid schedule.
pub fn register(
    service: IdentityService,
    hooks: Vec<SharedDeletionHook>,
    worker: &mut jobs::Worker,
    scheduler: &mut jobs::Scheduler,
) -> Result<(), jobs::ScheduleError> {
    let hooks: Arc<[SharedDeletionHook]> = hooks.into();
    worker.register(move |_: DeleteDueAccounts, _| {
        let (service, hooks) = (service.clone(), hooks.clone());
        async move {
            let deleted = service.delete_due_accounts(&hooks).await?;
            tracing::info!(target: "identity", deleted, "due accounts deleted");
            Ok(())
        }
    });
    scheduler.add(SCHEDULE_NAME, DELETION_CRON, DeleteDueAccounts {})?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::str::FromStr;

    use chrono::TimeZone;

    use super::*;

    #[test]
    fn hs2_an_account_goes_thirty_days_after_the_request() {
        let asked = Utc.with_ymd_and_hms(2026, 11, 27, 10, 0, 0).unwrap();
        assert_eq!(
            deletes_at(asked),
            Utc.with_ymd_and_hms(2026, 12, 27, 10, 0, 0).unwrap()
        );
        assert!(!is_due(
            asked,
            asked + Duration::days(30) - Duration::seconds(1)
        ));
        assert!(is_due(asked, asked + Duration::days(30)));
        assert_eq!(AccountDeletion::of(None).deletes_at, None);
    }

    #[test]
    fn cron_runs_once_a_day() {
        let schedule = cron::Schedule::from_str(DELETION_CRON).unwrap();
        let mut next = schedule.upcoming(Utc).take(2);
        let (a, b) = (next.next().unwrap(), next.next().unwrap());
        assert_eq!(b - a, Duration::days(1));
    }
}
