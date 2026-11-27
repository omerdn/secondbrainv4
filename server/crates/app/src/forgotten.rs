//! The forgotten items reminder (PRD BL2, H4, T4.3.9).
//!
//! Every hour [`ForgottenTick`] looks for people whose local time is [`SEND_HOUR`] and queues one
//! [`SendForgottenReminder`] each, once per local day (the dedupe key holds the date). The job
//! reads the Today screen **as the user**, so the list is exactly its "forgotten" section (items
//! untouched for the user's threshold, H4), and hands a reminder to the notification center,
//! which follows the user's channels for `forgotten_reminder` (BL3). Nothing forgotten: nothing
//! is sent.

use chrono::{DateTime, NaiveDate, Timelike, Utc};
use chrono_tz::Tz;
use kernel::{AppError, SharedClock};
use notifications::{NotificationPayload, Notifications};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::weekly::{PAGE, timezone};

/// The local hour of the reminder.
pub const SEND_HOUR: u32 = 10;
/// Titles named in the reminder at most.
pub const TITLES_MAX: usize = 3;
pub const TICK_CRON: &str = "0 0 * * * *";
pub const TICK_SCHEDULE: &str = "forgotten-reminder";

/// The user's local date when `now` is their reminder hour.
#[must_use]
pub fn reminder_day(now: DateTime<Utc>, tz: Tz) -> Option<NaiveDate> {
    let local = now.with_timezone(&tz);
    (local.hour() == SEND_HOUR).then(|| local.date_naive())
}

/// The reminder for a list of forgotten titles; `None` when there is none.
#[must_use]
pub fn reminder(local_date: NaiveDate, titles: &[String]) -> Option<NotificationPayload> {
    if titles.is_empty() {
        return None;
    }
    Some(NotificationPayload::ForgottenReminder {
        local_date,
        count: u32::try_from(titles.len()).unwrap_or(u32::MAX),
        titles: titles.iter().take(TITLES_MAX).cloned().collect(),
    })
}

