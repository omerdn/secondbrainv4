// Test module: panics are the failure mechanism.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{Datelike, NaiveDate, Weekday};
use proptest::prelude::*;

use super::*;

pub(crate) fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn occurrences(
    rules: &RuleSet,
    start: NaiveDate,
    first: NaiveDate,
    last: NaiveDate,
) -> Vec<NaiveDate> {
    first
        .iter_days()
        .take_while(|x| *x <= last)
        .filter(|x| rules.matches(start, *x))
        .collect()
}

fn parse(s: &str) -> RuleSet {
    s.parse().unwrap()
}

// --- R1: every recurrence type -------------------------------------------------------------

#[test]
fn daily() {
    let got = occurrences(
        &parse("FREQ=DAILY"),
        d(2026, 1, 30),
        d(2026, 1, 28),
        d(2026, 2, 2),
    );
    assert_eq!(
        got,
        [d(2026, 1, 30), d(2026, 1, 31), d(2026, 2, 1), d(2026, 2, 2)]
    );
}

#[test]
fn every_n_days() {
    let got = occurrences(
        &parse("FREQ=DAILY;INTERVAL=3"),
        d(2026, 2, 26),
        d(2026, 2, 1),
        d(2026, 3, 10),
    );
    assert_eq!(
        got,
        [
            d(2026, 2, 26),
            d(2026, 3, 1),
            d(2026, 3, 4),
            d(2026, 3, 7),
            d(2026, 3, 10)
        ]
    );
}

#[test]
fn weekly_on_selected_days() {
    // 2026-11-23 is a Monday.
    let got = occurrences(
        &parse("FREQ=WEEKLY;BYDAY=TH,MO"),
        d(2026, 11, 25),
        d(2026, 11, 23),
        d(2026, 12, 3),
    );
    assert_eq!(got, [d(2026, 11, 26), d(2026, 11, 30), d(2026, 12, 3)]);
}

#[test]
fn weekly_defaults_to_the_start_weekday() {
    let got = occurrences(
        &parse("FREQ=WEEKLY"),
        d(2026, 11, 25),
        d(2026, 11, 1),
        d(2026, 12, 10),
    );
    assert_eq!(got, [d(2026, 11, 25), d(2026, 12, 2), d(2026, 12, 9)]);
}

#[test]
fn every_n_weeks() {
    // Every other week on Monday and Thursday, starting Wednesday 2026-11-25: weeks of 11-23,
    // 12-07, 12-21 count; the Monday before the start does not.
    let got = occurrences(
        &parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH"),
        d(2026, 11, 25),
        d(2026, 11, 23),
        d(2026, 12, 24),
    );
    assert_eq!(
        got,
        [
            d(2026, 11, 26),
            d(2026, 12, 7),
            d(2026, 12, 10),
            d(2026, 12, 21),
            d(2026, 12, 24)
        ]
    );
}

#[test]
fn monthly_on_selected_days() {
    let got = occurrences(
        &parse("FREQ=MONTHLY;BYMONTHDAY=20,15"),
        d(2026, 1, 16),
        d(2026, 1, 1),
        d(2026, 3, 1),
    );
    assert_eq!(got, [d(2026, 1, 20), d(2026, 2, 15), d(2026, 2, 20)]);
}

#[test]
fn monthly_on_the_last_day() {
    let got = occurrences(
        &parse("FREQ=MONTHLY;BYMONTHDAY=-1"),
        d(2028, 1, 1),
        d(2028, 1, 1),
        d(2028, 4, 30),
    );
    assert_eq!(
        got,
        [
            d(2028, 1, 31),
            d(2028, 2, 29),
            d(2028, 3, 31),
            d(2028, 4, 30)
        ]
    );
}

#[test]
fn monthly_on_the_31st_skips_short_months() {
    let got = occurrences(
        &parse("FREQ=MONTHLY;BYMONTHDAY=31"),
        d(2026, 1, 1),
        d(2026, 1, 1),
        d(2026, 5, 31),
    );
    assert_eq!(got, [d(2026, 1, 31), d(2026, 3, 31), d(2026, 5, 31)]);
}

