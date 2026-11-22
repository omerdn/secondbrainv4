// Test module: panics are the failure mechanism.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{DateTime, Days, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use proptest::prelude::*;

use super::*;
use crate::rule::RuleSet;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn utc(y: i32, m: u32, day: u32, h: u32, min: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, day, h, min, 0).unwrap()
}

fn rules(s: &str) -> RuleSet {
    s.parse().unwrap()
}

fn version(s: &str, from: NaiveDate, to: Option<NaiveDate>) -> PlanVersion {
    PlanVersion {
        rules: rules(s),
        valid_from: from,
        valid_to: to,
    }
}

const IST: Tz = chrono_tz::Europe::Istanbul;

// --- construction ---------------------------------------------------------------------------

#[test]
fn versions_are_sorted_and_gaps_allowed() {
    let s = Schedule::new(
        IST,
        vec![
            version("FREQ=DAILY", d(2026, 3, 1), None),
            version("FREQ=DAILY", d(2026, 1, 1), Some(d(2026, 1, 31))),
        ],
    )
    .unwrap();
    assert_eq!(s.versions()[0].valid_from, d(2026, 1, 1));
    assert_eq!(s.timezone(), IST);
    // February is a gap (paused routine): no occurrences.
    let feb = s.occurrences(d(2026, 1, 31), d(2026, 3, 1)).unwrap();
    assert_eq!(feb, [d(2026, 1, 31), d(2026, 3, 1)]);
}

#[test]
fn invalid_versions_are_rejected() {
    assert_eq!(
        Schedule::new(
            IST,
            vec![version("FREQ=DAILY", d(2026, 2, 1), Some(d(2026, 1, 31)))]
        ),
        Err(ScheduleError::InvertedVersion {
            valid_from: d(2026, 2, 1),
            valid_to: d(2026, 1, 31)
        })
    );
    // Open-ended version followed by another.
    assert_eq!(
        Schedule::new(
            IST,
            vec![
                version("FREQ=DAILY", d(2026, 1, 1), None),
                version("FREQ=DAILY", d(2026, 2, 1), None)
            ]
        ),
        Err(ScheduleError::Overlap(d(2026, 1, 1), d(2026, 2, 1)))
    );
    // Closed version ending on the next one's first day.
    let err = Schedule::new(
        IST,
        vec![
            version("FREQ=DAILY", d(2026, 1, 1), Some(d(2026, 2, 1))),
            version("FREQ=DAILY", d(2026, 2, 1), None),
        ],
    )
    .unwrap_err();
    assert_eq!(err, ScheduleError::Overlap(d(2026, 1, 1), d(2026, 2, 1)));
    assert!(err.to_string().contains("overlap"));
}

// --- R2: start and end dates ----------------------------------------------------------------