/// A stable id for "the reminder of `day` for `user`" (one per day, whatever the retries).
#[must_use]
pub fn source_event(user: Uuid, day: NaiveDate) -> Uuid {
    let digest = Sha256::digest(format!("forgotten-reminder:{user}:{day}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ForgottenTick {}

impl jobs::Job for ForgottenTick {
    const KIND: &'static str = "forgotten.tick";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendForgottenReminder {
    pub user_id: Uuid,
    /// The user's local day, `YYYY-MM-DD`.
    pub local_date: String,
}

impl jobs::Job for SendForgottenReminder {
    const KIND: &'static str = "forgotten.remind";
    const MAX_ATTEMPTS: u32 = 3;
}

#[derive(Clone)]
pub struct ForgottenReminders {
    pub pool: sqlx::PgPool,
    pub clock: SharedClock,
    pub identity: identity::IdentityService,
    pub today: today::TodayService,
    pub notifications: Notifications,
}

impl ForgottenReminders {
    /// Queues today's reminder of everyone whose local reminder hour it is.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn tick(&self) -> Result<usize, AppError> {
        let now = self.clock.now();
        let mut after = None;
        let mut queued = 0;
        loop {
            let page = self.identity.timezones_page(after, PAGE).await?;
            let Some((last, _)) = page.last() else { break };
            after = Some(*last);
            for (user, zone) in page {
                let Some(day) = reminder_day(now, timezone(&zone)) else {
                    continue;
                };
                let mut conn = self.pool.acquire().await.map_err(AppError::internal)?;
                let id = jobs::enqueue(
                    &mut conn,
                    &SendForgottenReminder {
                        user_id: user,
                        local_date: day.to_string(),
                    },
                    jobs::EnqueueOptions {
                        run_at: None,
                        dedupe_key: Some(format!("forgotten-reminder:{user}:{day}")),
                    },
                )
                .await
                .map_err(AppError::internal)?;
                queued += usize::from(id.is_some());
            }
        }
        Ok(queued)
    }

    /// Sends `user`'s reminder of `day` if anything is forgotten.
    ///
    /// # Errors
    /// Retryable errors.
    pub async fn send(&self, user: Uuid, day: NaiveDate) -> Result<(), AppError> {
        let screen = match self.today.today(user, Some(day)).await {
            Ok(screen) => screen,
            // A day the server no longer accepts (a very late retry): skip it.
            Err(AppError::Validation(_)) => return Ok(()),
            Err(err) => return Err(err),
        };
        let titles: Vec<String> = screen.forgotten.iter().map(|i| i.title.clone()).collect();
        let Some(payload) = reminder(day, &titles) else {
            return Ok(());
        };
        match self
            .notifications
            .deliver(user, &payload, source_event(user, day))
            .await
        {
            Ok(_) => Ok(()),
            // Gone since: the notification's foreign key refuses it; nothing to retry.
            Err(err) if !self.user_exists(user).await? => {
                drop(err);
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    async fn user_exists(&self, user: Uuid) -> Result<bool, AppError> {
        match self
            .identity
            .current_user(identity::UserId::from_uuid(user))
            .await
        {
            Ok(_) => Ok(true),
            Err(AppError::Unauthenticated) => Ok(false),
            Err(err) => Err(err),
        }
    }
}

/// Registers the hourly tick and the per-user job (call from `app::run`).
///
/// # Errors
/// An invalid schedule.
pub fn register(
    reminders: ForgottenReminders,
    worker: &mut jobs::Worker,
    scheduler: &mut jobs::Scheduler,
) -> Result<(), jobs::ScheduleError> {
    let ticker = reminders.clone();
    worker.register(move |_: ForgottenTick, _| {
        let reminders = ticker.clone();
        async move {
            let queued = reminders.tick().await?;
            if queued > 0 {
                tracing::info!(target: "forgotten", queued, "forgotten reminders queued");
            }
            Ok(())
        }
    });
    worker.register(move |job: SendForgottenReminder, _| {
        let reminders = reminders.clone();
        async move {
            let day: NaiveDate = job
                .local_date
                .parse()
                .map_err(|err: chrono::ParseError| Box::new(err) as jobs::HandlerError)?;
            reminders.send(job.user_id, day).await?;
            Ok(())
        }
    });
    scheduler.add(TICK_SCHEDULE, TICK_CRON, ForgottenTick {})?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use chrono::TimeZone;

    use super::*;

    #[test]
    fn h4_once_a_day_at_the_users_reminder_hour() {
        let istanbul = timezone("Europe/Istanbul");
        // 07:30 UTC is 10:30 in Istanbul.
        let now = Utc.with_ymd_and_hms(2026, 11, 27, 7, 30, 0).unwrap();
        assert_eq!(
            reminder_day(now, istanbul).unwrap().to_string(),
            "2026-11-27"
        );
        assert_eq!(reminder_day(now, Tz::UTC), None);
        assert_eq!(
            reminder_day(now + chrono::Duration::hours(1), istanbul),
            None
        );
        // 01:00 UTC on the 27th is 10:00 on the 27th in Tokyo.
        let tokyo = Utc.with_ymd_and_hms(2026, 11, 27, 1, 0, 0).unwrap();
        assert_eq!(
            reminder_day(tokyo, timezone("Asia/Tokyo"))
                .unwrap()
                .to_string(),
            "2026-11-27"
        );
    }

    #[test]
    fn h4_nothing_forgotten_nothing_sent_and_a_few_titles_named() {
        let day: NaiveDate = "2026-11-27".parse().unwrap();
        assert!(reminder(day, &[]).is_none());
        let titles: Vec<String> = (1..=5).map(|i| format!("Eski {i}")).collect();
        assert_eq!(
            reminder(day, &titles),
            Some(NotificationPayload::ForgottenReminder {
                local_date: day,
                count: 5,
                titles: vec!["Eski 1".into(), "Eski 2".into(), "Eski 3".into()],
            })
        );
        let user = Uuid::now_v7();
        assert_eq!(source_event(user, day), source_event(user, day));
        assert_ne!(
            source_event(user, day),
            source_event(user, day.succ_opt().unwrap())
        );
    }
}
