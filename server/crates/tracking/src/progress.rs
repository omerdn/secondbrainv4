//! `ProgressProvider` and the tracker's progress (PRD T1, T4, ARCHITECTURE §7.1, §7.2, T1.3.3).
//!
//! The calculation is a pure function. A tracker with a repeating period is measured period by
//! period: the surplus of one day never covers another day's shortfall, as with routines (§7.2).
//! Periods are clipped to the requested range, and each keeps its full target.

use std::future::Future;
use std::pin::Pin;

use chrono::NaiveDate;
use kernel::AppError;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use sqlx::PgPool;
use uuid::Uuid;

use crate::aggregate::{aggregate, window_for};
use crate::model::{Direction, Period, Tracker, TrackerEntry};
use crate::repo;

/// Longest range a progress calculation accepts (20 years), which bounds the work per request.
pub const MAX_RANGE_DAYS: i64 = 7_305;

/// A span of calendar days, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    start: NaiveDate,
    end: NaiveDate,
}

impl DateRange {
    /// `None` if `start` is after `end` or the range is longer than [`MAX_RANGE_DAYS`].
    #[must_use]
    pub fn new(start: NaiveDate, end: NaiveDate) -> Option<Self> {
        let days = (end - start).num_days();
        (0..MAX_RANGE_DAYS)
            .contains(&days)
            .then_some(Self { start, end })
    }

    #[must_use]
    pub const fn start(self) -> NaiveDate {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> NaiveDate {
        self.end
    }
}

/// An item's contribution to a goal (ARCHITECTURE §7.1). `total = 0` means "not started yet" and
/// the goal leaves it out (§7.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub done: f64,
    pub total: f64,
    pub expected_to_date: f64,
}

/// A tracker's progress and whether it has succeeded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackerStatus {
    pub progress: Progress,
    /// Rising: the target of every period is reached. Limit: the range is over and no period
    /// went past the limit; before the end it can still be broken, so it is not a success yet.
    pub successful: bool,
}

/// Result of [`ProgressProvider::progress`].
pub type ProgressFuture<'a> = Pin<Box<dyn Future<Output = Result<Progress, AppError>> + Send + 'a>>;

/// Implemented by every item kind that contributes to a goal (ADR-0006). Goals call this
/// interface and never read another module's tables (ADR-0007).
pub trait ProgressProvider: Send + Sync {
    /// Progress of `item` over `range`, as of the local day `today`.
    fn progress(&self, item: Uuid, range: DateRange, today: NaiveDate) -> ProgressFuture<'_>;
}

/// The stretches of `range` the tracker is measured over.
fn windows(period: Period, range: DateRange) -> Vec<(NaiveDate, NaiveDate)> {
    match period {
        Period::Total => vec![(range.start, range.end)],
        Period::Range { start, end } => {
            let (start, end) = (start.max(range.start), end.min(range.end));
            if start <= end {
                vec![(start, end)]
            } else {
                Vec::new()
            }
        }
        Period::Day | Period::Week | Period::Month => {
            let mut result = Vec::new();
            let mut cursor = Some(range.start);
            while let Some(start) = cursor.filter(|day| *day <= range.end) {
                let end = window_for(period, start)
                    .end
                    .map_or(range.end, |end| end.min(range.end));
                result.push((start, end));
                cursor = end.succ_opt();
            }
            result
        }
    }
}

/// The share of a window that has passed by the end of `today`.
fn elapsed_share((start, end): (NaiveDate, NaiveDate), today: NaiveDate) -> Decimal {
    let days = (end - start).num_days() + 1;
    let passed = ((today - start).num_days() + 1).clamp(0, days);
    Decimal::from(passed) / Decimal::from(days)
}

fn to_f64(value: Decimal) -> f64 {
    value.to_f64().unwrap_or(0.0)
}

