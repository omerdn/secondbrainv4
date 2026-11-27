//! Routine completions (PRD R3, R4, R7, ARCHITECTURE §5.6, §7.5, T2.2.2). A completion is an entry
//! of the routine's own tracker, matched to an occurrence by its local day. The rules are pure
//! functions; the clock comes in from outside.

use access::Perms;
use chrono::{DateTime, Days, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use items::TouchAction;
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use recurrence::{PlanVersion, Schedule, local_date};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use tracking::{Aggregation, Direction, NewEntry, NewTracker, OwnerKind, Period};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{PlanVersionView, invalid, parse_timezone, target_on};
use crate::repo;
use crate::service::RoutinesService;

/// How many days back a day can still be marked (R7).
pub const BACKDATE_DAYS: u64 = 3;
/// Most units one request may add.
pub const COUNT_MAX: i32 = 100;
/// Longest range `GET /routines/{id}/completions` returns.
pub const DAYS_MAX: i64 = 400;
/// Days returned when the range is left out.
const DEFAULT_DAYS: u64 = 30;
const TRACKER_UNIT: &str = "count";

/// `POST /routines/{id}/completions` body.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct CompleteRoutineRequest {
    /// The routine's local day; today when absent. At most 3 days back (R7).
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub date: Option<NaiveDate>,
    /// How many to add; 1 when absent (R3).
    #[serde(default)]
    #[ts(optional)]
    pub count: Option<i32>,
}

/// `DELETE /routines/{id}/completions` query.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct UndoCompletionQuery {
    pub date: Option<NaiveDate>,
}

/// `GET /routines/{id}/completions` query: local days, both ends included.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct RoutineDaysQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

/// One local day of a routine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RoutineDay {
    #[ts(type = "string")]
    pub date: NaiveDate,
    /// Whether the plan has an occurrence on this day.
    pub scheduled: bool,
    pub target: i32,
    /// What was done; partial days count (R4), and it can pass the target.
    pub done: i32,
    /// `done` reached `target` on a scheduled day.
    pub complete: bool,
}

/// Whether a day can be marked on `today`: from 3 days back up to today, both included (R7).
#[must_use]
pub fn may_mark_day(date: NaiveDate, today: NaiveDate) -> bool {
    today
        .checked_sub_days(Days::new(BACKDATE_DAYS))
        .is_some_and(|first| (first..=today).contains(&date))
}

/// The state of one day from what was entered on it (R3, R4).
#[must_use]
pub fn routine_day(date: NaiveDate, scheduled: bool, target: i32, done: i32) -> RoutineDay {
    RoutineDay {
        date,
        scheduled,
        target,
        done,
        complete: scheduled && done >= target,
    }
}

pub(crate) fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