#[test]
fn start_and_end_dates_bound_occurrences() {
    let s = Schedule::new(
        IST,
        vec![version("FREQ=DAILY", d(2026, 1, 10), Some(d(2026, 1, 12)))],
    )
    .unwrap();
    assert_eq!(
        s.occurrences(d(2026, 1, 1), d(2026, 1, 31)).unwrap(),
        [d(2026, 1, 10), d(2026, 1, 11), d(2026, 1, 12)]
    );
    let empty = Schedule::new(IST, vec![]).unwrap();
    assert!(
        empty
            .occurrences(d(2026, 1, 1), d(2026, 1, 31))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn range_validation() {
    let s = Schedule::new(IST, vec![version("FREQ=DAILY", d(2026, 1, 1), None)]).unwrap();
    assert_eq!(
        s.occurrences(d(2026, 1, 2), d(2026, 1, 1)),
        Err(ScheduleError::InvertedRange)
    );
    let first = d(2026, 1, 1);
    let ok_last = first + Days::new(u64::try_from(MAX_RANGE_DAYS - 1).unwrap());
    assert_eq!(
        s.occurrences(first, ok_last).unwrap().len(),
        usize::try_from(MAX_RANGE_DAYS).unwrap()
    );
    assert_eq!(
        s.occurrences(first, ok_last + Days::new(1)),
        Err(ScheduleError::RangeTooLong)
    );
    assert_eq!(s.occurrences(first, first).unwrap(), [first]);
}

// --- R11: plan changes keep history ---------------------------------------------------------

#[test]
fn plan_change_keeps_past_occurrences() {
    let mut s = Schedule::new(IST, vec![version("FREQ=DAILY", d(2026, 1, 1), None)]).unwrap();
    let before = s.occurrences(d(2026, 1, 1), d(2026, 1, 31)).unwrap();
    s.change_plan(rules("FREQ=WEEKLY;BYDAY=MO"), d(2026, 1, 15))
        .unwrap();
    let after = s.occurrences(d(2026, 1, 1), d(2026, 1, 31)).unwrap();
    assert_eq!(after[..14], before[..14]);
    // From the 15th (a Thursday) on, Mondays only: 19 and 26 January.
    assert_eq!(after[14..], [d(2026, 1, 19), d(2026, 1, 26)]);
    assert_eq!(s.versions()[0].valid_to, Some(d(2026, 1, 14)));
    assert_eq!(s.versions()[1].valid_from, d(2026, 1, 15));
}

#[test]
fn plan_change_anchors_intervals_on_its_first_day() {
    let mut s = Schedule::new(
        IST,
        vec![version("FREQ=DAILY;INTERVAL=2", d(2026, 1, 1), None)],
    )
    .unwrap();
    s.change_plan(rules("FREQ=DAILY;INTERVAL=3"), d(2026, 1, 6))
        .unwrap();
    let got = s.occurrences(d(2026, 1, 1), d(2026, 1, 13)).unwrap();
    assert_eq!(
        got,
        [
            d(2026, 1, 1),
            d(2026, 1, 3),
            d(2026, 1, 5),
            d(2026, 1, 6),
            d(2026, 1, 9),
            d(2026, 1, 12)
        ]
    );
}

#[test]
fn plan_change_on_the_first_day_replaces_the_version() {
    let mut s = Schedule::new(
        IST,
        vec![version("FREQ=DAILY", d(2026, 1, 1), Some(d(2026, 6, 30)))],
    )
    .unwrap();
    s.change_plan(rules("FREQ=WEEKLY"), d(2026, 1, 1)).unwrap();
    assert_eq!(
        s.versions(),
        [version("FREQ=WEEKLY", d(2026, 1, 1), Some(d(2026, 6, 30)))]
    );
}

#[test]
fn plan_change_keeps_the_end_date_and_validates() {
    let mut s = Schedule::new(
        IST,
        vec![version("FREQ=DAILY", d(2026, 1, 1), Some(d(2026, 6, 30)))],
    )
    .unwrap();
    assert_eq!(
        s.change_plan(rules("FREQ=WEEKLY"), d(2025, 12, 31)),
        Err(ScheduleError::ChangeBeforeCurrentVersion(d(2026, 1, 1)))
    );
    assert_eq!(
        s.change_plan(rules("FREQ=WEEKLY"), d(2026, 7, 1)),
        Err(ScheduleError::ChangeAfterEnd(d(2026, 6, 30)))
    );
    s.change_plan(rules("FREQ=WEEKLY"), d(2026, 6, 30)).unwrap();
    assert_eq!(
        s.versions()[1],
        version("FREQ=WEEKLY", d(2026, 6, 30), Some(d(2026, 6, 30)))
    );
}

#[test]
fn plan_change_on_an_empty_schedule_starts_it() {
    let mut s = Schedule::new(IST, vec![]).unwrap();
    s.change_plan(rules("FREQ=DAILY"), d(2026, 1, 1)).unwrap();
    assert!(s.occurs_on(d(2026, 1, 1)));
    assert!(!s.occurs_on(d(2025, 12, 31)));
}

fn date_strategy() -> impl Strategy<Value = NaiveDate> {
    (0u64..15_000).prop_map(|n| d(2000, 1, 1) + Days::new(n))
}

proptest! {
    /// R11: whatever the old and new rules and the change date, nothing before the change moves.
    #[test]
    fn plan_changes_never_rewrite_history(
        old in crate::rule::tests::rule_strategy(),
        new in crate::rule::tests::rule_strategy(),
        start in date_strategy(),
        change_after in 0u64..800,
        window in 1u64..800,
    ) {
        let original = Schedule::new(IST, vec![PlanVersion { rules: old.into(), valid_from: start, valid_to: None }]).unwrap();
        let change = start + Days::new(change_after);
        let mut changed = original.clone();
        changed.change_plan(new.clone().into(), change).unwrap();

        let last = change + Days::new(window);
        let before_old: Vec<_> = original.occurrences(start, last).unwrap().into_iter().filter(|x| *x < change).collect();
        let before_new: Vec<_> = changed.occurrences(start, last).unwrap().into_iter().filter(|x| *x < change).collect();
        prop_assert_eq!(before_old, before_new);
        // From the change on, the new rules apply, anchored on the change date.
        for x in changed.occurrences(change, last).unwrap() {
            prop_assert!(new.matches(change, x));
        }
    }
}

// --- R2: time zones and DST -----------------------------------------------------------------

#[test]
fn local_date_follows_the_routine_time_zone() {
    // 21:30 UTC is already the next day in Istanbul (UTC+3).
    assert_eq!(local_date(utc(2026, 11, 22, 21, 30), IST), d(2026, 11, 23));
    assert_eq!(local_date(utc(2026, 11, 22, 20, 59), IST), d(2026, 11, 22));
    let s = Schedule::new(IST, vec![]).unwrap();
    assert_eq!(s.today(utc(2026, 11, 22, 21, 0)), d(2026, 11, 23));
}

#[test]
fn dst_transitions_neither_skip_nor_repeat_days() {
    let ny: Tz = chrono_tz::America::New_York;
    // Spring forward on 2026-03-08 (02:00 EST -> 03:00 EDT): that day has 23 hours.
    assert_eq!(local_date(utc(2026, 3, 8, 6, 30), ny), d(2026, 3, 8)); // 01:30 EST
    assert_eq!(local_date(utc(2026, 3, 8, 7, 30), ny), d(2026, 3, 8)); // 03:30 EDT
    assert_eq!(local_date(utc(2026, 3, 9, 3, 59), ny), d(2026, 3, 8)); // 23:59 EDT
    assert_eq!(local_date(utc(2026, 3, 9, 4, 0), ny), d(2026, 3, 9));
    // Fall back on 2026-11-01: 25 hours, 01:xx happens twice, still one date.
    assert_eq!(local_date(utc(2026, 11, 1, 5, 30), ny), d(2026, 11, 1)); // 01:30 EDT
    assert_eq!(local_date(utc(2026, 11, 1, 6, 30), ny), d(2026, 11, 1)); // 01:30 EST
    assert_eq!(local_date(utc(2026, 11, 2, 4, 59), ny), d(2026, 11, 1)); // 23:59 EST

    let s = Schedule::new(ny, vec![version("FREQ=DAILY", d(2026, 1, 1), None)]).unwrap();
    let got = s
        .occurrences_between(utc(2026, 3, 7, 12, 0), utc(2026, 3, 10, 3, 0))
        .unwrap();
    assert_eq!(got, [d(2026, 3, 7), d(2026, 3, 8), d(2026, 3, 9)]);
    let got = s
        .occurrences_between(utc(2026, 10, 31, 12, 0), utc(2026, 11, 2, 12, 0))
        .unwrap();
    assert_eq!(got, [d(2026, 10, 31), d(2026, 11, 1), d(2026, 11, 2)]);
}

const ZONES: [Tz; 7] = [
    chrono_tz::America::New_York,
    chrono_tz::Europe::London,
    chrono_tz::Europe::Istanbul,
    chrono_tz::Australia::Sydney,
    chrono_tz::Pacific::Chatham,
    chrono_tz::America::Santiago,
    chrono_tz::Asia::Kolkata,
];

proptest! {
    /// Across zones with and without DST, a daily routine yields every local date in a window
    /// exactly once, in order, from the local date of its start to that of its end.
    #[test]
    fn daily_occurrences_are_contiguous_across_dst(
        zone in 0usize..ZONES.len(),
        start_secs in 1_577_836_800i64..1_893_456_000, // 2020-01-01 .. 2030-01-01
        hours in 0i64..24 * 120,
    ) {
        let tz = ZONES[zone];
        let from = DateTime::from_timestamp(start_secs, 0).unwrap();
        let to = from + chrono::Duration::hours(hours);
        let s = Schedule::new(tz, vec![version("FREQ=DAILY", d(2019, 1, 1), None)]).unwrap();
        let got = s.occurrences_between(from, to).unwrap();
        prop_assert_eq!(got.first().copied(), Some(local_date(from, tz)));
        prop_assert_eq!(got.last().copied(), Some(local_date(to, tz)));
        for pair in got.windows(2) {
            prop_assert_eq!(pair[0].succ_opt(), Some(pair[1]));
        }
        // Hourly samples never land on a date outside the result.
        let mut t = from;
        while t <= to {
            prop_assert!(got.contains(&local_date(t, tz)));
            t += chrono::Duration::hours(1);
        }
    }
}
