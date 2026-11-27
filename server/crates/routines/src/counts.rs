//! How much of a routine's day is done (PRD R3–R5, R9, R10, T3.3.1): one pure rule that the
//! heatmap, the day state, the streak, goal progress and reminders all read.
//!
//! - **Shared pool** (`pool`, the default and what a personal routine is): whoever does it adds to
//!   one counter; with a target of 1 one person is enough.
//! - **Each person** (`individual`): every responsible person must reach the target. A day's
//!   target is the target times the number of them, and each adds at most the target, so one
//!   person doing more never covers another's shortfall.
//!
//! Since everyone reads the same counts, a shared routine's streak is shared without anything of
//! its own (R10).

use std::collections::HashMap;

use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use tracking::TrackerEntry;
use ts_rs::TS;
use uuid::Uuid;

/// How a shared routine's day is completed (R5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "RoutineCompletionMode")]
pub enum CompletionMode {
    #[default]
    Pool,
    Individual,
}

impl CompletionMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pool => "pool",
            Self::Individual => "individual",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pool" => Some(Self::Pool),
            "individual" => Some(Self::Individual),
            _ => None,
        }
    }
}

/// A day's (done, target) under `mode`, from what each person did that day. `people` are the
/// responsible ones; in `individual` mode only they count, each up to `target`.
#[must_use]
pub fn effective(
    mode: CompletionMode,
    target: i32,
    people: &[Uuid],
    by_person: &HashMap<Uuid, i32>,
) -> (i32, i32) {
    match mode {
        CompletionMode::Pool => (
            by_person
                .values()
                .fold(0_i32, |sum, done| sum.saturating_add((*done).max(0))),
            target,
        ),
        CompletionMode::Individual => {
            let target = target.max(0);
            let done = people.iter().fold(0_i32, |sum, person| {
                let theirs = by_person.get(person).copied().unwrap_or(0).clamp(0, target);
                sum.saturating_add(theirs)
            });
            let count = i32::try_from(people.len()).unwrap_or(i32::MAX).max(1);
            (done, target.saturating_mul(count))
        }
    }
}

/// Who still has something to do on a day (R5, R13, T3.3.4): in a shared pool everyone in
/// `people` until the day is complete, in each-person mode those below `target`. `people` are
/// the responsible ones.
#[must_use]
pub fn still_to_do(
    mode: CompletionMode,
    target: i32,
    people: &[Uuid],
    by_person: &HashMap<Uuid, i32>,
) -> Vec<Uuid> {
    match mode {
        CompletionMode::Pool => {
            let (done, target) = effective(mode, target, people, by_person);
            if done >= target {
                Vec::new()
            } else {
                people.to_vec()
            }
        }
        CompletionMode::Individual => people
            .iter()
            .copied()
            .filter(|person| by_person.get(person).copied().unwrap_or(0) < target.max(0))
            .collect(),
    }
}

/// What everyone did on each day, ready to be read under a mode.
#[derive(Debug, Clone, Default)]
pub struct DayCounts {
    mode: CompletionMode,
    people: Vec<Uuid>,
    by_day: HashMap<NaiveDate, HashMap<Uuid, i32>>,
}

impl DayCounts {
    /// `people` are the routine's responsible ones (never empty: the owner when nobody is).
    #[must_use]
    pub fn new(mode: CompletionMode, people: Vec<Uuid>, entries: &[TrackerEntry]) -> Self {
        let mut by_day: HashMap<NaiveDate, HashMap<Uuid, i32>> = HashMap::new();
        for entry in entries {
            let count = entry.value.trunc().to_i32().unwrap_or(i32::MAX);
            let done = by_day
                .entry(entry.local_date)
                .or_default()
                .entry(entry.user_id)
                .or_default();
            *done = done.saturating_add(count);
        }
        Self {
            mode,
            people,
            by_day,
        }
    }

    /// A shared pool from totals per day: what a personal routine is.
    #[must_use]
    pub fn pool(totals: &HashMap<NaiveDate, i32>) -> Self {
        let someone = Uuid::nil();
        Self {
            mode: CompletionMode::Pool,
            people: vec![someone],
            by_day: totals
                .iter()
                .map(|(day, done)| (*day, HashMap::from([(someone, *done)])))
                .collect(),
        }
    }

    /// The day's (done, target) for a plan target of `target`.
    #[must_use]
    pub fn day(&self, date: NaiveDate, target: i32) -> (i32, i32) {
        let empty = HashMap::new();
        let by_person = self.by_day.get(&date).unwrap_or(&empty);
        effective(self.mode, target, &self.people, by_person)
    }

