// Test module: panics are the failure mechanism.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{Days, NaiveDate, Weekday};
use chrono_tz::Tz;
use proptest::prelude::*;

use super::*;
use crate::schedule::PlanVersion;

const IST: Tz = chrono_tz::Europe::Istanbul;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

fn schedule(rule: &str, from: NaiveDate) -> Schedule {
    Schedule::new(
        IST,
        vec![PlanVersion {
            rules: rule.parse().unwrap(),
            valid_from: from,
            valid_to: None,
        }],
    )
    .unwrap()
}

fn who(rotation: &Rotation<char>, s: &Schedule, first: NaiveDate, last: NaiveDate) -> String {
    rotation
        .assignments(s, first, last)
        .unwrap()
        .iter()
        .map(|a| a.assignee)
        .collect()
}

#[test]
fn members_are_required_and_unique() {
    assert_eq!(
        Rotation::<char>::new(vec![], RotationCadence::Weekly, d(1, 1)),
        Err(RotationError::NoMembers)
    );
    assert_eq!(
        Rotation::new(vec!['a', 'b', 'a'], RotationCadence::Weekly, d(1, 1)),
        Err(RotationError::DuplicateMember)
    );
}

#[test]
fn every_occurrence_turns_over_the_list() {
    let s = schedule("FREQ=DAILY", d(3, 2));
    let r = Rotation::new(
        vec!['a', 'b', 'c'],
        RotationCadence::EveryOccurrence,
        d(3, 2),
    )
    .unwrap();
    assert_eq!(who(&r, &s, d(3, 2), d(3, 8)), "abcabca");
    assert_eq!(r.assignee_on(&s, d(3, 5)).unwrap(), 'a');
    assert_eq!(r.assignee_on(&s, d(3, 6)).unwrap(), 'b');
}

#[test]
fn only_real_occurrences_take_a_turn() {
    // Mondays and Thursdays: skipped days do not consume anyone's turn.
    let s = schedule("FREQ=WEEKLY;BYDAY=MO,TH", d(3, 2));
    let r = Rotation::new(vec!['a', 'b'], RotationCadence::EveryOccurrence, d(3, 2)).unwrap();
    let got = r.assignments(&s, d(3, 2), d(3, 16)).unwrap();
    let dates: Vec<_> = got.iter().map(|a| a.date).collect();
    assert_eq!(dates, [d(3, 2), d(3, 5), d(3, 9), d(3, 12), d(3, 16)]);
    assert_eq!(who(&r, &s, d(3, 2), d(3, 16)), "ababa");
    assert_eq!(
        r.assignee_on(&s, d(3, 3)),
        Err(RotationError::NotAnOccurrence(d(3, 3)))
    );
}

#[test]
fn weekly_keeps_one_person_for_the_calendar_week() {
    // Starts on a Wednesday: its short first week is still the first person's.
    let s = schedule("FREQ=DAILY", d(3, 4));
    let r = Rotation::new(vec!['a', 'b'], RotationCadence::Weekly, d(3, 4)).unwrap();
    assert_eq!(d(3, 4).weekday(), Weekday::Wed);
    assert_eq!(who(&r, &s, d(3, 4), d(3, 8)), "aaaaa");
    assert_eq!(who(&r, &s, d(3, 9), d(3, 15)), "bbbbbbb");
    assert_eq!(who(&r, &s, d(3, 16), d(3, 16)), "a");
}

#[test]
fn a_window_continues_the_count_from_the_start() {
    let s = schedule("FREQ=DAILY", d(1, 1));
    let r = Rotation::new(
        vec!['a', 'b', 'c'],
        RotationCadence::EveryOccurrence,
        d(1, 1),
    )
    .unwrap();
    // January 11 is the eleventh occurrence.
    assert_eq!(who(&r, &s, d(1, 11), d(1, 13)), "bca");
    // Days before the start have nobody.
    let later = Rotation::new(vec!['a', 'b'], RotationCadence::EveryOccurrence, d(1, 5)).unwrap();
    assert_eq!(who(&later, &s, d(1, 1), d(1, 6)), "ab");
    assert_eq!(
        later.assignee_on(&s, d(1, 4)),
        Err(RotationError::NotAnOccurrence(d(1, 4)))
    );
    assert!(later.assignments(&s, d(1, 1), d(1, 4)).unwrap().is_empty());
}

#[test]
fn a_swap_exchanges_two_days_and_shifts_nobody_else() {
    let s = schedule("FREQ=DAILY", d(3, 2));
    let mut r = Rotation::new(
        vec!['a', 'b', 'c'],
        RotationCadence::EveryOccurrence,
        d(3, 2),
    )
    .unwrap();
    r.swap(&s, d(3, 2), d(3, 3)).unwrap();
    assert_eq!(who(&r, &s, d(3, 2), d(3, 8)), "bacabca");
    assert_eq!(r.overrides().len(), 2);
    assert_eq!(r.assignee_on(&s, d(3, 2)).unwrap(), 'b');

    // Swapping back leaves no override behind.
    r.swap(&s, d(3, 2), d(3, 3)).unwrap();
    assert!(r.overrides().is_empty());
    assert_eq!(who(&r, &s, d(3, 2), d(3, 8)), "abcabca");
}

