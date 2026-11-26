//! Routine streaks (PRD R9, ARCHITECTURE §7.4, T2.2.3). The streak is computed on demand from the
//! plan and the entries; nothing is stored. The rules are pure functions; the clock comes in from
//! outside.

use std::collections::HashMap;

use access::Perms;
use chrono::{DateTime, NaiveDate, Utc};
use kernel::AppError;
use recurrence::Schedule;
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::completions::{done_by_day, internal, schedule_of};
use crate::model::{PlanVersionView, parse_timezone, target_on};
use crate::repo;
use crate::service::RoutinesService;

/// `GET /routines/{id}/streak` response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RoutineStreak {
    /// Consecutive fully completed occurrences up to today (R9).
    pub current: u32,
    /// Today has an occurrence that is not complete yet; it does not break the streak until the
    /// day ends.
    pub today_pending: bool,
}

/// One day as the streak sees it: whether it is scheduled and whether it is complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreakDay {
    pub date: NaiveDate,
    pub scheduled: bool,
    pub complete: bool,
}

/// The streak on `today` over `days`, in any order (R9): scanning back from today, every complete
/// occurrence adds one and the first incomplete one stops the scan. Days without an occurrence are
/// skipped, a partial day is incomplete, and today's occurrence is skipped while it is incomplete.
/// Days after `today` are ignored.
#[must_use]
pub fn streak(days: &[StreakDay], today: NaiveDate) -> RoutineStreak {
    let mut sorted: Vec<&StreakDay> = days
        .iter()
        .filter(|d| d.scheduled && d.date <= today)
        .collect();
    sorted.sort_by_key(|d| std::cmp::Reverse(d.date));
    let mut current = 0_u32;
    let mut today_pending = false;
    for day in sorted {
        if day.complete {
            current = current.saturating_add(1);
        } else if day.date == today {
            today_pending = true;
        } else {
            break;
        }
    }
    RoutineStreak {
        current,
        today_pending,
    }
}

/// The streak of a routine at `now`: the days from the first plan version up to today, in the
/// routine's time zone, with what was done on each (R3, R9). Partial days count as done but never
/// as complete (R4).
#[must_use]
pub fn routine_streak(
    schedule: &Schedule,
    versions: &[PlanVersionView],
    done: &HashMap<NaiveDate, i32>,
    now: DateTime<Utc>,
) -> RoutineStreak {
    let today = schedule.today(now);
    let Some(first) = schedule.versions().iter().map(|v| v.valid_from).min() else {
        return RoutineStreak {
            current: 0,
            today_pending: false,
        };
    };
    let days: Vec<StreakDay> = first
        .iter_days()
        .take_while(|date| *date <= today)
        .map(|date| {
            let scheduled = schedule.occurs_on(date);
            StreakDay {
                date,
                scheduled,
                complete: scheduled
                    && done.get(&date).copied().unwrap_or(0) >= target_on(versions, date),
            }
        })
        .collect();
    streak(&days, today)
}