    /// [`still_to_do`] for one day of these counts.
    #[must_use]
    pub fn still_to_do(&self, date: NaiveDate, target: i32, people: &[Uuid]) -> Vec<Uuid> {
        let empty = HashMap::new();
        let by_person = self.by_day.get(&date).unwrap_or(&empty);
        still_to_do(self.mode, target, people, by_person)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn people(n: usize) -> Vec<Uuid> {
        (0..n).map(|i| Uuid::from_u128(i as u128 + 1)).collect()
    }

    #[test]
    fn r5_a_pool_adds_up_whoever_does_it() {
        let [ali, ayse] = [Uuid::from_u128(1), Uuid::from_u128(2)];
        let did = HashMap::from([(ali, 1), (ayse, 2)]);
        assert_eq!(
            effective(CompletionMode::Pool, 3, &[ali, ayse], &did),
            (3, 3)
        );
        // Target 1: one person is enough.
        let one = HashMap::from([(ayse, 1)]);
        assert_eq!(
            effective(CompletionMode::Pool, 1, &[ali, ayse], &one),
            (1, 1)
        );
    }

    #[test]
    fn t3_3_4_reminders_go_to_whoever_still_has_to_do_it() {
        let [ali, ayse] = [Uuid::from_u128(1), Uuid::from_u128(2)];
        let both = [ali, ayse];
        let none = HashMap::new();
        assert_eq!(still_to_do(CompletionMode::Pool, 2, &both, &none), both);
        let half = HashMap::from([(ali, 1)]);
        assert_eq!(still_to_do(CompletionMode::Pool, 2, &both, &half), both);
        let full = HashMap::from([(ali, 1), (ayse, 1)]);
        assert!(still_to_do(CompletionMode::Pool, 2, &both, &full).is_empty());
        // Each person: only those below the target, however much the others did.
        let ali_done = HashMap::from([(ali, 5)]);
        assert_eq!(
            still_to_do(CompletionMode::Individual, 2, &both, &ali_done),
            [ayse]
        );
        assert!(
            still_to_do(
                CompletionMode::Individual,
                2,
                &both,
                &HashMap::from([(ali, 2), (ayse, 2)])
            )
            .is_empty()
        );
    }

    #[test]
    fn r5_each_person_must_reach_the_target() {
        let [ali, ayse, outsider] = [Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(9)];
        let both = [ali, ayse];
        // Ali did 5 of 2, Ayşe 1: his surplus does not cover her.
        let did = HashMap::from([(ali, 5), (ayse, 1), (outsider, 7)]);
        assert_eq!(
            effective(CompletionMode::Individual, 2, &both, &did),
            (3, 4)
        );
        let all = HashMap::from([(ali, 2), (ayse, 2)]);
        let (done, target) = effective(CompletionMode::Individual, 2, &both, &all);
        assert!(done >= target);
    }

    proptest! {
        /// The pool counts everything; each-person mode is complete exactly when everyone is.
        #[test]
        fn modes_agree_with_their_definition(
            target in 1i32..10,
            n in 1usize..5,
            did in prop::collection::vec(0i32..15, 5),
        ) {
            let people = people(n);
            let by_person: HashMap<Uuid, i32> =
                people.iter().zip(&did).map(|(p, d)| (*p, *d)).collect();
            let (done, total) = effective(CompletionMode::Pool, target, &people, &by_person);
            prop_assert_eq!(done, did.iter().take(n).sum::<i32>());
            prop_assert_eq!(total, target);
            let (done, total) = effective(CompletionMode::Individual, target, &people, &by_person);
            let everyone = did.iter().take(n).all(|d| *d >= target);
            prop_assert_eq!(done >= total, everyone);
            prop_assert!(done <= total);
            prop_assert_eq!(total, target * i32::try_from(n).unwrap());
        }

        /// With one person the two modes complete the same days.
        #[test]
        fn one_person_sees_no_difference(target in 1i32..10, did in 0i32..20) {
            let one = people(1);
            let by_person = HashMap::from([(one[0], did)]);
            let pool = effective(CompletionMode::Pool, target, &one, &by_person);
            let each = effective(CompletionMode::Individual, target, &one, &by_person);
            prop_assert_eq!(pool.0 >= pool.1, each.0 >= each.1);
        }
    }
}