#[test]
fn a_chain_of_swaps_uses_the_current_people() {
    let s = schedule("FREQ=DAILY", d(3, 2));
    let mut r = Rotation::new(
        vec!['a', 'b', 'c'],
        RotationCadence::EveryOccurrence,
        d(3, 2),
    )
    .unwrap();
    r.swap(&s, d(3, 2), d(3, 3)).unwrap(); // b a c
    r.swap(&s, d(3, 3), d(3, 4)).unwrap(); // b c a
    assert_eq!(who(&r, &s, d(3, 2), d(3, 4)), "bca");
}

#[test]
fn a_swap_works_on_one_day_of_a_weekly_turn() {
    let s = schedule("FREQ=DAILY", d(3, 2));
    let mut r = Rotation::new(vec!['a', 'b'], RotationCadence::Weekly, d(3, 2)).unwrap();
    r.swap(&s, d(3, 4), d(3, 10)).unwrap();
    assert_eq!(who(&r, &s, d(3, 2), d(3, 8)), "aabaaaa");
    assert_eq!(who(&r, &s, d(3, 9), d(3, 15)), "babbbbb");
}

#[test]
fn invalid_swaps_change_nothing() {
    let s = schedule("FREQ=WEEKLY;BYDAY=MO", d(3, 2));
    let mut r = Rotation::new(vec!['a', 'b'], RotationCadence::EveryOccurrence, d(3, 2)).unwrap();
    let before = r.clone();
    assert_eq!(
        r.swap(&s, d(3, 2), d(3, 3)),
        Err(RotationError::NotAnOccurrence(d(3, 3)))
    );
    assert_eq!(
        r.swap(&s, d(3, 2), d(3, 2)),
        Err(RotationError::SameAssignee)
    );
    assert_eq!(
        r.swap(&s, d(3, 2), d(3, 16)),
        Err(RotationError::SameAssignee),
        "both are a's turn"
    );
    assert_eq!(r, before);
}

#[test]
fn stored_overrides_are_restored_and_checked() {
    let s = schedule("FREQ=DAILY", d(3, 2));
    let r = Rotation::new(vec!['a', 'b'], RotationCadence::EveryOccurrence, d(3, 2))
        .unwrap()
        .with_overrides([(d(3, 2), 'b')])
        .unwrap();
    assert_eq!(who(&r, &s, d(3, 2), d(3, 3)), "bb");
    assert_eq!(
        Rotation::new(vec!['a'], RotationCadence::Weekly, d(3, 2))
            .unwrap()
            .with_overrides([(d(3, 2), 'x')]),
        Err(RotationError::UnknownMember(d(3, 2)))
    );
}

#[test]
fn ranges_are_bounded() {
    let s = schedule("FREQ=DAILY", d(1, 1));
    let r = Rotation::new(vec!['a'], RotationCadence::EveryOccurrence, d(1, 1)).unwrap();
    let far = d(1, 1) + Days::new(u64::try_from(MAX_ROTATION_DAYS).unwrap() + 1);
    assert_eq!(r.assignee_on(&s, far), Err(RotationError::TooFarFromStart));
    assert_eq!(
        r.assignments(&s, far, far),
        Err(RotationError::TooFarFromStart)
    );
    assert_eq!(
        r.assignments(&s, d(1, 2), d(1, 1)),
        Err(RotationError::Schedule(ScheduleError::InvertedRange))
    );
}

#[test]
fn a_plan_change_keeps_counting_real_occurrences() {
    let s = Schedule::new(
        IST,
        vec![
            PlanVersion {
                rules: "FREQ=DAILY".parse().unwrap(),
                valid_from: d(3, 2),
                valid_to: Some(d(3, 3)),
            },
            PlanVersion {
                rules: "FREQ=WEEKLY;BYDAY=MO".parse().unwrap(),
                valid_from: d(3, 9),
                valid_to: None,
            },
        ],
    )
    .unwrap();
    let r = Rotation::new(
        vec!['a', 'b', 'c'],
        RotationCadence::EveryOccurrence,
        d(3, 2),
    )
    .unwrap();
    assert_eq!(who(&r, &s, d(3, 2), d(3, 23)), "abcab");
}

const RULES: [&str; 3] = [
    "FREQ=DAILY",
    "FREQ=WEEKLY;BYDAY=MO,WE,FR",
    "FREQ=DAILY;INTERVAL=2",
];

