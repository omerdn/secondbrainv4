//! Routine reminders (PRD R13, BL2, T2.2.7).
//!
//! A sweep runs every minute. For each active routine with reminder times it finds the times
//! that came due in the last [`REMINDER_WINDOW`], in the routine's own time zone, on days the
//! plan has an occurrence that is not complete yet. Each due reminder is recorded per person in
//! `routine_reminders_sent_to` and announced with [`RoutineReminderDue`] in one transaction, so
//! each person gets it at most once; the notification center turns the event into a
//! notification and honours that person's channels (BL3).
//!
//! Who gets it (R5, R6, T3.3.4): in a rotation, the person responsible for that occurrence (swaps
//! included); with fixed assignees, everyone in a shared pool until the day is complete, and in
//! each-person mode those below their target; with no assignees, the owner. Only people who can
//! still view the routine are reminded (V9).

use access::Perms;
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use kernel::AppError;
use kernel::event::DomainEvent;
use recurrence::local_date;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::completions::schedule_of;
use crate::model::{parse_timezone, target_on};
use crate::repo;
use crate::service::RoutinesService;

/// How late a reminder may still go out: the sweep catches up after a short outage, but a
/// reminder hours late is no reminder.
pub const REMINDER_WINDOW: Duration = Duration::minutes(15);
/// Every minute (6-field cron, UTC).
pub const REMINDER_CRON: &str = "0 * * * * *";

/// A reminder came due (published in the sweep's transaction).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineReminderDue {
    pub routine_id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub local_date: NaiveDate,
    /// `HH:MM` local time.
    pub time: String,
}

impl DomainEvent for RoutineReminderDue {
    const EVENT_TYPE: &'static str = "routine.reminder_due";
}

