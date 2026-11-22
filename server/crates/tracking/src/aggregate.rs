//! Aggregation methods and period calculations (PRD T1, ARCHITECTURE §7.2, T1.3.2). Pure
//! functions: the current day and the time zone come from the caller.
//!
//! An entry belongs to a period by its `local_date`, the user's calendar day when it happened, so
//! a daylight saving change never moves an entry into another period. Weeks start on Monday.

use chrono::{DateTime, Datelike, Days, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use rust_decimal::Decimal;

use crate::model::{Aggregation, Period, TrackerEntry};

/// A span of calendar days; both ends are included and an absent end is unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateWindow {
    pub start: Option<NaiveDate>,
    pub end: Option<NaiveDate>,
}

impl DateWindow {
    /// Whether `date` falls inside the window.
    #[must_use]
    pub fn contains(self, date: NaiveDate) -> bool {
        self.start.is_none_or(|start| start <= date) && self.end.is_none_or(|end| date <= end)
    }

    /// The window as instants in `timezone`: its first moment, and the first moment after it.
    /// Days are not always 24 hours long, so these cannot be derived by adding hours.
    #[must_use]
    pub fn utc_bounds(self, timezone: Tz) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
        let start = self.start.map(|date| start_of_day(date, timezone));
        let end = self.end.map(|date| {
            let next = date.succ_opt().unwrap_or(date);
            start_of_day(next, timezone)
        });
        (start, end)
    }
}

/// The period containing `date`. For [`Period::Range`] this is the range itself, whether or not
/// `date` falls inside it.
#[must_use]
pub fn window_for(period: Period, date: NaiveDate) -> DateWindow {
    let (start, end) = match period {
        Period::Day => (Some(date), Some(date)),
        Period::Week => {
            let since_monday = u64::from(date.weekday().num_days_from_monday());
            let monday = date
                .checked_sub_days(Days::new(since_monday))
                .unwrap_or(NaiveDate::MIN);
            let sunday = monday
                .checked_add_days(Days::new(6))
                .unwrap_or(NaiveDate::MAX);
            (Some(monday), Some(sunday))
        }
        Period::Month => (first_of_month(date), last_of_month(date)),
        Period::Total => (None, None),
        Period::Range { start, end } => (Some(start), Some(end)),
    };
    DateWindow { start, end }
}

fn first_of_month(date: NaiveDate) -> Option<NaiveDate> {
    date.with_day(1)
}

fn last_of_month(date: NaiveDate) -> Option<NaiveDate> {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|first| first.pred_opt())
        .or(Some(NaiveDate::MAX))
}

/// The first moment of `date` in `timezone`. Midnight can occur twice (the earlier one counts) or
/// not at all, when clocks jump over it: then the day starts at the first time that exists. If
/// the whole day was skipped, this is the start of the next one.
#[must_use]
pub fn start_of_day(date: NaiveDate, timezone: Tz) -> DateTime<Utc> {
    let midnight = date.and_time(NaiveTime::MIN);
    (0..=(24 * 60))
        .find_map(|minutes| {
            let local = midnight.checked_add_signed(Duration::minutes(minutes))?;
            timezone.from_local_datetime(&local).earliest()
        })
        .map_or_else(
            || Utc.from_utc_datetime(&midnight),
            |start| start.with_timezone(&Utc),
        )
}

/// Combines live entries with `aggregation`; deleted entries never count. With no entries, a sum
/// or count is zero, while the last and highest values do not exist.
pub fn aggregate<'a>(
    aggregation: Aggregation,
    entries: impl IntoIterator<Item = &'a TrackerEntry>,
) -> Option<Decimal> {
    let live = entries
        .into_iter()
        .filter(|entry| entry.deleted_at.is_none());
    match aggregation {
        Aggregation::Sum => {
            Some(live.fold(Decimal::ZERO, |sum, entry| sum.saturating_add(entry.value)))
        }
        Aggregation::Count => Some(Decimal::from(live.count())),
        Aggregation::Last => live
            .max_by_key(|entry| (entry.occurred_at, entry.id))
            .map(|entry| entry.value),
        Aggregation::Max => live.map(|entry| entry.value).max(),
    }
}