fn cadence() -> impl Strategy<Value = RotationCadence> {
    prop_oneof![
        Just(RotationCadence::EveryOccurrence),
        Just(RotationCadence::Weekly)
    ]
}

fn setup(rule: usize, members: u8, cadence: RotationCadence) -> (Schedule, Rotation<u8>) {
    let start = d(1, 5);
    let s = schedule(RULES[rule], start);
    let r = Rotation::new((0..members).collect(), cadence, start).unwrap();
    (s, r)
}

proptest! {
    #[test]
    fn every_occurrence_is_a_fair_cycle(rule in 0..RULES.len(), members in 1u8..7) {
        let (s, r) = setup(rule, members, RotationCadence::EveryOccurrence);
        let got = r.assignments(&s, d(1, 5), d(6, 30)).unwrap();
        for (i, a) in got.iter().enumerate() {
            prop_assert_eq!(usize::from(a.assignee), i % usize::from(members));
        }
        let counts: Vec<usize> = (0..members)
            .map(|m| got.iter().filter(|a| a.assignee == m).count())
            .collect();
        let (min, max) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
        prop_assert!(max - min <= 1);
    }

    #[test]
    fn a_week_has_one_person_and_the_next_week_the_next(
        rule in 0..RULES.len(),
        members in 2u8..7,
    ) {
        let (s, r) = setup(rule, members, RotationCadence::Weekly);
        let got = r.assignments(&s, d(1, 5), d(6, 30)).unwrap();
        for a in &got {
            let weeks = (monday(a.date) - monday(d(1, 5))).num_days();
            prop_assert_eq!(i64::from(a.assignee), (weeks / 7) % i64::from(members));
        }
    }

    #[test]
    fn a_window_agrees_with_the_whole(
        rule in 0..RULES.len(),
        members in 1u8..7,
        cadence in cadence(),
        from in 0u64..120,
        len in 0u64..40,
    ) {
        let (s, r) = setup(rule, members, cadence);
        let whole = r.assignments(&s, d(1, 5), d(7, 31)).unwrap();
        let (first, last) = (d(1, 5) + Days::new(from), d(1, 5) + Days::new(from + len));
        let window = r.assignments(&s, first, last).unwrap();
        let expected: Vec<_> = whole
            .iter()
            .copied()
            .filter(|a| a.date >= first && a.date <= last)
            .collect();
        prop_assert_eq!(&window, &expected);
        for a in window {
            prop_assert_eq!(r.assignee_on(&s, a.date).unwrap(), a.assignee);
        }
    }

    #[test]
    fn a_swap_changes_exactly_its_two_dates(
        rule in 0..RULES.len(),
        members in 2u8..7,
        cadence in cadence(),
        picks in prop::collection::vec((0usize..60, 0usize..60), 1..8),
    ) {
        let (s, mut r) = setup(rule, members, cadence);
        let (first, last) = (d(1, 5), d(6, 30));
        for (x, y) in picks {
            let before = r.assignments(&s, first, last).unwrap();
            let (a, b) = (before[x % before.len()], before[y % before.len()]);
            let result = r.swap(&s, a.date, b.date);
            let after = r.assignments(&s, first, last).unwrap();
            if a.assignee == b.assignee {
                prop_assert_eq!(result, Err(RotationError::SameAssignee));
                prop_assert_eq!(&after, &before);
                continue;
            }
            prop_assert!(result.is_ok());
            for (old, new) in before.iter().zip(&after) {
                let expected = if old.date == a.date {
                    b.assignee
                } else if old.date == b.date {
                    a.assignee
                } else {
                    old.assignee
                };
                prop_assert_eq!(new.assignee, expected);
            }
            // Swapping the same two dates again restores the previous state.
            let mut undone = r.clone();
            undone.swap(&s, a.date, b.date).unwrap();
            prop_assert_eq!(undone.assignments(&s, first, last).unwrap(), before);
        }
    }

    #[test]
    fn swaps_survive_being_stored(
        rule in 0..RULES.len(),
        members in 2u8..7,
        cadence in cadence(),
        x in 0usize..60,
        y in 0usize..60,
    ) {
        let (s, mut r) = setup(rule, members, cadence);
        let (first, last) = (d(1, 5), d(6, 30));
        let all = r.assignments(&s, first, last).unwrap();
        let (a, b) = (all[x % all.len()], all[y % all.len()]);
        let _ = r.swap(&s, a.date, b.date);
        let restored = Rotation::new((0..members).collect(), cadence, first)
            .unwrap()
            .with_overrides(r.overrides().iter().map(|(date, who)| (*date, *who)))
            .unwrap();
        prop_assert_eq!(
            restored.assignments(&s, first, last).unwrap(),
            r.assignments(&s, first, last).unwrap()
        );
    }
}