/// The tracker's progress over `range` as of `today`, from its entries in any order.
#[must_use]
pub fn tracker_status(
    tracker: &Tracker,
    entries: &[TrackerEntry],
    range: DateRange,
    today: NaiveDate,
) -> TrackerStatus {
    let mut sorted: Vec<&TrackerEntry> = entries.iter().collect();
    sorted.sort_by_key(|entry| entry.local_date);
    let windows = windows(tracker.period, range);
    let value_in = |(start, end): (NaiveDate, NaiveDate)| {
        let from = sorted.partition_point(|entry| entry.local_date < start);
        let to = sorted.partition_point(|entry| entry.local_date <= end);
        aggregate(tracker.aggregation, sorted[from..to].iter().copied()).unwrap_or(Decimal::ZERO)
    };
    match tracker.direction {
        Direction::Increase => {
            let target = tracker.target_value;
            let (mut done, mut expected) = (Decimal::ZERO, Decimal::ZERO);
            for &window in &windows {
                let reached = value_in(window).clamp(Decimal::ZERO, target);
                done = done.saturating_add(reached);
                expected = expected.saturating_add(target * elapsed_share(window, today));
            }
            let total = target.saturating_mul(Decimal::from(windows.len()));
            TrackerStatus {
                progress: Progress {
                    done: to_f64(done),
                    total: to_f64(total),
                    expected_to_date: to_f64(expected.min(total)),
                },
                successful: !windows.is_empty() && done >= total,
            }
        }
        Direction::Limit => {
            // A period counts once it has begun; one still running complies until it is broken.
            let begun: Vec<_> = windows.iter().filter(|(s, _)| *s <= today).collect();
            let within = begun
                .iter()
                .filter(|window| value_in(***window) <= tracker.target_value)
                .count();
            let over = today >= range.end && begun.len() == windows.len();
            TrackerStatus {
                progress: Progress {
                    done: within as f64,
                    total: begun.len() as f64,
                    expected_to_date: begun.len() as f64,
                },
                successful: over && !windows.is_empty() && within == windows.len(),
            }
        }
    }
}

/// [`ProgressProvider`] for trackers, reading the `tracking` module's own tables.
#[derive(Debug, Clone)]
pub struct TrackerProgress {
    pool: PgPool,
}

impl TrackerProgress {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The tracker's status over `range`.
    ///
    /// # Errors
    /// [`AppError::NotFound`] if the item has no tracker, or [`AppError::Internal`].
    pub async fn status(
        &self,
        item: Uuid,
        range: DateRange,
        today: NaiveDate,
    ) -> Result<TrackerStatus, AppError> {
        let tracker = repo::get_tracker(&self.pool, item)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let entries = repo::list_entries(&self.pool, item, Some(range.start), Some(range.end))
            .await
            .map_err(internal)?;
        Ok(tracker_status(&tracker, &entries, range, today))
    }
}

impl ProgressProvider for TrackerProgress {
    fn progress(&self, item: Uuid, range: DateRange, today: NaiveDate) -> ProgressFuture<'_> {
        Box::pin(async move {
            let status = self.status(item, range, today).await?;
            Ok(status.progress)
        })
    }
}

