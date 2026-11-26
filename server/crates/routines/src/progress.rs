//! A routine's progress towards a goal (PRD D5, R4, T4, ARCHITECTURE §7.1, §7.2, T2.2.4). The
//! calculation is a pure function; [`RoutineProgress`] only loads its inputs.

use std::collections::HashMap;

use chrono::NaiveDate;
use kernel::AppError;
use recurrence::Schedule;
use sqlx::PgPool;
use tracking::{DateRange, Progress, ProgressFuture, ProgressProvider};
use uuid::Uuid;

use crate::completions::{done_by_day, internal, schedule_of};
use crate::model::{PlanVersionView, parse_timezone, target_on};
use crate::repo;

/// The days both the goal's range and the routine's dates cover (D5), or `None` if they do not
/// meet.
#[must_use]
pub fn overlap(
    range: DateRange,
    start: NaiveDate,
    end: Option<NaiveDate>,
) -> Option<(NaiveDate, NaiveDate)> {
    let first = range.start().max(start);
    let last = end.map_or(range.end(), |end| end.min(range.end()));
    (first <= last).then_some((first, last))
}

/// The routine's progress over `range` (§7.2). Only occurrences inside the overlap of the goal's
/// range and the routine's dates count (D5). Each occurrence adds its own day's target to
/// `total` and at most that target to `done`: a surplus never covers another day's shortfall,
/// and a partial day counts (R4). `expected_to_date` is the target of the occurrences up to
/// `today`.
#[must_use]
pub fn routine_progress(
    schedule: &Schedule,
    versions: &[PlanVersionView],
    done: &HashMap<NaiveDate, i32>,
    (start, end): (NaiveDate, Option<NaiveDate>),
    range: DateRange,
    today: NaiveDate,
) -> Progress {
    let mut progress = Progress {
        done: 0.0,
        total: 0.0,
        expected_to_date: 0.0,
    };
    let Some((first, last)) = overlap(range, start, end) else {
        return progress;
    };
    for date in first.iter_days().take_while(|date| *date <= last) {
        if !schedule.occurs_on(date) {
            continue;
        }
        let target = target_on(versions, date).max(0);
        let reached = done.get(&date).copied().unwrap_or(0).clamp(0, target);
        progress.total += f64::from(target);
        progress.done += f64::from(reached);
        if date <= today {
            progress.expected_to_date += f64::from(target);
        }
    }
    progress
}

/// [`ProgressProvider`] for routines. Goals call this and never read the routine tables
/// (ADR-0007).
#[derive(Clone)]
pub struct RoutineProgress {
    pool: PgPool,
}

impl RoutineProgress {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn load(
        &self,
        item: Uuid,
        range: DateRange,
        today: NaiveDate,
    ) -> Result<Progress, AppError> {
        let routine = repo::get_routine(&self.pool, item)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let versions = repo::list_versions(&self.pool, item)
            .await
            .map_err(internal)?;
        let schedule = schedule_of(parse_timezone(&routine.timezone)?, &versions)?;
        let entries = match overlap(range, routine.start_date, routine.end_date) {
            Some((first, last)) => {
                tracking::repo::list_entries(&self.pool, item, Some(first), Some(last))
                    .await
                    .map_err(internal)?
            }
            None => Vec::new(),
        };
        Ok(routine_progress(
            &schedule,
            &versions,
            &done_by_day(&entries),
            (routine.start_date, routine.end_date),
            range,
            today,
        ))
    }
}

impl ProgressProvider for RoutineProgress {
    fn progress(&self, item: Uuid, range: DateRange, today: NaiveDate) -> ProgressFuture<'_> {
        Box::pin(self.load(item, range, today))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::Days;
    use proptest::prelude::*;
    use recurrence::PlanVersion;

    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn range(from: u32, to: u32) -> DateRange {
        DateRange::new(day(from), day(to)).unwrap()
    }

    fn version(
        rrule: &str,
        target: i32,
        from: NaiveDate,
        to: Option<NaiveDate>,
    ) -> PlanVersionView {
        PlanVersionView {
            id: Uuid::now_v7(),
            rrule: rrule.into(),
            daily_target: target,
            valid_from: from,
            valid_to: to,
        }
    }