#[test]
fn monthly_defaults_to_the_start_day_and_honours_interval() {
    let got = occurrences(
        &parse("FREQ=MONTHLY;INTERVAL=2"),
        d(2026, 1, 30),
        d(2026, 1, 1),
        d(2026, 7, 31),
    );
    // March 30, (May 30), July 30; February is skipped by the interval anyway.
    assert_eq!(
        got,
        [
            d(2026, 1, 30),
            d(2026, 3, 30),
            d(2026, 5, 30),
            d(2026, 7, 30)
        ]
    );
}

#[test]
fn yearly_on_selected_dates() {
    let rules = RuleSet::new([
        Rule::yearly(1, 3, 8).unwrap(),
        Rule::yearly(1, 12, 25).unwrap(),
    ])
    .unwrap();
    assert_eq!(
        rules.to_string(),
        "FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=8\nFREQ=YEARLY;BYMONTH=12;BYMONTHDAY=25"
    );
    let got = occurrences(&rules, d(2026, 1, 1), d(2026, 1, 1), d(2027, 12, 31));
    assert_eq!(
        got,
        [
            d(2026, 3, 8),
            d(2026, 12, 25),
            d(2027, 3, 8),
            d(2027, 12, 25)
        ]
    );
    assert_eq!(rules.to_string().parse::<RuleSet>().unwrap(), rules);
}

#[test]
fn yearly_defaults_and_leap_day() {
    // Started on 29 February: only leap years have an occurrence.
    let got = occurrences(
        &parse("FREQ=YEARLY"),
        d(2024, 2, 29),
        d(2024, 1, 1),
        d(2032, 12, 31),
    );
    assert_eq!(got, [d(2024, 2, 29), d(2028, 2, 29), d(2032, 2, 29)]);
    let got = occurrences(
        &parse("FREQ=YEARLY;INTERVAL=2;BYMONTH=6"),
        d(2026, 1, 15),
        d(2026, 1, 1),
        d(2030, 12, 31),
    );
    assert_eq!(got, [d(2026, 6, 15), d(2028, 6, 15), d(2030, 6, 15)]);
}

#[test]
fn nothing_before_the_start() {
    let rules = parse("FREQ=DAILY");
    assert!(!rules.matches(d(2026, 5, 2), d(2026, 5, 1)));
}

// --- constructors and printing ------------------------------------------------------------

#[test]
fn constructors_normalize_and_print_canonically() {
    let weekly = Rule::weekly(2, [Weekday::Sun, Weekday::Mon, Weekday::Sun]);
    assert_eq!(weekly.to_string(), "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,SU");
    assert_eq!(weekly.frequency(), Frequency::Weekly);
    assert_eq!(weekly.interval(), 2);
    assert_eq!(Rule::daily(0).interval(), 1, "interval 0 is treated as 1");
    assert_eq!(Rule::daily(1).to_string(), "FREQ=DAILY");
    assert_eq!(
        Rule::monthly(1, [20, -1, 15, 20]).unwrap().to_string(),
        "FREQ=MONTHLY;BYMONTHDAY=-1,15,20"
    );
    let all_days = Rule::weekly(
        1,
        [
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
        ],
    );
    assert_eq!(all_days.to_string(), "FREQ=WEEKLY;BYDAY=TU,WE,TH,FR,SA");
    assert_eq!(Frequency::Yearly.to_string(), "YEARLY");
}

#[test]
fn constructors_validate() {
    assert!(matches!(
        Rule::monthly(1, [0]),
        Err(RuleError::InvalidValue {
            part: "BYMONTHDAY",
            ..
        })
    ));
    assert!(matches!(
        Rule::monthly(1, [32]),
        Err(RuleError::InvalidValue {
            part: "BYMONTHDAY",
            ..
        })
    ));
    assert!(matches!(
        Rule::yearly(1, 13, 1),
        Err(RuleError::InvalidValue {
            part: "BYMONTH",
            ..
        })
    ));
    assert!(matches!(
        Rule::yearly(1, 0, 1),
        Err(RuleError::InvalidValue {
            part: "BYMONTH",
            ..
        })
    ));
    assert_eq!(RuleSet::new([]), Err(RuleError::Empty));
}

#[test]
fn rule_set_dedups_and_converts() {
    let set = RuleSet::new([Rule::daily(1), Rule::daily(1)]).unwrap();
    assert_eq!(set.rules().len(), 1);
    assert_eq!(RuleSet::from(Rule::daily(1)), set);
}

// --- parsing --------------------------------------------------------------------------------

