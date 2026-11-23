//! A task's progress (PRD G3, T4, ARCHITECTURE §7.1, §7.2, T2.1.2). The calculation is a pure
//! function; [`TaskProgress`] only loads its inputs.

use chrono::NaiveDate;
use kernel::AppError;
use sqlx::PgPool;
use tracking::{DateRange, Progress, ProgressFuture, ProgressProvider};
use uuid::Uuid;

use crate::repo;

/// Share of the subtasks that are done, from 0 to 1 (G3). A task always has a subtask (G2); an
/// empty count still gives 0 rather than a division by zero.
#[must_use]
pub fn completion(done: u32, total: u32) -> f64 {
    if total == 0 {
        0.0
    } else {
        (f64::from(done) / f64::from(total)).clamp(0.0, 1.0)
    }
}

/// Share of the span `start..=end` that has begun by `today`, from 0 to 1. The day `today` counts
/// as elapsed, so the last day reaches 1.
fn elapsed_share(start: NaiveDate, end: NaiveDate, today: NaiveDate) -> f64 {
    if today < start {
        return 0.0;
    }
    if today >= end {
        return 1.0;
    }
    let days = |from: NaiveDate, to: NaiveDate| {
        f64::from(i32::try_from((to - from).num_days() + 1).unwrap_or(i32::MAX))
    };
    (days(start, today) / days(start, end)).clamp(0.0, 1.0)
}

/// The task's progress over `range` (§7.2): `total` subtasks, `done` of them finished, and
/// `expected_to_date` growing linearly between the task's dates. A missing date falls back to
/// the goal's range.
#[must_use]
pub fn task_progress(
    (done, total): (u32, u32),
    (start, due): (Option<NaiveDate>, Option<NaiveDate>),
    range: DateRange,
    today: NaiveDate,
) -> Progress {
    let done = done.min(total);
    let start = start.unwrap_or_else(|| range.start());
    let end = due.unwrap_or_else(|| range.end());
    // Only one date was set and it lies outside the goal's range.
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (range.start(), range.end())
    };
    Progress {
        done: f64::from(done),
        total: f64::from(total),
        expected_to_date: f64::from(total) * elapsed_share(start, end, today),
    }
}

/// [`ProgressProvider`] for tasks. Goals call this and never read the task tables (ADR-0007).
#[derive(Clone)]
pub struct TaskProgress {
    pool: PgPool,
}

impl TaskProgress {
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
        let internal = |err: sqlx::Error| AppError::Internal(Box::new(err));
        let task = repo::get_task(&self.pool, item)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let counts = repo::subtask_counts(&self.pool, item)
            .await
            .map_err(internal)?;
        Ok(task_progress(
            counts,
            (task.start_date, task.due_date),
            range,
            today,
        ))
    }
}

impl ProgressProvider for TaskProgress {
    fn progress(&self, item: Uuid, range: DateRange, today: NaiveDate) -> ProgressFuture<'_> {
        Box::pin(self.load(item, range, today))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::Days;
    use proptest::prelude::*;

    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn range(start: u32, end: u32) -> DateRange {
        DateRange::new(day(start), day(end)).unwrap()
    }

    #[test]
    fn g3_completion_is_done_over_total() {
        assert!((completion(1, 4) - 0.25).abs() < f64::EPSILON);
        assert!((completion(4, 4) - 1.0).abs() < f64::EPSILON);
        assert!(completion(0, 3).abs() < f64::EPSILON);
        assert!(completion(0, 0).abs() < f64::EPSILON);
        assert!((completion(9, 3) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn expected_grows_linearly_between_the_task_dates() {
        let dates = (Some(day(1)), Some(day(10)));
        let at = |today| task_progress((0, 10), dates, range(1, 30), day(today)).expected_to_date;
        assert!((at(1) - 1.0).abs() < 1e-9);
        assert!((at(5) - 5.0).abs() < 1e-9);
        assert!((at(10) - 10.0).abs() < 1e-9);
        assert!((at(25) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn before_the_start_nothing_is_expected() {
        let progress = task_progress((0, 4), (Some(day(10)), Some(day(20))), range(1, 30), day(9));
        assert!(progress.expected_to_date.abs() < f64::EPSILON);
    }

    #[test]
    fn without_dates_the_goal_range_is_used() {
        let progress = task_progress((1, 10), (None, None), range(1, 10), day(5));
        assert!((progress.expected_to_date - 5.0).abs() < 1e-9);
        assert!((progress.done - 1.0).abs() < f64::EPSILON);
        assert!((progress.total - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_missing_date_comes_from_the_goal_range() {
        let only_due = task_progress((0, 10), (None, Some(day(10))), range(1, 30), day(5));
        assert!((only_due.expected_to_date - 5.0).abs() < 1e-9);
        let only_start = task_progress((0, 10), (Some(day(21)), None), range(1, 30), day(25));
        assert!((only_start.expected_to_date - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_lone_date_outside_the_goal_falls_back_to_the_goal_range() {
        // Starts after the goal ends: the pair would be inverted.
        let progress = task_progress((0, 10), (Some(day(29)), None), range(1, 10), day(5));
        assert!((progress.expected_to_date - 5.0).abs() < 1e-9);
    }

    proptest! {
        #[test]
        fn g3_progress_stays_between_0_and_1(
            done in 0_u32..300,
            total in 0_u32..300,
            start in proptest::option::of(0_u64..400),
            length in proptest::option::of(0_u64..400),
            range_start in 0_u64..400,
            range_length in 0_u64..400,
            today in 0_u64..1_000,
        ) {
            let base = day(1);
            let at = |offset: u64| base.checked_add_days(Days::new(offset)).unwrap();
            let task_start = start.map(at);
            let due = match (start, length) {
                (Some(s), Some(l)) => Some(at(s + l)),
                (None, Some(l)) => Some(at(l)),
                _ => None,
            };
            let goal = DateRange::new(at(range_start), at(range_start + range_length)).unwrap();
            let progress = task_progress((done, total), (task_start, due), goal, at(today));
            prop_assert!(progress.done >= 0.0 && progress.done <= progress.total);
            prop_assert!(progress.expected_to_date >= 0.0);
            prop_assert!(progress.expected_to_date <= progress.total);
            let share = completion(done, total);
            prop_assert!((0.0..=1.0).contains(&share));
        }

        #[test]
        fn expected_never_falls_as_days_pass(
            start in 0_u64..200,
            length in 0_u64..200,
            today in 0_u64..500,
        ) {
            let at = |offset: u64| day(1).checked_add_days(Days::new(offset)).unwrap();
            let dates = (Some(at(start)), Some(at(start + length)));
            let goal = DateRange::new(at(0), at(500)).unwrap();
            let now = task_progress((0, 7), dates, goal, at(today));
            let next = task_progress((0, 7), dates, goal, at(today + 1));
            prop_assert!(next.expected_to_date >= now.expected_to_date);
        }
    }
}