/// The reminder times (local day and time) that came due in `(now - window, now]` in
/// `timezone`. Today and yesterday are looked at, so a time just before midnight is caught after
/// it. A time that does not exist on a day (a DST gap) is skipped that day; a repeated one (a
/// DST fold) counts once, at its first instant.
#[must_use]
pub fn due_reminders(
    now: DateTime<Utc>,
    timezone: Tz,
    times: &[NaiveTime],
    window: Duration,
) -> Vec<(NaiveDate, NaiveTime)> {
    let today = local_date(now, timezone);
    let days = [today.pred_opt(), Some(today)];
    let mut due = Vec::new();
    for day in days.into_iter().flatten() {
        for &time in times {
            let Some(at) = timezone.from_local_datetime(&day.and_time(time)).earliest() else {
                continue;
            };
            let at = at.with_timezone(&Utc);
            if at > now - window && at <= now {
                due.push((day, time));
            }
        }
    }
    due
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl RoutinesService {
    /// Sends the reminders due now (see the module docs); how many went out.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors; reminders sent before the error stay sent.
    pub async fn send_due_reminders(&self) -> Result<usize, AppError> {
        let now = self.clock.now();
        let rows = repo::routines_with_reminders(&self.pool)
            .await
            .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let items = self.items.active_items_for_jobs(&ids).await?;
        let mut sent = 0;
        for row in rows {
            let Some(item) = items.iter().find(|item| item.id == row.item_id) else {
                continue;
            };
            let Ok(timezone) = parse_timezone(&row.timezone) else {
                continue;
            };
            let due = due_reminders(now, timezone, &row.reminder_times, REMINDER_WINDOW);
            if due.is_empty() {
                continue;
            }
            let versions = repo::list_versions(&self.pool, row.item_id)
                .await
                .map_err(internal)?;
            let schedule = schedule_of(timezone, &versions)?;
            for (day, time) in due {
                if day < row.start_date
                    || row.end_date.is_some_and(|end| end < day)
                    || !schedule.occurs_on(day)
                {
                    continue;
                }
                let entries =
                    tracking::repo::list_entries(&self.pool, row.item_id, Some(day), Some(day))
                        .await
                        .map_err(internal)?;
                let people = self.people(row.item_id).await?;
                let mut recipients = self.counts(&row, &entries).await?.still_to_do(
                    day,
                    target_on(&versions, day),
                    &people,
                );
                if !recipients.is_empty()
                    && let Some(responsible) = self.responsible_on(&row, day).await?
                {
                    // R6: a rotation (always a shared pool) hands the day to one person.
                    recipients = vec![responsible];
                }
                for user_id in recipients {
                    if !self
                        .permissions
                        .check(&self.pool, user_id, row.item_id, Perms::VIEW)
                        .await
                        .map_err(internal)?
                    {
                        continue;
                    }
                    let mut tx = self.pool.begin().await.map_err(internal)?;
                    if !repo::record_reminder(&mut tx, (row.item_id, user_id), day, time, now)
                        .await
                        .map_err(internal)?
                    {
                        continue;
                    }
                    db::outbox::publish(
                        &mut tx,
                        &RoutineReminderDue {
                            routine_id: row.item_id,
                            user_id,
                            title: item.title.clone(),
                            local_date: day,
                            time: time.format("%H:%M").to_string(),
                        },
                    )
                    .await
                    .map_err(internal)?;
                    tx.commit().await.map_err(internal)?;
                    sent += 1;
                }
            }
        }
        Ok(sent)
    }
}

/// The scheduled sweep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRoutineReminders {}

impl jobs::Job for SendRoutineReminders {
    const KIND: &'static str = "routines.send_reminders";
    const MAX_ATTEMPTS: u32 = 1;
}

/// Schedules the reminder sweep (call from `app::run`).
///
/// # Errors
/// [`jobs::ScheduleError`] for a bad schedule.
pub fn register(
    (worker, scheduler): (&mut jobs::Worker, &mut jobs::Scheduler),
    routines: RoutinesService,
) -> Result<(), jobs::ScheduleError> {
    worker.register(move |_: SendRoutineReminders, _| {
        let routines = routines.clone();
        async move {
            routines
                .send_due_reminders()
                .await
                .map_err(|err| jobs::HandlerError::from(Box::new(err) as Box<_>))?;
            Ok(())
        }
    });
    scheduler.add(
        "routines-send-reminders",
        REMINDER_CRON,
        SendRoutineReminders {},
    )?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::NaiveDate;
    use proptest::prelude::*;

    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn a_reminder_is_due_at_its_local_time_and_shortly_after() {
        let istanbul: Tz = "Europe/Istanbul".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 27).unwrap();
        // 08:00 in Istanbul is 05:00 UTC.
        assert!(
            due_reminders(
                at(2026, 11, 27, 4, 59),
                istanbul,
                &[hm(8, 0)],
                REMINDER_WINDOW
            )
            .is_empty()
        );
        assert_eq!(
            due_reminders(
                at(2026, 11, 27, 5, 0),
                istanbul,
                &[hm(8, 0)],
                REMINDER_WINDOW
            ),
            [(day, hm(8, 0))]
        );
        assert_eq!(
            due_reminders(
                at(2026, 11, 27, 5, 14),
                istanbul,
                &[hm(8, 0)],
                REMINDER_WINDOW
            )
            .len(),
            1
        );
        assert!(
            due_reminders(
                at(2026, 11, 27, 5, 15),
                istanbul,
                &[hm(8, 0)],
                REMINDER_WINDOW
            )
            .is_empty()
        );
        // In UTC the same instant is not 08:00.
        assert!(
            due_reminders(
                at(2026, 11, 27, 5, 0),
                chrono_tz::UTC,
                &[hm(8, 0)],
                REMINDER_WINDOW
            )
            .is_empty()
        );
    }

    #[test]
    fn a_reminder_before_midnight_is_caught_after_it() {
        let istanbul: Tz = "Europe/Istanbul".parse().unwrap();
        // 23:55 on the 26th in Istanbul is 20:55 UTC; the sweep runs at 21:02 UTC (00:02 local).
        assert_eq!(
            due_reminders(
                at(2026, 11, 26, 21, 2),
                istanbul,
                &[hm(23, 55)],
                REMINDER_WINDOW
            ),
            [(NaiveDate::from_ymd_opt(2026, 11, 26).unwrap(), hm(23, 55))]
        );
    }

    #[test]
    fn dst_gaps_are_skipped_and_folds_count_once() {
        let ny: Tz = "America/New_York".parse().unwrap();
        // 8 March 2026: 02:30 does not exist in New York.
        for minute in 0..180 {
            let now = at(2026, 3, 8, 6, 0) + Duration::minutes(minute);
            assert!(due_reminders(now, ny, &[hm(2, 30)], REMINDER_WINDOW).is_empty());
        }
        // 1 November 2026: 01:30 happens twice; only the first (05:30 UTC) is due.
        let hits: usize = (0..240)
            .map(|minute| {
                due_reminders(
                    at(2026, 11, 1, 4, 0) + Duration::minutes(minute),
                    ny,
                    &[hm(1, 30)],
                    Duration::minutes(1),
                )
                .len()
            })
            .sum();
        assert_eq!(hits, 1);
    }

    proptest! {
        /// Sweeping every minute finds each reminder exactly once a day.
        #[test]
        fn a_minute_sweep_finds_each_time_once_a_day(h in 0u32..24, m in 0u32..60, zone in 0usize..4) {
            let zones = ["Europe/Istanbul", "Asia/Tokyo", "America/Sao_Paulo", "Pacific/Kiritimati"];
            let tz: Tz = zones[zone].parse().unwrap();
            let start = at(2026, 6, 10, 0, 0);
            let hits: usize = (0..24 * 60)
                .map(|minute| due_reminders(start + Duration::minutes(minute), tz, &[hm(h, m)], Duration::minutes(1)).len())
                .sum();
            prop_assert_eq!(hits, 1);
        }
    }
}