#[test]
fn parsing_is_lenient_about_case_prefix_and_blank_lines() {
    let rule: Rule = "rrule:freq=weekly; byday=mo"
        .to_uppercase()
        .parse()
        .unwrap();
    assert_eq!(rule, Rule::weekly(1, [Weekday::Mon]));
    let rule: Rule = " freq = monthly ; bymonthday = -1 ".parse().unwrap();
    assert_eq!(rule, Rule::monthly(1, [-1]).unwrap());
    let set: RuleSet = "\nFREQ=DAILY\n\n  \nFREQ=WEEKLY\n".parse().unwrap();
    assert_eq!(set.rules().len(), 2);
}

#[test]
fn parse_errors() {
    let err = |s: &str| s.parse::<Rule>().unwrap_err();
    assert_eq!(err(""), RuleError::Empty);
    assert_eq!(err("RRULE:"), RuleError::Empty);
    assert_eq!("  \n ".parse::<RuleSet>().unwrap_err(), RuleError::Empty);
    assert_eq!(err("FREQ"), RuleError::Malformed("FREQ".into()));
    assert_eq!(err("INTERVAL=2"), RuleError::MissingFreq);
    assert_eq!(
        err("FREQ=HOURLY"),
        RuleError::InvalidValue {
            part: "FREQ",
            value: "HOURLY".into()
        }
    );
    assert_eq!(
        err("FREQ=DAILY;FREQ=DAILY"),
        RuleError::Duplicate("FREQ".into())
    );
    assert_eq!(
        err("FREQ=DAILY;INTERVAL=2;INTERVAL=3"),
        RuleError::Duplicate("INTERVAL".into())
    );
    assert_eq!(
        err("FREQ=WEEKLY;BYDAY=MO;BYDAY=TU"),
        RuleError::Duplicate("BYDAY".into())
    );
    assert_eq!(
        err("FREQ=MONTHLY;BYMONTHDAY=1;BYMONTHDAY=2"),
        RuleError::Duplicate("BYMONTHDAY".into())
    );
    assert_eq!(
        err("FREQ=YEARLY;BYMONTH=1;BYMONTH=2"),
        RuleError::Duplicate("BYMONTH".into())
    );
    assert_eq!(
        err("FREQ=DAILY;INTERVAL=0"),
        RuleError::InvalidValue {
            part: "INTERVAL",
            value: "0".into()
        }
    );
    assert_eq!(
        err("FREQ=DAILY;INTERVAL=x"),
        RuleError::InvalidValue {
            part: "INTERVAL",
            value: "X".into()
        }
    );
    assert_eq!(
        err("FREQ=WEEKLY;BYDAY=1MO"),
        RuleError::InvalidValue {
            part: "BYDAY",
            value: "1MO".into()
        }
    );
    assert_eq!(
        err("FREQ=MONTHLY;BYMONTHDAY=0"),
        RuleError::InvalidValue {
            part: "BYMONTHDAY",
            value: "0".into()
        }
    );
    assert_eq!(
        err("FREQ=MONTHLY;BYMONTHDAY=-32"),
        RuleError::InvalidValue {
            part: "BYMONTHDAY",
            value: "-32".into()
        }
    );
    assert_eq!(
        err("FREQ=MONTHLY;BYMONTHDAY=300"),
        RuleError::InvalidValue {
            part: "BYMONTHDAY",
            value: "300".into()
        }
    );
    assert_eq!(
        err("FREQ=YEARLY;BYMONTH=13"),
        RuleError::InvalidValue {
            part: "BYMONTH",
            value: "13".into()
        }
    );
    assert_eq!(
        err("FREQ=YEARLY;BYMONTH=a"),
        RuleError::InvalidValue {
            part: "BYMONTH",
            value: "A".into()
        }
    );
    assert_eq!(
        err("FREQ=DAILY;COUNT=3"),
        RuleError::Unsupported("COUNT".into())
    );
    assert_eq!(
        err("FREQ=DAILY;UNTIL=20261231"),
        RuleError::Unsupported("UNTIL".into())
    );
    assert_eq!(
        err("FREQ=MONTHLY;BYDAY=MO"),
        RuleError::NotAllowed {
            part: "BYDAY",
            freq: Frequency::Monthly
        }
    );
    assert_eq!(
        err("FREQ=DAILY;BYMONTHDAY=1"),
        RuleError::NotAllowed {
            part: "BYMONTHDAY",
            freq: Frequency::Daily
        }
    );
    assert_eq!(
        err("FREQ=WEEKLY;BYMONTHDAY=1"),
        RuleError::NotAllowed {
            part: "BYMONTHDAY",
            freq: Frequency::Weekly
        }
    );
    assert_eq!(
        err("FREQ=MONTHLY;BYMONTH=1"),
        RuleError::NotAllowed {
            part: "BYMONTH",
            freq: Frequency::Monthly
        }
    );
    // Every error renders a message.
    assert_eq!(
        err("FREQ=MONTHLY;BYMONTH=1").to_string(),
        "BYMONTH cannot be used with FREQ=MONTHLY"
    );
}