pub(crate) fn schedule_of(
    timezone: Tz,
    versions: &[PlanVersionView],
) -> Result<Schedule, AppError> {
    let plans = versions
        .iter()
        .map(|v| {
            Ok(PlanVersion {
                rules: v.rrule.parse().map_err(internal)?,
                valid_from: v.valid_from,
                valid_to: v.valid_to,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Schedule::new(timezone, plans).map_err(internal)
}

fn check_count(count: i32) -> Result<(), AppError> {
    if (1..=COUNT_MAX).contains(&count) {
        return Ok(());
    }
    let mut errors = ValidationErrors::new();
    errors.push(FieldError::new(
        "count",
        FieldErrorCode::OutOfRange,
        format!("must be between 1 and {COUNT_MAX}"),
    ));
    errors.into_result()
}

/// The moment an entry is recorded at: now for today, midday of the local day for a past day.
fn occurred_at(
    date: NaiveDate,
    today: NaiveDate,
    now: DateTime<Utc>,
    timezone: Tz,
) -> DateTime<Utc> {
    if date == today {
        now
    } else {
        tracking::start_of_day(date, timezone) + Duration::hours(12)
    }
}

/// The routine's tracker, created for a routine that does not have one yet.
pub(crate) async fn ensure_tracker(
    conn: &mut PgConnection,
    routine: Uuid,
    target: i32,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    if tracking::repo::get_tracker(&mut *conn, routine)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let definition = NewTracker {
        item_id: routine,
        unit: TRACKER_UNIT.into(),
        target_value: Decimal::from(target),
        period: Period::Day,
        aggregation: Aggregation::Sum,
        direction: Direction::Increase,
        owner_kind: OwnerKind::Routine,
    };
    tracking::repo::insert_tracker(conn, &definition, now).await?;
    Ok(())
}

impl RoutinesService {
    /// Adds to a day (EDIT). A partial day counts (R4); only the last 3 days can be marked, in the
    /// routine's time zone (R7).
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`], [`AppError::Validation`] for a day that
    /// cannot be marked or has no occurrence, [`AppError::Conflict`] for a routine in the trash,
    /// or [`AppError::Internal`].
    pub async fn complete(
        &self,
        actor: Uuid,
        id: Uuid,
        request: CompleteRoutineRequest,
    ) -> Result<RoutineDay, AppError> {
        self.require_live(actor, id).await?;
        let count = request.count.unwrap_or(1);
        check_count(count)?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row = repo::lock_routine(&mut tx, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let timezone = parse_timezone(&row.timezone)?;
        let today = local_date(now, timezone);
        let date = request.date.unwrap_or(today);
        if !may_mark_day(date, today) {
            return Err(invalid("date", "only the last 3 days can be marked"));
        }
        let versions = repo::list_versions(&mut *tx, id).await.map_err(internal)?;
        if !schedule_of(timezone, &versions)?.occurs_on(date) {
            return Err(invalid("date", "the routine has no occurrence on this day"));
        }
        let target = target_on(&versions, date);
        ensure_tracker(&mut tx, id, target, now)
            .await
            .map_err(internal)?;
        let mut entries = tracking::repo::list_entries(&mut *tx, id, Some(date), Some(date))
            .await
            .map_err(internal)?;
        // R5: the day is read under the routine's mode, before and after this entry.
        let (before, day_target) = self.counts(&row, &entries).await?.day(date, target);
        let entry = NewEntry {
            tracker_item_id: id,
            user_id: actor,
            value: Decimal::from(count),
            occurred_at: occurred_at(date, today, now, timezone),
            local_date: date,
            note_block_id: None,
            note: None,
        };
        let added = tracking::repo::insert_entry(&mut *tx, &entry, now)
            .await
            .map_err(internal)?;
        entries.push(added);
        let (done, _) = self.counts(&row, &entries).await?.day(date, target);
        // A1: the day is recorded as completed once, when it reaches its target.
        if before < day_target && done >= day_target {
            self.items
                .record_completed(&mut tx, actor, id)
                .await
                .map_err(internal)?;
        } else {
            self.items
                .touch(&mut tx, id, TouchAction::EntryLogged)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(routine_day(date, true, day_target, done))
    }

    /// Takes back the actor's latest entry on a day (EDIT), within the same 3 days (R7). With
    /// nothing to take back it changes nothing.
    ///
    /// # Errors
    /// As [`Self::complete`].
    pub async fn undo_completion(
        &self,
        actor: Uuid,
        id: Uuid,
        query: UndoCompletionQuery,
    ) -> Result<RoutineDay, AppError> {
        self.require_live(actor, id).await?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row = repo::lock_routine(&mut tx, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let timezone = parse_timezone(&row.timezone)?;
        let today = local_date(now, timezone);
        let date = query.date.unwrap_or(today);
        if !may_mark_day(date, today) {
            return Err(invalid("date", "only the last 3 days can be changed"));
        }
        let versions = repo::list_versions(&mut *tx, id).await.map_err(internal)?;
        let scheduled = schedule_of(timezone, &versions)?.occurs_on(date);
        let target = target_on(&versions, date);
        let mut entries = tracking::repo::list_entries(&mut *tx, id, Some(date), Some(date))
            .await
            .map_err(internal)?;
        if let Some(at) = entries.iter().rposition(|entry| entry.user_id == actor) {
            let entry = entries.remove(at);
            tracking::repo::soft_delete_entry(&mut *tx, id, entry.id, now)
                .await
                .map_err(internal)?;
            self.items
                .touch(&mut tx, id, TouchAction::Edited)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        let (done, day_target) = self.counts(&row, &entries).await?.day(date, target);
        Ok(routine_day(date, scheduled, day_target, done))
    }

    /// The routine's days in a range, oldest first (VIEW). Without a range: the last 30 days.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Validation`] for a bad range, or [`AppError::Internal`].
    pub async fn days(
        &self,
        actor: Uuid,
        id: Uuid,
        query: RoutineDaysQuery,
    ) -> Result<Vec<RoutineDay>, AppError> {
        self.permissions
            .require(&self.pool, actor, id, Perms::VIEW)
            .await?;
        let row = repo::get_routine(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let timezone = parse_timezone(&row.timezone)?;
        let today = local_date(self.clock.now(), timezone);
        let to = query.to.unwrap_or(today);
        let from = query
            .from
            .or_else(|| to.checked_sub_days(Days::new(DEFAULT_DAYS - 1)))
            .unwrap_or(to);
        let span = (to - from).num_days();
        if !(0..DAYS_MAX).contains(&span) {
            return Err(invalid(
                "from",
                format!("the range must be 1 to {DAYS_MAX} days"),
            ));
        }
        let versions = repo::list_versions(&self.pool, id)
            .await
            .map_err(internal)?;
        let schedule = schedule_of(timezone, &versions)?;
        let entries = tracking::repo::list_entries(&self.pool, id, Some(from), Some(to))
            .await
            .map_err(internal)?;
        let counts = self.counts(&row, &entries).await?;
        Ok(from
            .iter_days()
            .take_while(|date| *date <= to)
            .map(|date| {
                let (done, target) = counts.day(date, target_on(&versions, date));
                routine_day(date, schedule.occurs_on(date), target, done)
            })
            .collect())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;

    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn r7_three_days_back_and_never_ahead() {
        let today = day(23);
        assert!(may_mark_day(day(23), today));
        assert!(may_mark_day(day(20), today));
        assert!(!may_mark_day(day(19), today));
        assert!(!may_mark_day(day(24), today));
    }

    #[test]
    fn r7_crosses_month_and_year_ends() {
        let today = NaiveDate::from_ymd_opt(2027, 1, 2).unwrap();
        assert!(may_mark_day(
            NaiveDate::from_ymd_opt(2026, 12, 30).unwrap(),
            today
        ));
        assert!(!may_mark_day(
            NaiveDate::from_ymd_opt(2026, 12, 29).unwrap(),
            today
        ));
    }

    #[test]
    fn r4_a_partial_day_counts_but_is_not_complete() {
        let partial = routine_day(day(1), true, 8, 5);
        assert_eq!(partial.done, 5);
        assert!(!partial.complete);
        assert!(routine_day(day(1), true, 8, 8).complete);
        assert!(routine_day(day(1), true, 8, 9).complete);
        assert!(!routine_day(day(1), false, 1, 3).complete);
    }

    #[test]
    fn a_past_day_is_recorded_inside_that_local_day() {
        let tz = chrono_tz::Asia::Tokyo;
        let now = Utc.with_ymd_and_hms(2026, 11, 23, 12, 0, 0).unwrap();
        let at = occurred_at(day(21), day(23), now, tz);
        assert_eq!(local_date(at, tz), day(21));
        assert_eq!(occurred_at(day(23), day(23), now, tz), now);
    }

    #[test]
    fn r7_at_clock_changes_the_window_stays_in_calendar_days() {
        let tz = chrono_tz::America::New_York;
        // Clocks go forward on 8 March 2026 and back on 1 November 2026.
        for (year, month, d) in [(2026, 3, 8), (2026, 3, 9), (2026, 11, 1), (2026, 11, 2)] {
            let date = NaiveDate::from_ymd_opt(year, month, d).unwrap();
            let at = occurred_at(date, date + Days::new(2), Utc::now(), tz);
            assert_eq!(local_date(at, tz), date);
        }
    }

    proptest! {
        #[test]
        fn r7_holds_in_every_time_zone(
            seconds in 1_750_000_000_i64..1_850_000_000,
            zone in prop::sample::select(vec![
                chrono_tz::UTC,
                chrono_tz::Europe::Istanbul,
                chrono_tz::America::New_York,
                chrono_tz::Asia::Tokyo,
                chrono_tz::Pacific::Apia,
                chrono_tz::Australia::Lord_Howe,
            ]),
        ) {
            let now = Utc.timestamp_opt(seconds, 0).unwrap();
            let today = local_date(now, zone);
            prop_assert!(may_mark_day(today, today));
            prop_assert!(may_mark_day(today - Days::new(3), today));
            prop_assert!(!may_mark_day(today - Days::new(4), today));
            prop_assert!(!may_mark_day(today + Days::new(1), today));
            let past = today - Days::new(3);
            prop_assert_eq!(local_date(occurred_at(past, today, now, zone), zone), past);
        }

        #[test]
        fn r4_complete_means_scheduled_and_reached(
            target in 1_i32..100,
            done in 0_i32..200,
            scheduled: bool,
        ) {
            let state = routine_day(day(1), scheduled, target, done);
            prop_assert_eq!(state.complete, scheduled && done >= target);
            prop_assert_eq!(state.done, done);
        }
    }
}