fn internal(err: sqlx::Error) -> AppError {
    AppError::Internal(Box::new(err))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::{DateTime, Days, TimeZone, Utc};
    use proptest::prelude::*;

    use super::*;
    use crate::model::{Aggregation, OwnerKind};

    fn next_day(day: NaiveDate) -> NaiveDate {
        day.succ_opt().unwrap()
    }

    fn date(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    fn range(start: NaiveDate, end: NaiveDate) -> DateRange {
        DateRange::new(start, end).unwrap()
    }

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
    }

    fn tracker(period: Period, direction: Direction, target: i64) -> Tracker {
        Tracker {
            item_id: Uuid::nil(),
            unit: "x".into(),
            target_value: Decimal::from(target),
            period,
            aggregation: Aggregation::Sum,
            direction,
            owner_kind: OwnerKind::Standalone,
            created_at: at(),
            updated_at: at(),
        }
    }

    fn entry(value: i64, on: NaiveDate) -> TrackerEntry {
        TrackerEntry {
            id: Uuid::now_v7(),
            tracker_item_id: Uuid::nil(),
            user_id: Uuid::nil(),
            value: Decimal::from(value),
            occurred_at: at(),
            local_date: on,
            note_block_id: None,
            created_at: at(),
            deleted_at: None,
        }
    }

    #[test]
    fn date_range_rejects_inverted_and_huge_spans() {
        assert!(DateRange::new(date(1, 2), date(1, 1)).is_none());
        assert!(DateRange::new(date(1, 1), date(1, 1)).is_some());
        let far = date(1, 1) + Days::new(u64::try_from(MAX_RANGE_DAYS).unwrap());
        assert!(DateRange::new(date(1, 1), far).is_none());
        assert!(DateRange::new(date(1, 1), far.pred_opt().unwrap()).is_some());
        let r = range(date(1, 1), date(1, 2));
        assert_eq!((r.start(), r.end()), (date(1, 1), date(1, 2)));
    }

    #[test]
    fn windows_are_clipped_to_the_range() {
        // Wed to next Tue: two clipped weeks.
        let r = range(date(11, 25), date(12, 1));
        assert_eq!(
            windows(Period::Week, r),
            [(date(11, 25), date(11, 29)), (date(11, 30), date(12, 1))]
        );
        assert_eq!(
            windows(Period::Month, r),
            [(date(11, 25), date(11, 30)), (date(12, 1), date(12, 1))]
        );
        assert_eq!(windows(Period::Day, r).len(), 7);
        assert_eq!(windows(Period::Total, r), [(date(11, 25), date(12, 1))]);
        let inside = Period::Range {
            start: date(11, 1),
            end: date(11, 27),
        };
        assert_eq!(windows(inside, r), [(date(11, 25), date(11, 27))]);
        let outside = Period::Range {
            start: date(1, 1),
            end: date(1, 2),
        };
        assert!(windows(outside, r).is_empty());
    }

    #[test]
    fn increase_total_caps_at_the_target_and_is_linear() {
        let t = tracker(Period::Total, Direction::Increase, 300);
        let r = range(date(1, 1), date(1, 10));
        let status = tracker_status(&t, &[entry(120, date(1, 2))], r, date(1, 5));
        assert_eq!(
            status.progress,
            Progress {
                done: 120.0,
                total: 300.0,
                expected_to_date: 150.0
            }
        );
        assert!(!status.successful);

        let over = tracker_status(&t, &[entry(500, date(1, 2))], r, date(1, 5));
        assert_eq!(over.progress.done, 300.0);
        assert!(over.successful, "reaching the target is a success at once");
    }

    #[test]
    fn increase_surplus_does_not_cover_another_period() {
        let t = tracker(Period::Day, Direction::Increase, 10);
        let r = range(date(3, 1), date(3, 3));
        let entries = [entry(25, date(3, 1)), entry(4, date(3, 2))];
        let status = tracker_status(&t, &entries, r, date(3, 2));
        assert_eq!(status.progress.done, 14.0);
        assert_eq!(status.progress.total, 30.0);
        assert_eq!(status.progress.expected_to_date, 20.0);
        assert!(!status.successful);

        let all = [
            entry(10, date(3, 1)),
            entry(10, date(3, 2)),
            entry(11, date(3, 3)),
        ];
        assert!(tracker_status(&t, &all, r, date(3, 3)).successful);
    }

    #[test]
    fn increase_ignores_negative_values_in_a_period() {
        let t = tracker(Period::Total, Direction::Increase, 10);
        let r = range(date(1, 1), date(1, 2));
        let status = tracker_status(&t, &[entry(-5, date(1, 1))], r, date(1, 1));
        assert_eq!(status.progress.done, 0.0);
    }

    #[test]
    fn expected_before_during_and_after_the_range() {
        let t = tracker(Period::Total, Direction::Increase, 100);
        let r = range(date(2, 1), date(2, 4));
        let expected = |today| tracker_status(&t, &[], r, today).progress.expected_to_date;
        assert_eq!(expected(date(1, 31)), 0.0);
        assert_eq!(expected(date(2, 1)), 25.0);
        assert_eq!(expected(date(2, 4)), 100.0);
        assert_eq!(expected(date(3, 1)), 100.0);
    }

    #[test]
    fn limit_counts_complying_periods_among_those_begun() {
        let t = tracker(Period::Day, Direction::Limit, 2000);
        let r = range(date(5, 1), date(5, 4));
        let entries = [
            entry(1800, date(5, 1)),
            entry(2000, date(5, 2)), // exactly at the limit complies
            entry(2500, date(5, 3)),
        ];
        let status = tracker_status(&t, &entries, r, date(5, 3));
        assert_eq!(
            status.progress,
            Progress {
                done: 2.0,
                total: 3.0,
                expected_to_date: 3.0
            }
        );
        assert!(!status.successful);
    }

    #[test]
    fn limit_period_in_progress_complies_until_broken() {
        let t = tracker(Period::Week, Direction::Limit, 100);
        let r = range(date(11, 23), date(11, 29));
        let under = tracker_status(&t, &[entry(60, date(11, 24))], r, date(11, 25));
        assert_eq!((under.progress.done, under.progress.total), (1.0, 1.0));
        assert!(!under.successful, "the week can still be broken");
        let broken = tracker_status(
            &t,
            &[entry(60, date(11, 24)), entry(60, date(11, 25))],
            r,
            date(11, 25),
        );
        assert_eq!((broken.progress.done, broken.progress.total), (0.0, 1.0));
    }

    #[test]
    fn limit_succeeds_only_when_the_range_is_over_and_clean() {
        let t = tracker(Period::Day, Direction::Limit, 10);
        let r = range(date(5, 1), date(5, 2));
        let clean = [entry(5, date(5, 1)), entry(10, date(5, 2))];
        assert!(!tracker_status(&t, &clean, r, date(5, 1)).successful);
        assert!(tracker_status(&t, &clean, r, date(5, 2)).successful);
        assert!(tracker_status(&t, &clean, r, date(6, 1)).successful);
        let broken = [entry(11, date(5, 1))];
        assert!(!tracker_status(&t, &broken, r, date(6, 1)).successful);
    }

    #[test]
    fn nothing_begun_or_no_overlap_means_not_started() {
        let limit = tracker(Period::Day, Direction::Limit, 10);
        let r = range(date(5, 1), date(5, 2));
        let early = tracker_status(&limit, &[], r, date(4, 1));
        assert_eq!(early.progress.total, 0.0);
        assert!(!early.successful);

        let outside = Period::Range {
            start: date(1, 1),
            end: date(1, 2),
        };
        for direction in [Direction::Increase, Direction::Limit] {
            let status = tracker_status(&tracker(outside, direction, 10), &[], r, date(6, 1));
            assert_eq!(status.progress.total, 0.0, "{direction:?}");
            assert!(!status.successful, "{direction:?}");
        }
    }

    #[test]
    fn entries_outside_the_range_and_deleted_ones_do_not_count() {
        let t = tracker(Period::Total, Direction::Increase, 100);
        let r = range(date(5, 2), date(5, 3));
        let mut deleted = entry(40, date(5, 2));
        deleted.deleted_at = Some(at());
        let entries = [
            entry(50, date(5, 1)),
            entry(7, date(5, 3)),
            deleted,
            entry(50, date(5, 4)),
        ];
        assert_eq!(
            tracker_status(&t, &entries, r, date(5, 3)).progress.done,
            7.0
        );
    }

    /// Goals depend on the trait alone, so any module can provide progress.
    struct Fixed(Progress);

    impl ProgressProvider for Fixed {
        fn progress(&self, _: Uuid, _: DateRange, _: NaiveDate) -> ProgressFuture<'_> {
            let value = self.0;
            Box::pin(async move { Ok(value) })
        }
    }

    #[tokio::test]
    async fn the_trait_works_as_a_trait_object() {
        let expected = Progress {
            done: 1.0,
            total: 2.0,
            expected_to_date: 1.5,
        };
        let provider: Box<dyn ProgressProvider> = Box::new(Fixed(expected));
        let r = range(date(1, 1), date(1, 2));
        let got = provider.progress(Uuid::nil(), r, date(1, 1)).await;
        assert_eq!(got.ok(), Some(expected));
    }

    fn scenario() -> impl Strategy<Value = (Period, Vec<(i64, u64)>, u64, i64)> {
        (
            prop::sample::select(vec![
                Period::Day,
                Period::Week,
                Period::Month,
                Period::Total,
                Period::Range {
                    start: date(1, 20),
                    end: date(2, 10),
                },
            ]),
            prop::collection::vec((-50i64..200, 0u64..90), 0..40),
            0u64..120,
            1i64..300,
        )
    }

    proptest! {
        #[test]
        fn progress_stays_within_its_bounds((period, rows, today, target) in scenario()) {
            let base = date(1, 1);
            let r = range(base, base + Days::new(89));
            let entries: Vec<_> = rows
                .iter()
                .map(|&(value, day)| entry(value, base + Days::new(day)))
                .collect();
            let today = base + Days::new(today);
            for direction in [Direction::Increase, Direction::Limit] {
                let status = tracker_status(&tracker(period, direction, target), &entries, r, today);
                let p = status.progress;
                prop_assert!(p.done >= 0.0);
                prop_assert!(p.done <= p.total);
                prop_assert!(p.expected_to_date >= 0.0);
                prop_assert!(p.expected_to_date <= p.total);
            }
        }

        #[test]
        fn expected_never_falls_as_days_pass((period, _, today, target) in scenario()) {
            let base = date(1, 1);
            let r = range(base, base + Days::new(89));
            let t = tracker(period, Direction::Increase, target);
            let day = base + Days::new(today);
            let now = tracker_status(&t, &[], r, day).progress.expected_to_date;
            let later = tracker_status(&t, &[], r, next_day(day)).progress.expected_to_date;
            prop_assert!(later >= now);
        }

        #[test]
        fn more_progress_never_hurts_a_rising_tracker(
            (period, rows, today, target) in scenario(),
            extra in 0i64..100,
        ) {
            let base = date(1, 1);
            let r = range(base, base + Days::new(89));
            let t = tracker(period, Direction::Increase, target);
            let positive: Vec<_> = rows
                .iter()
                .map(|&(value, day)| entry(value.abs(), base + Days::new(day)))
                .collect();
            let today = base + Days::new(today);
            let before = tracker_status(&t, &positive, r, today).progress.done;
            let mut more = positive.clone();
            more.push(entry(extra, base));
            let after = tracker_status(&t, &more, r, today).progress.done;
            prop_assert!(after >= before);
        }

        #[test]
        fn order_of_entries_does_not_matter((period, rows, today, target) in scenario()) {
            let base = date(1, 1);
            let r = range(base, base + Days::new(89));
            let entries: Vec<_> = rows
                .iter()
                .map(|&(value, day)| entry(value, base + Days::new(day)))
                .collect();
            let mut reversed = entries.clone();
            reversed.reverse();
            let today = base + Days::new(today);
            for direction in [Direction::Increase, Direction::Limit] {
                let t = tracker(period, direction, target);
                prop_assert_eq!(
                    tracker_status(&t, &entries, r, today),
                    tracker_status(&t, &reversed, r, today)
                );
            }
        }
    }
}