// --- properties -----------------------------------------------------------------------------

fn date_strategy() -> impl Strategy<Value = NaiveDate> {
    // 1990-01-01 .. ~2060
    (0u64..25_000).prop_map(|n| d(1990, 1, 1) + chrono::Days::new(n))
}

fn weekday_strategy() -> impl Strategy<Value = Weekday> {
    (0u8..7).prop_map(|n| Weekday::try_from(n).unwrap())
}

fn month_day_strategy() -> impl Strategy<Value = i8> {
    prop_oneof![1i8..=31, -31i8..=-1]
}

pub(crate) fn rule_strategy() -> impl Strategy<Value = Rule> {
    prop_oneof![
        (1u32..10).prop_map(Rule::daily),
        (1u32..5, proptest::collection::vec(weekday_strategy(), 0..4))
            .prop_map(|(i, days)| Rule::weekly(i, days)),
        (
            1u32..4,
            proptest::collection::vec(month_day_strategy(), 0..3)
        )
            .prop_map(|(i, days)| Rule::monthly(i, days).unwrap()),
        (1u32..3, 1u8..=12, month_day_strategy())
            .prop_map(|(i, m, day)| Rule::yearly(i, m, day).unwrap()),
    ]
}

proptest! {
    #[test]
    fn display_and_parse_round_trip(rule in rule_strategy()) {
        let text = rule.to_string();
        prop_assert_eq!(text.parse::<Rule>().unwrap(), rule);
    }

    /// "The 31st": an occurrence on every month in range that has a 31st, and nowhere else.
    #[test]
    fn the_31st_occurs_exactly_in_31_day_months(start in date_strategy(), span in 0u64..1_500, interval in 1u32..4) {
        let rules = RuleSet::from(Rule::monthly(interval, [31]).unwrap());
        let last = start + chrono::Days::new(span);
        let got = occurrences(&rules, start, start, last);
        for x in &got {
            prop_assert_eq!(x.day(), 31);
            prop_assert_eq!(months_since(start, *x) % i64::from(interval), 0);
        }
        let expected = start
            .iter_days()
            .take_while(|x| *x <= last)
            .filter(|x| x.day() == 31 && months_since(start, *x) % i64::from(interval) == 0)
            .count();
        prop_assert_eq!(got.len(), expected);
    }

    /// "The last day of the month": exactly one occurrence per (selected) month, on its last day.
    #[test]
    fn the_last_day_occurs_once_per_month(start in date_strategy(), months in 1i64..60, interval in 1u32..4) {
        let rules = RuleSet::from(Rule::monthly(interval, [-1]).unwrap());
        let first = start.with_day(1).unwrap();
        let last = first.checked_add_months(chrono::Months::new(u32::try_from(months).unwrap())).unwrap().pred_opt().unwrap();
        let got = occurrences(&rules, first, first, last);
        for x in &got {
            prop_assert_eq!(x.succ_opt().unwrap().day(), 1, "{} is not a month end", x);
        }
        let selected = (0..months).filter(|m| m % i64::from(interval) == 0).count();
        prop_assert_eq!(got.len(), selected);
    }

    /// A negative month day -n matches the same date as the positive day (length + 1 - n).
    #[test]
    fn negative_month_days_count_from_the_end(date in date_strategy(), n in 1i8..=31) {
        let len = days_in_month(date.year(), date.month());
        let positive = i64::from(len) + 1 - i64::from(n);
        prop_assert_eq!(is_month_day(date, -n), i64::from(date.day()) == positive);
    }
}