    fn schedule(versions: &[PlanVersionView]) -> Schedule {
        Schedule::new(
            chrono_tz::Europe::Istanbul,
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
    fn d5_only_occurrences_inside_both_ranges_count() {
        // The routine runs 10–20 November, the goal 1–15 November: 10–15 count.
        let versions = [version("FREQ=DAILY", 1, day(10), Some(day(20)))];
        let plan = schedule(&versions);
        let done: HashMap<_, _> = (5..=25).map(|d| (day(d), 1)).collect();
        let p = routine_progress(
            &plan,
            &versions,
            &done,
            (day(10), Some(day(20))),
            range(1, 15),
            day(30),
        );
        assert_eq!((p.done, p.total, p.expected_to_date), (6.0, 6.0, 6.0));
    }

    #[test]
    fn d5_ranges_that_do_not_meet_give_nothing() {
        let versions = [version("FREQ=DAILY", 1, day(20), None)];
        let plan = schedule(&versions);
        let p = routine_progress(
            &plan,
            &versions,
            &HashMap::from([(day(20), 1)]),
            (day(20), None),
            range(1, 15),
            day(30),
        );
        assert_eq!((p.done, p.total, p.expected_to_date), (0.0, 0.0, 0.0));
        assert_eq!(overlap(range(1, 15), day(20), None), None);
        assert_eq!(
            overlap(range(1, 15), day(1), Some(day(1))),
            Some((day(1), day(1)))
        );
    }

    #[test]
    fn a_day_contributes_at_most_its_target() {
        // Target 3: 5 on the 1st does not cover the 2nd's 1; partial days count (R4).
        let versions = [version("FREQ=DAILY", 3, day(1), None)];
        let plan = schedule(&versions);
        let done = HashMap::from([(day(1), 5), (day(2), 1)]);
        let p = routine_progress(&plan, &versions, &done, (day(1), None), range(1, 3), day(2));
        assert_eq!(p.done, 4.0);
        assert_eq!(p.total, 9.0);
        assert_eq!(p.expected_to_date, 6.0);
    }

    #[test]
    fn unscheduled_days_and_plan_versions_shape_the_total() {
        // Mondays (the 2nd and 9th) with target 1, then every day with target 2 from the 14th.
        let versions = [
            version("FREQ=WEEKLY;BYDAY=MO", 1, day(1), Some(day(13))),
            version("FREQ=DAILY", 2, day(14), None),
        ];
        let plan = schedule(&versions);
        let done = HashMap::from([(day(3), 1), (day(9), 1), (day(14), 2)]);
        let p = routine_progress(
            &plan,
            &versions,
            &done,
            (day(1), None),
            range(1, 15),
            day(14),
        );
        // The 3rd is not scheduled, so its entry does not count.
        assert_eq!(p.done, 3.0);
        assert_eq!(p.total, 1.0 + 1.0 + 2.0 + 2.0);
        assert_eq!(p.expected_to_date, 1.0 + 1.0 + 2.0);
    }

    fn rules() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec![
            "FREQ=DAILY",
            "FREQ=DAILY;INTERVAL=3",
            "FREQ=WEEKLY;BYDAY=MO,TH",
            "FREQ=MONTHLY;BYMONTHDAY=1,15,-1",
        ])
    }

    proptest! {
        #[test]
        fn progress_stays_within_its_bounds(
            rule in rules(),
            target in 1_i32..10,
            routine_start in 0_u64..60,
            routine_days in prop::option::of(0_u64..90),
            goal_start in 0_u64..60,
            goal_days in 0_u64..90,
            today_offset in 0_u64..150,
            entries in prop::collection::vec((0_u64..150, 0_i32..30), 0..80),
        ) {
            let base = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
            let start = base + Days::new(routine_start);
            let end = routine_days.map(|n| start + Days::new(n));
            let versions = [version(rule, target, start, end)];
            let plan = schedule(&versions);
            let goal = DateRange::new(base + Days::new(goal_start), base + Days::new(goal_start + goal_days)).unwrap();
            let today = base + Days::new(today_offset);
            let mut done: HashMap<NaiveDate, i32> = HashMap::new();
            for (offset, n) in entries {
                *done.entry(base + Days::new(offset)).or_default() += n;
            }
            let p = routine_progress(&plan, &versions, &done, (start, end), goal, today);
            prop_assert!(p.done >= 0.0);
            prop_assert!(p.done <= p.total);
            prop_assert!(p.expected_to_date <= p.total);
            // The total is the target of every occurrence in the overlap (D5).
            let expected_total = overlap(goal, start, end).map_or(0, |(first, last)| {
                plan.occurrences(first, last).unwrap().len()
            });
            prop_assert_eq!(p.total, f64::from(target) * expected_total as f64);
            // Entries outside the overlap never change the result.
            let inside: HashMap<NaiveDate, i32> = done
                .iter()
                .filter(|(date, _)| overlap(goal, start, end).is_some_and(|(f, l)| (f..=l).contains(*date)))
                .map(|(date, n)| (*date, *n))
                .collect();
            prop_assert_eq!(routine_progress(&plan, &versions, &inside, (start, end), goal, today), p);
        }

        #[test]
        fn more_done_never_lowers_progress(
            target in 1_i32..10,
            entries in prop::collection::vec((1_u32..29, 0_i32..15), 0..40),
            extra_day in 1_u32..29,
            extra in 1_i32..15,
        ) {
            let versions = [version("FREQ=DAILY", target, day(1), None)];
            let plan = schedule(&versions);
            let mut done: HashMap<NaiveDate, i32> = HashMap::new();
            for (d, n) in entries {
                *done.entry(day(d)).or_default() += n;
            }
            let before = routine_progress(&plan, &versions, &done, (day(1), None), range(1, 28), day(28));
            *done.entry(day(extra_day)).or_default() += extra;
            let after = routine_progress(&plan, &versions, &done, (day(1), None), range(1, 28), day(28));
            prop_assert!(after.done >= before.done);
            prop_assert!(after.done - before.done <= f64::from(target));
            prop_assert_eq!(after.total, before.total);
        }
    }
}
