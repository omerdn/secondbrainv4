//! The weekly summary (PRD OZ1, ARCHITECTURE §14, T4.3.5).
//!
//! Every hour [`WeeklySummaryTick`] looks for people whose local time is Monday 09:00–09:59 and
//! queues one [`SendWeeklySummary`] each (once per week: the dedupe key holds the week). That job
//! reads last week through the modules' services **as the user** (so only what they see), turns
//! it into a [`WeeklySummary`] with the pure [`summarize`], and hands it to the notification
//! center, which follows the user's channels for `weekly_summary` (BL3): in-app, e-mail (the
//! default) and push. A week with nothing to tell sends nothing.
//!
//! It lives in `app` because it reads through many modules (like `exports`).

use std::str::FromStr;

use chrono::{DateTime, Datelike, Days, NaiveDate, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use kernel::{AppError, SharedClock};
use notifications::{
    NotificationKind, NotificationPayload, Notifications, SummaryGoal, SummaryRoutine,
    SummarySession, WeeklySummary,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Every hour on the hour (UTC); each user's local Monday morning falls in one of them.
pub const TICK_CRON: &str = "0 0 * * * *";
pub const TICK_SCHEDULE: &str = "weekly-summary";
/// The local hour the summary goes out on Mondays.
pub const SEND_HOUR: u32 = 9;
/// Forgotten items listed at most.
pub const FORGOTTEN_MAX: usize = 5;
/// Users read per page by the tick.
const PAGE: i64 = 500;

/// The time zone of a user, UTC when unknown.
#[must_use]
pub fn timezone(name: &str) -> Tz {
    Tz::from_str(name).unwrap_or(Tz::UTC)
}

/// Whether `now` is the user's Monday send hour.
#[must_use]
pub fn is_send_time(now: DateTime<Utc>, tz: Tz) -> bool {
    let local = now.with_timezone(&tz);
    local.weekday() == Weekday::Mon && local.hour() == SEND_HOUR
}

/// The last full week (Monday to Sunday) before `now`, in the user's time zone.
#[must_use]
pub fn previous_week(now: DateTime<Utc>, tz: Tz) -> (NaiveDate, NaiveDate) {
    let today = now.with_timezone(&tz).date_naive();
    let this_monday = today
        .checked_sub_days(Days::new(u64::from(today.weekday().num_days_from_monday())))
        .unwrap_or(today);
    let start = this_monday
        .checked_sub_days(Days::new(7))
        .unwrap_or(this_monday);
    let end = this_monday.pred_opt().unwrap_or(this_monday);
    (start, end)
}

/// One routine's week as read from `routines`.
#[derive(Debug, Clone)]
pub struct RoutineWeek {
    pub id: Uuid,
    pub title: String,
    pub days: Vec<routines::RoutineDay>,
    pub streak: u32,
}

/// Whole percent of a 0–1 share, clamped.
fn percent(share: f64) -> u32 {
    if share.is_finite() {
        // Clamped to 0..=1000 first, so the cast cannot truncate or wrap.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let value = (share.clamp(0.0, 10.0) * 100.0).round() as u32;
        value
    } else {
        0
    }
}

fn minutes(ms: i64) -> u32 {
    u32::try_from(ms.max(0) / 60_000).unwrap_or(u32::MAX)
}

/// The summary of a week from what the services returned (OZ1). Pure: no clock, no I/O.
/// Routines that were not due that week, and sessions under a minute, are left out.
#[must_use]
pub fn summarize(
    (week_start, week_end): (NaiveDate, NaiveDate),
    routines: &[RoutineWeek],
    goals: &[goals::ActiveGoal],
    sessions: &[(Option<Uuid>, String, i64)],
    forgotten: &[String],
) -> WeeklySummary {
    let in_week = |date: NaiveDate| date >= week_start && date <= week_end;
    let routines = routines
        .iter()
        .filter_map(|routine| {
            let due: Vec<&routines::RoutineDay> = routine
                .days
                .iter()
                .filter(|day| day.scheduled && in_week(day.date))
                .collect();
            if due.is_empty() {
                return None;
            }
            Some(SummaryRoutine {
                routine_id: routine.id,
                title: routine.title.clone(),
                done_days: u32::try_from(due.iter().filter(|day| day.complete).count())
                    .unwrap_or(u32::MAX),
                scheduled_days: u32::try_from(due.len()).unwrap_or(u32::MAX),
                streak: routine.streak,
            })
        })
        .collect();
    let goals = goals
        .iter()
        .map(|goal| SummaryGoal {
            goal_id: goal.item.id,
            title: goal.item.title.clone(),
            actual_percent: percent(goal.actual),
            expected_percent: percent(goal.expected),
        })
        .collect();
    let mut sessions: Vec<SummarySession> = sessions
        .iter()
        .filter(|(_, _, ms)| minutes(*ms) > 0)
        .map(|(item_id, title, ms)| SummarySession {
            item_id: *item_id,
            title: title.clone(),
            minutes: minutes(*ms),
        })
        .collect();
    sessions.sort_by(|a, b| b.minutes.cmp(&a.minutes).then(a.title.cmp(&b.title)));
    let work_minutes = sessions.iter().map(|s| s.minutes).sum();
    WeeklySummary {
        week_start,
        week_end,
        routines,
        goals,
        sessions,
        work_minutes,
        forgotten: forgotten.iter().take(FORGOTTEN_MAX).cloned().collect(),
    }
}

/// A stable id for "the summary of `week_start` for `user`", so a retried job never makes a
/// second notification or e-mail.
#[must_use]
pub fn source_event(user: Uuid, week_start: NaiveDate) -> Uuid {
    let digest = Sha256::digest(format!("weekly-summary:{user}:{week_start}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

/// The hourly scan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeeklySummaryTick {}

impl jobs::Job for WeeklySummaryTick {
    const KIND: &'static str = "summary.weekly_tick";
}

/// One user's summary of the week starting `week_start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendWeeklySummary {
    pub user_id: Uuid,
    #[serde(with = "date")]
    pub week_start: NaiveDate,
}

mod date {
    use chrono::NaiveDate;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(date: &NaiveDate, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&date.to_string())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveDate, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl jobs::Job for SendWeeklySummary {
    const KIND: &'static str = "summary.weekly";
    const MAX_ATTEMPTS: u32 = 5;
}

/// The services the summary reads, all permission-checked.
#[derive(Clone)]
pub struct WeeklySummaries {
    pub pool: sqlx::PgPool,
    pub clock: SharedClock,
    pub identity: identity::IdentityService,
    pub items: items::ItemsService,
    pub routines: routines::RoutinesService,
    pub sessions: sessions::WorkSessions,
    pub today: today::TodayService,
    pub notifications: Notifications,
}

impl WeeklySummaries {
    /// Queues the summaries of everyone whose local Monday send hour it is.
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
                let tz = timezone(&zone);
                if !is_send_time(now, tz) {
                    continue;
                }
                let (week_start, _) = previous_week(now, tz);
                let mut conn = self.pool.acquire().await.map_err(AppError::internal)?;
                let id = jobs::enqueue(
                    &mut conn,
                    &SendWeeklySummary {
                        user_id: user,
                        week_start,
                    },
                    jobs::EnqueueOptions {
                        run_at: None,
                        dedupe_key: Some(format!("weekly-summary:{user}:{week_start}")),
                    },
                )
                .await
                .map_err(AppError::internal)?;
                queued += usize::from(id.is_some());
            }
        }
        Ok(queued)
    }

    /// Reads `user`'s week through the services, as them.
    ///
    /// # Errors
    /// [`AppError::Internal`]; [`AppError::Unauthenticated`] once the user is gone.
    pub async fn build(
        &self,
        user: Uuid,
        week_start: NaiveDate,
    ) -> Result<WeeklySummary, AppError> {
        let profile = self
            .identity
            .current_user(identity::UserId::from_uuid(user))
            .await?;
        let tz = timezone(&profile.timezone);
        let week_end = week_start
            .checked_add_days(Days::new(6))
            .unwrap_or(week_start);
        let monday = week_end.succ_opt().unwrap_or(week_end);
        // The Today screen as of this Monday: routines, active goals and forgotten items.
        let screen = self.today.today(user, Some(monday)).await?;
        let mut routines = Vec::new();
        for routine in &screen.routines {
            let id = routine.item.id;
            let days = self
                .routines
                .days(
                    user,
                    id,
                    routines::RoutineDaysQuery {
                        from: Some(week_start),
                        to: Some(week_end),
                    },
                )
                .await;
            let streak = self.routines.streak(user, id).await;
            match (days, streak) {
                (Ok(days), Ok(streak)) => routines.push(RoutineWeek {
                    id,
                    title: routine.item.title.clone(),
                    days,
                    streak: streak.current,
                }),
                (Err(AppError::NotFound), _) | (_, Err(AppError::NotFound)) => {}
                (Err(err), _) | (_, Err(err)) => return Err(err),
            }
        }
        let stats = self
            .sessions
            .stats(user, (Some(week_start), Some(week_end)), tz)
            .await?;
        // One row per item already.
        let mut sessions = Vec::new();
        for item in &stats.items {
            let title = match item.item_id {
                Some(id) => match self.items.get(user, id).await {
                    Ok(found) => found.title,
                    // Hidden or gone since: its time is not attributed to a title.
                    Err(AppError::NotFound) => continue,
                    Err(err) => return Err(err),
                },
                None => "Genel".to_owned(),
            };
            sessions.push((item.item_id, title, item.work_ms));
        }
        let forgotten: Vec<String> = screen.forgotten.iter().map(|i| i.title.clone()).collect();
        Ok(summarize(
            (week_start, week_end),
            &routines,
            &screen.goals,
            &sessions,
            &forgotten,
        ))
    }

    /// Builds and delivers one summary; a week with nothing to tell sends nothing.
    ///
    /// # Errors
    /// Retryable errors.
    pub async fn send(&self, user: Uuid, week_start: NaiveDate) -> Result<(), AppError> {
        let summary = match self.build(user, week_start).await {
            Ok(summary) => summary,
            // The account is gone: nothing to send, nothing to retry.
            Err(AppError::Unauthenticated) => return Ok(()),
            Err(err) => return Err(err),
        };
        if summary.is_empty() {
            return Ok(());
        }
        let event = source_event(user, week_start);
        self.notifications
            .deliver(
                user,
                &NotificationPayload::WeeklySummary(summary.clone()),
                event,
            )
            .await?;
        self.notifications
            .queue_email(
                user,
                NotificationKind::WeeklySummary,
                notifications::email::Template::WeeklySummary(summary),
                event,
            )
            .await?;
        Ok(())
    }
}

/// Registers the hourly tick and the per-user job (call from `app::run`).
///
/// # Errors
/// An invalid schedule.
pub fn register(
    summaries: WeeklySummaries,
    worker: &mut jobs::Worker,
    scheduler: &mut jobs::Scheduler,
) -> Result<(), jobs::ScheduleError> {
    let ticker = summaries.clone();
    worker.register(move |_: WeeklySummaryTick, _| {
        let summaries = ticker.clone();
        async move {
            let queued = summaries.tick().await?;
            if queued > 0 {
                tracing::info!(target: "summary", queued, "weekly summaries queued");
            }
            Ok(())
        }
    });
    worker.register(move |job: SendWeeklySummary, _| {
        let summaries = summaries.clone();
        async move {
            summaries.send(job.user_id, job.week_start).await?;
            Ok(())
        }
    });
    scheduler.add(TICK_SCHEDULE, TICK_CRON, WeeklySummaryTick {})?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use chrono::TimeZone;

    use super::*;

    fn day(date: &str, scheduled: bool, complete: bool) -> routines::RoutineDay {
        routines::routine_day(date.parse().unwrap(), scheduled, 1, i32::from(complete))
    }

    #[test]
    fn oz1_sent_on_the_users_monday_morning() {
        let istanbul = timezone("Europe/Istanbul");
        // Monday 09:30 in Istanbul is 06:30 UTC.
        let monday = Utc.with_ymd_and_hms(2026, 11, 23, 6, 30, 0).unwrap();
        assert!(is_send_time(monday, istanbul));
        assert!(!is_send_time(monday, Tz::UTC), "06:30 in UTC");
        assert!(is_send_time(monday + chrono::Duration::hours(3), Tz::UTC));
        assert!(!is_send_time(monday + chrono::Duration::days(1), istanbul));
        // Monday 09:00 in Tokyo is still Sunday in UTC.
        let tokyo = Utc.with_ymd_and_hms(2026, 11, 23, 0, 0, 0).unwrap();
        assert!(is_send_time(tokyo, timezone("Asia/Tokyo")));
        assert_eq!(timezone("Nowhere/Land"), Tz::UTC);
    }

    #[test]
    fn oz1_the_week_is_last_monday_to_sunday_locally() {
        let monday = Utc.with_ymd_and_hms(2026, 11, 23, 6, 30, 0).unwrap();
        let week = previous_week(monday, timezone("Europe/Istanbul"));
        assert_eq!(week.0.to_string(), "2026-11-16");
        assert_eq!(week.1.to_string(), "2026-11-22");
        // Same instant, but it is still Sunday the 22nd in Los Angeles.
        let la = previous_week(monday, timezone("America/Los_Angeles"));
        assert_eq!(la.0.to_string(), "2026-11-09");
        assert_eq!(la.1.to_string(), "2026-11-15");
    }

    #[test]
    fn oz1_summarize_counts_due_days_rounds_goals_and_sorts_time() {
        let week = ("2026-11-16".parse().unwrap(), "2026-11-22".parse().unwrap());
        let routine = RoutineWeek {
            id: Uuid::now_v7(),
            title: "Su iç".into(),
            days: vec![
                day("2026-11-15", true, true), // the week before: ignored
                day("2026-11-16", true, true),
                day("2026-11-17", true, false),
                day("2026-11-18", false, false), // a day off
                day("2026-11-19", true, true),
            ],
            streak: 1,
        };
        let idle = RoutineWeek {
            id: Uuid::now_v7(),
            title: "Hiç yok".into(),
            days: vec![day("2026-11-16", false, false)],
            streak: 0,
        };
        let sessions = vec![
            (None, "Genel".to_owned(), 30 * 60_000),
            (Some(Uuid::now_v7()), "Rapor".to_owned(), 95 * 60_000),
            (Some(Uuid::now_v7()), "Kısa".to_owned(), 59_000),
        ];
        let forgotten: Vec<String> = (0..8).map(|i| format!("Eski {i}")).collect();
        let summary = summarize(week, &[routine, idle], &[], &sessions, &forgotten);
        assert_eq!(summary.routines.len(), 1, "a routine never due is left out");
        assert_eq!(summary.routines[0].done_days, 2);
        assert_eq!(summary.routines[0].scheduled_days, 3);
        let titles: Vec<&str> = summary.sessions.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Rapor", "Genel"],
            "longest first, under a minute dropped"
        );
        assert_eq!(summary.work_minutes, 125);
        assert_eq!(summary.forgotten.len(), FORGOTTEN_MAX);
        assert!(!summary.is_empty());
        assert!(summarize(week, &[], &[], &[], &[]).is_empty());
        assert_eq!(percent(0.456), 46);
        assert_eq!(percent(f64::NAN), 0);
        assert_eq!(percent(-1.0), 0);
    }

    #[test]
    fn a_week_has_one_stable_event() {
        let user = Uuid::now_v7();
        let week: NaiveDate = "2026-11-16".parse().unwrap();
        assert_eq!(source_event(user, week), source_event(user, week));
        assert_ne!(
            source_event(user, week),
            source_event(user, week.succ_opt().unwrap())
        );
    }
}