/// The value of the period containing `date`.
pub fn aggregate_period<'a>(
    aggregation: Aggregation,
    period: Period,
    date: NaiveDate,
    entries: impl IntoIterator<Item = &'a TrackerEntry>,
) -> Option<Decimal> {
    let window = window_for(period, date);
    aggregate(
        aggregation,
        entries
            .into_iter()
            .filter(|entry| window.contains(entry.local_date)),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::Weekday;
    use chrono_tz::{America, Asia, Europe, Pacific};
    use proptest::prelude::*;
    use recurrence::local_date;
    use uuid::Uuid;

    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    fn entry(value: Decimal, on: NaiveDate, minute: i64) -> TrackerEntry {
        let at = on.and_time(NaiveTime::MIN).and_utc() + Duration::minutes(minute);
        TrackerEntry {
            id: Uuid::now_v7(),
            tracker_item_id: Uuid::nil(),
            user_id: Uuid::nil(),
            value,
            occurred_at: at,
            local_date: on,
            note_block_id: None,
            note: None,
            created_at: at,
            deleted_at: None,
        }
    }

    fn hours(window: DateWindow, timezone: Tz) -> i64 {
        let (start, end) = window.utc_bounds(timezone);
        (end.unwrap() - start.unwrap()).num_hours()
    }

    #[test]
    fn day_week_and_month_windows() {
        let wednesday = date(2026, 11, 25);
        assert_eq!(
            window_for(Period::Day, wednesday),
            DateWindow {
                start: Some(wednesday),
                end: Some(wednesday)
            }
        );
        assert_eq!(
            window_for(Period::Week, wednesday),
            DateWindow {
                start: Some(date(2026, 11, 23)),
                end: Some(date(2026, 11, 29))
            }
        );
        // A week across a new year, from its Sunday.
        assert_eq!(
            window_for(Period::Week, date(2027, 1, 3)),
            DateWindow {
                start: Some(date(2026, 12, 28)),
                end: Some(date(2027, 1, 3))
            }
        );
        for (day, last) in [
            (date(2026, 2, 10), date(2026, 2, 28)),
            (date(2028, 2, 29), date(2028, 2, 29)),
            (date(2026, 12, 31), date(2026, 12, 31)),
            (date(2026, 4, 1), date(2026, 4, 30)),
        ] {
            let window = window_for(Period::Month, day);
            assert_eq!(window.start, day.with_day(1));
            assert_eq!(window.end, Some(last));
        }
    }

    #[test]
    fn total_is_unbounded_and_range_is_fixed() {
        let total = window_for(Period::Total, date(2026, 1, 1));
        assert_eq!(
            total,
            DateWindow {
                start: None,
                end: None
            }
        );
        assert!(total.contains(NaiveDate::MIN) && total.contains(NaiveDate::MAX));
        assert_eq!(total.utc_bounds(Europe::Istanbul), (None, None));

        let (start, end) = (date(2026, 3, 10), date(2026, 3, 20));
        let range = window_for(Period::Range { start, end }, date(2026, 12, 1));
        assert_eq!((range.start, range.end), (Some(start), Some(end)));
        assert!(range.contains(start) && range.contains(end));
        assert!(!range.contains(date(2026, 3, 9)) && !range.contains(date(2026, 3, 21)));
    }

    #[test]
    fn windows_at_the_edges_of_the_calendar_do_not_panic() {
        for day in [NaiveDate::MIN, NaiveDate::MAX] {
            for period in [Period::Day, Period::Week, Period::Month, Period::Total] {
                assert!(window_for(period, day).contains(day), "{period:?}");
            }
        }
    }

    #[test]
    fn days_around_daylight_saving_are_23_and_25_hours() {
        let spring = window_for(Period::Day, date(2026, 3, 8));
        let autumn = window_for(Period::Day, date(2026, 11, 1));
        let plain = window_for(Period::Day, date(2026, 6, 1));
        assert_eq!(hours(spring, America::New_York), 23);
        assert_eq!(hours(autumn, America::New_York), 25);
        assert_eq!(hours(plain, America::New_York), 24);
        // Istanbul has no daylight saving.
        assert_eq!(hours(spring, Europe::Istanbul), 24);
        let week = window_for(Period::Week, date(2026, 3, 8));
        assert_eq!(hours(week, America::New_York), 7 * 24 - 1);
        let march = window_for(Period::Month, date(2026, 3, 15));
        assert_eq!(hours(march, America::New_York), 31 * 24 - 1);
        assert_eq!(
            march.utc_bounds(America::New_York),
            (Some(utc(2026, 3, 1, 5)), Some(utc(2026, 4, 1, 4)))
        );
    }

    #[test]
    fn midnight_that_is_skipped_or_repeated() {
        // Havana jumps from 00:00 to 01:00, so the day starts at 01:00 (UTC-4).
        assert_eq!(
            start_of_day(date(2026, 3, 8), America::Havana),
            utc(2026, 3, 8, 5)
        );
        // In autumn 00:00 happens twice; the earlier one (UTC-4) starts the day.
        assert_eq!(
            start_of_day(date(2026, 11, 1), America::Havana),
            utc(2026, 11, 1, 4)
        );
        // Samoa skipped all of 2011-12-30: it "starts" when 12-31 does.
        assert_eq!(
            start_of_day(date(2011, 12, 30), Pacific::Apia),
            start_of_day(date(2011, 12, 31), Pacific::Apia)
        );
    }

    #[test]
    fn an_entry_late_at_night_counts_on_its_local_day() {
        let instant = utc(2026, 2, 1, 3); // 22:00 on Jan 31 in New York
        let day = local_date(instant, America::New_York);
        assert_eq!(day, date(2026, 1, 31));
        let late = entry(Decimal::TEN, day, 0);
        let january = aggregate_period(Aggregation::Sum, Period::Month, date(2026, 1, 15), [&late]);
        let february =
            aggregate_period(Aggregation::Sum, Period::Month, date(2026, 2, 15), [&late]);
        assert_eq!(january, Some(Decimal::TEN));
        assert_eq!(february, Some(Decimal::ZERO));
    }

    #[test]
    fn empty_periods() {
        let none: [&TrackerEntry; 0] = [];
        assert_eq!(aggregate(Aggregation::Sum, none), Some(Decimal::ZERO));
        assert_eq!(aggregate(Aggregation::Count, none), Some(Decimal::ZERO));
        assert_eq!(aggregate(Aggregation::Last, none), None);
        assert_eq!(aggregate(Aggregation::Max, none), None);
    }

    #[test]
    fn each_method_on_known_values() {
        let day = date(2026, 5, 1);
        let values = [
            entry(Decimal::new(25, 1), day, 10),
            entry(Decimal::new(70, 1), day, 5),
            entry(Decimal::new(-1, 0), day, 20),
        ];
        assert_eq!(
            aggregate(Aggregation::Sum, &values),
            Some(Decimal::new(85, 1))
        );
        assert_eq!(
            aggregate(Aggregation::Count, &values),
            Some(Decimal::from(3))
        );
        assert_eq!(
            aggregate(Aggregation::Last, &values),
            Some(Decimal::from(-1))
        );
        assert_eq!(aggregate(Aggregation::Max, &values), Some(Decimal::from(7)));
    }

    #[test]
    fn last_breaks_a_tie_by_id_and_sum_saturates() {
        let day = date(2026, 5, 1);
        let mut first = entry(Decimal::ONE, day, 0);
        let mut second = entry(Decimal::TWO, day, 0);
        first.id = Uuid::from_u128(1);
        second.id = Uuid::from_u128(2);
        assert_eq!(
            aggregate(Aggregation::Last, [&second, &first]),
            Some(Decimal::TWO)
        );
        let huge = [entry(Decimal::MAX, day, 0), entry(Decimal::MAX, day, 1)];
        assert_eq!(aggregate(Aggregation::Sum, &huge), Some(Decimal::MAX));
    }

    #[test]
    fn deleted_entries_never_count() {
        let day = date(2026, 5, 1);
        let live = entry(Decimal::ONE, day, 0);
        let mut deleted = entry(Decimal::TEN, day, 5);
        deleted.deleted_at = Some(deleted.occurred_at);
        for (aggregation, expected) in [
            (Aggregation::Sum, Decimal::ONE),
            (Aggregation::Count, Decimal::ONE),
            (Aggregation::Last, Decimal::ONE),
            (Aggregation::Max, Decimal::ONE),
        ] {
            assert_eq!(aggregate(aggregation, [&live, &deleted]), Some(expected));
        }
    }

    fn any_date() -> impl Strategy<Value = NaiveDate> {
        (0u64..14_000).prop_map(|days| date(2000, 1, 1) + Days::new(days))
    }

    fn any_zone() -> impl Strategy<Value = Tz> {
        prop::sample::select(vec![
            Europe::Istanbul,
            Europe::London,
            America::New_York,
            America::Havana,
            America::Sao_Paulo,
            Asia::Beirut,
            Asia::Kolkata,
            Asia::Tokyo,
        ])
    }

    fn any_entries() -> impl Strategy<Value = Vec<TrackerEntry>> {
        prop::collection::vec((-1_000_000i64..1_000_000, 0i64..100_000), 0..30).prop_map(|rows| {
            rows.into_iter()
                .map(|(value, minute)| entry(Decimal::new(value, 2), date(2026, 5, 1), minute))
                .collect()
        })
    }

    proptest! {
        #[test]
        fn every_window_contains_its_date(day in any_date()) {
            for period in [Period::Day, Period::Week, Period::Month, Period::Total] {
                prop_assert!(window_for(period, day).contains(day));
            }
        }

        #[test]
        fn weeks_are_seven_days_from_monday(day in any_date()) {
            let week = window_for(Period::Week, day);
            let (start, end) = (week.start.unwrap(), week.end.unwrap());
            prop_assert_eq!(start.weekday(), Weekday::Mon);
            prop_assert_eq!(end.weekday(), Weekday::Sun);
            prop_assert_eq!((end - start).num_days(), 6);
        }

        #[test]
        fn months_cover_exactly_their_days(day in any_date()) {
            let month = window_for(Period::Month, day);
            let (start, end) = (month.start.unwrap(), month.end.unwrap());
            prop_assert_eq!(start.day(), 1);
            prop_assert_eq!((start.year(), start.month()), (day.year(), day.month()));
            prop_assert_eq!((end.year(), end.month()), (day.year(), day.month()));
            prop_assert_eq!(end.succ_opt().unwrap().day(), 1);
        }

        #[test]
        fn consecutive_periods_never_overlap_or_leave_gaps(day in any_date()) {
            for period in [Period::Day, Period::Week, Period::Month] {
                let window = window_for(period, day);
                let next = window.end.unwrap().succ_opt().unwrap();
                prop_assert_eq!(window_for(period, next).start, Some(next));
                prop_assert!(!window.contains(next));
            }
        }

        #[test]
        fn utc_bounds_match_the_local_day(day in any_date(), zone in any_zone()) {
            let (start, end) = window_for(Period::Day, day).utc_bounds(zone);
            let (start, end) = (start.unwrap(), end.unwrap());
            prop_assert!(start < end);
            prop_assert_eq!(local_date(start, zone), day);
            prop_assert_eq!(local_date(end - Duration::seconds(1), zone), day);
            prop_assert_eq!(local_date(end, zone), day.succ_opt().unwrap());
            prop_assert!(local_date(start - Duration::seconds(1), zone) < day);
        }

        #[test]
        fn adjacent_days_share_their_boundary(day in any_date(), zone in any_zone()) {
            let (_, end) = window_for(Period::Day, day).utc_bounds(zone);
            let next = day.succ_opt().unwrap();
            let (start, _) = window_for(Period::Day, next).utc_bounds(zone);
            prop_assert_eq!(end, start);
        }

        #[test]
        fn methods_match_their_definitions(entries in any_entries()) {
            let values: Vec<Decimal> = entries.iter().map(|e| e.value).collect();
            prop_assert_eq!(
                aggregate(Aggregation::Sum, &entries),
                Some(values.iter().copied().sum::<Decimal>())
            );
            prop_assert_eq!(
                aggregate(Aggregation::Count, &entries),
                Some(Decimal::from(values.len()))
            );
            prop_assert_eq!(
                aggregate(Aggregation::Max, &entries),
                values.iter().copied().max()
            );
            let latest = entries.iter().max_by_key(|e| (e.occurred_at, e.id)).map(|e| e.value);
            prop_assert_eq!(aggregate(Aggregation::Last, &entries), latest);
        }

        #[test]
        fn order_of_entries_does_not_matter(entries in any_entries()) {
            let mut reversed = entries.clone();
            reversed.reverse();
            for &aggregation in Aggregation::ALL {
                prop_assert_eq!(
                    aggregate(aggregation, &entries),
                    aggregate(aggregation, &reversed)
                );
            }
        }

        #[test]
        fn a_period_sees_only_its_own_entries(
            days in prop::collection::vec(0u64..120, 0..40),
            on in 0u64..120,
        ) {
            let base = date(2026, 1, 1);
            let entries: Vec<TrackerEntry> = days
                .iter()
                .map(|&d| entry(Decimal::ONE, base + Days::new(d), 0))
                .collect();
            let target = base + Days::new(on);
            for period in [Period::Day, Period::Week, Period::Month, Period::Total] {
                let window = window_for(period, target);
                let expected = entries.iter().filter(|e| window.contains(e.local_date)).count();
                prop_assert_eq!(
                    aggregate_period(Aggregation::Count, period, target, &entries),
                    Some(Decimal::from(expected))
                );
            }
        }
    }
}