impl RoutinesService {
    /// The routine's current streak (VIEW).
    ///
    /// # Errors
    /// [`AppError::NotFound`] or [`AppError::Internal`].
    pub async fn streak(&self, actor: Uuid, id: Uuid) -> Result<RoutineStreak, AppError> {
        self.permissions
            .require(&self.pool, actor, id, Perms::VIEW)
            .await?;
        let row = repo::get_routine(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let timezone = parse_timezone(&row.timezone)?;
        let now = self.clock.now();
        let versions = repo::list_versions(&self.pool, id)
            .await
            .map_err(internal)?;
        let schedule = schedule_of(timezone, &versions)?;
        let from = versions.iter().map(|v| v.valid_from).min();
        let entries = tracking::repo::list_entries(&self.pool, id, from, Some(schedule.today(now)))
            .await
            .map_err(internal)?;
        Ok(routine_streak(
            &schedule,
            &versions,
            &done_by_day(&entries),
            now,
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::{Days, Duration, TimeZone};
    use chrono_tz::Tz;
    use proptest::prelude::*;
    use recurrence::{PlanVersion, local_date};

    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn scheduled(d: u32, complete: bool) -> StreakDay {
        StreakDay {
            date: day(d),
            scheduled: true,
            complete,
        }
    }

    fn off(d: u32) -> StreakDay {
        StreakDay {
            date: day(d),
            scheduled: false,
            complete: false,
        }
    }

    fn view(rrule: &str, target: i32, from: NaiveDate) -> PlanVersionView {
        PlanVersionView {
            id: Uuid::now_v7(),
            rrule: rrule.into(),
            daily_target: target,
            valid_from: from,
            valid_to: None,
        }
    }

    fn schedule(zone: Tz, versions: &[PlanVersionView]) -> Schedule {
        Schedule::new(
            zone,
            versions
                .iter()
                .map(|v| PlanVersion {
                    rules: v.rrule.parse().unwrap(),
                    valid_from: v.valid_from,
                    valid_to: v.valid_to,
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn r9_counts_consecutive_complete_occurrences() {
        let days = [
            scheduled(1, true),
            scheduled(2, false),
            scheduled(3, true),
            scheduled(4, true),
            scheduled(5, true),
        ];
        assert_eq!(streak(&days, day(5)).current, 3);
        // On the 2nd, its miss is still pending.
        assert_eq!(streak(&days, day(2)).current, 1);
        assert_eq!(streak(&days, day(3)).current, 1);
        assert_eq!(streak(&[], day(5)).current, 0);
    }

    #[test]
    fn r9_unscheduled_days_do_not_break_the_streak() {
        let days = [
            scheduled(1, true),
            off(2),
            off(3),
            scheduled(4, true),
            off(5),
        ];
        assert_eq!(streak(&days, day(5)).current, 2);
    }

    #[test]
    fn r9_a_partial_day_does_not_continue_the_streak() {
        let versions = [view("FREQ=DAILY", 3, day(1))];
        let plan = schedule(chrono_tz::UTC, &versions);
        let done = HashMap::from([(day(1), 3), (day(2), 2), (day(3), 3), (day(4), 5)]);
        let now = Utc.with_ymd_and_hms(2026, 11, 4, 20, 0, 0).unwrap();
        assert_eq!(routine_streak(&plan, &versions, &done, now).current, 2);
    }

    #[test]
    fn r9_today_does_not_break_the_streak_until_it_ends() {
        let days = [scheduled(3, true), scheduled(4, true), scheduled(5, false)];
        assert_eq!(
            streak(&days, day(5)),
            RoutineStreak {
                current: 2,
                today_pending: true
            }
        );
        // The next day, the miss counts.
        let next = [
            scheduled(3, true),
            scheduled(4, true),
            scheduled(5, false),
            off(6),
        ];
        assert_eq!(
            streak(&next, day(6)),
            RoutineStreak {
                current: 0,
                today_pending: false
            }
        );
        let done_today = [scheduled(4, true), scheduled(5, true)];
        assert_eq!(
            streak(&done_today, day(5)),
            RoutineStreak {
                current: 2,
                today_pending: false
            }
        );
    }

    #[test]
    fn r9_today_ends_at_local_midnight() {
        let zone = chrono_tz::Europe::Istanbul;
        let versions = [view("FREQ=DAILY", 1, day(1))];
        let plan = schedule(zone, &versions);
        let done = HashMap::from([(day(1), 1), (day(2), 1)]);
        // 23:59 on 3 November in Istanbul (UTC+3): the 3rd is still today.
        let before = Utc.with_ymd_and_hms(2026, 11, 3, 20, 59, 0).unwrap();
        assert_eq!(routine_streak(&plan, &versions, &done, before).current, 2);
        // 00:00 on the 4th: the 3rd was missed.
        let after = before + Duration::minutes(1);
        assert_eq!(routine_streak(&plan, &versions, &done, after).current, 0);
    }

    #[test]
    fn r9_follows_plan_versions_and_their_targets() {
        // Weekly on Mondays until the 15th, then every day with a target of 2 (R11).
        let mut monday = view("FREQ=WEEKLY;BYDAY=MO", 1, day(2));
        monday.valid_to = Some(day(15));
        let versions = [monday, view("FREQ=DAILY", 2, day(16))];
        let plan = schedule(chrono_tz::UTC, &versions);
        let done = HashMap::from([(day(2), 1), (day(9), 1), (day(16), 2), (day(17), 1)]);
        let on = |d: u32| Utc.with_ymd_and_hms(2026, 11, d, 12, 0, 0).unwrap();
        assert_eq!(routine_streak(&plan, &versions, &done, on(16)).current, 3);
        // The 17th is partial: pending while it is today, a break once it has passed.
        let pending = routine_streak(&plan, &versions, &done, on(17));
        assert_eq!(pending.current, 3);
        assert!(pending.today_pending);
        assert_eq!(routine_streak(&plan, &versions, &done, on(18)).current, 0);
    }

    #[test]
    fn r9_before_the_routine_starts_there_is_no_streak() {
        let versions = [view("FREQ=DAILY", 1, day(10))];
        let plan = schedule(chrono_tz::UTC, &versions);
        let now = Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap();
        assert_eq!(
            routine_streak(&plan, &versions, &HashMap::new(), now),
            RoutineStreak {
                current: 0,
                today_pending: false
            }
        );
    }

    fn zones() -> impl Strategy<Value = Tz> {
        prop::sample::select(vec![
            chrono_tz::UTC,
            chrono_tz::Europe::Istanbul,
            chrono_tz::Europe::Berlin,
            chrono_tz::America::New_York,
            chrono_tz::Australia::Lord_Howe,
            chrono_tz::Pacific::Apia,
            chrono_tz::Asia::Tokyo,
        ])
    }

    proptest! {
        #[test]
        fn r9_adding_unscheduled_days_never_changes_the_streak(
            pattern in prop::collection::vec(any::<bool>(), 1..40),
            gaps in prop::collection::vec(0_u64..3, 40),
        ) {
            let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
            let mut dense = Vec::new();
            let mut sparse = Vec::new();
            let mut date = start;
            for (i, complete) in pattern.iter().enumerate() {
                dense.push(StreakDay { date: start + Days::new(i as u64), scheduled: true, complete: *complete });
                sparse.push(StreakDay { date, scheduled: true, complete: *complete });
                for _ in 0..gaps[i] {
                    date = date + Days::new(1);
                    sparse.push(StreakDay { date, scheduled: false, complete: false });
                }
                date = date + Days::new(1);
            }
            let dense_today = dense.last().unwrap().date;
            let sparse_today = sparse.iter().rev().find(|d| d.scheduled).unwrap().date;
            prop_assert_eq!(streak(&dense, dense_today), streak(&sparse, sparse_today));
        }

        #[test]
        fn r9_streak_is_the_trailing_run_of_complete_occurrences(
            pattern in prop::collection::vec(any::<bool>(), 1..60),
        ) {
            let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
            let days: Vec<StreakDay> = pattern.iter().enumerate().map(|(i, c)| StreakDay {
                date: start + Days::new(i as u64), scheduled: true, complete: *c,
            }).collect();
            let today = days.last().unwrap().date;
            let result = streak(&days, today);
            let last_pending = !pattern[pattern.len() - 1];
            let considered = if last_pending { &pattern[..pattern.len() - 1] } else { &pattern[..] };
            let expected = considered.iter().rev().take_while(|c| **c).count() as u32;
            prop_assert_eq!(result.current, expected);
            prop_assert_eq!(result.today_pending, last_pending);
            // Order of the input does not matter.
            let mut reversed = days.clone();
            reversed.reverse();
            prop_assert_eq!(streak(&reversed, today), result);
        }

        #[test]
        fn r9_daily_entries_build_a_streak_across_clock_changes(
            zone in zones(),
            // Covers the March/April and October/November transitions of 2026 in every zone above.
            start_offset in 0_i64..300,
            length in 1_u64..60,
            minute in 0_i64..1440,
        ) {
            let first = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap() + Days::new(start_offset as u64);
            let last = first + Days::new(length - 1);
            let versions = [view("FREQ=DAILY", 1, first)];
            let plan = schedule(zone, &versions);
            // Every day is done once, at some local moment inside that day.
            let done: HashMap<NaiveDate, i32> = first
                .iter_days()
                .take_while(|d| *d <= last)
                .map(|d| {
                    let at = tracking::start_of_day(d, zone) + Duration::minutes(minute);
                    let local = local_date(at.min(tracking::start_of_day(d + Days::new(1), zone) - Duration::seconds(1)), zone);
                    (local, 1)
                })
                .collect();
            prop_assert_eq!(done.len() as u64, length);
            // Any moment of the last day: all days count.
            let end_of_last = tracking::start_of_day(last + Days::new(1), zone) - Duration::seconds(1);
            prop_assert_eq!(routine_streak(&plan, &versions, &done, end_of_last).current as u64, length);
            // Any moment of the next day: the streak holds while the day is pending.
            let next = tracking::start_of_day(last + Days::new(1), zone);
            let pending = routine_streak(&plan, &versions, &done, next);
            prop_assert_eq!(pending.current as u64, length);
            prop_assert!(pending.today_pending);
            // The day after that, the missed day breaks it.
            let later = tracking::start_of_day(last + Days::new(2), zone);
            prop_assert_eq!(routine_streak(&plan, &versions, &done, later).current, 0);
        }
    }
}
