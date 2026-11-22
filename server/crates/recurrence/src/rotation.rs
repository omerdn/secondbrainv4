//! Rotation of a routine's responsible person, and swaps (PRD R6, ARCHITECTURE §8, T1.4.2). Pure.
//!
//! The responsible person is `members[turn mod n]`. The turn counts the occurrences since the
//! start, or the calendar weeks (Monday first) since the start's week. A swap is stored as
//! overrides on the two dates and is applied on top: it never shifts anyone else's turn.

use std::collections::BTreeMap;

use chrono::{Datelike, Days, NaiveDate};

use crate::schedule::{Schedule, ScheduleError};

/// Furthest date from the start a rotation is computed for, in days (about 100 years). Counting
/// occurrences walks the days in between, so this bounds the work of one request.
pub const MAX_ROTATION_DAYS: i64 = 36_600;

/// When the responsible person changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationCadence {
    EveryOccurrence,
    /// Everyone keeps all occurrences of their calendar week.
    Weekly,
}

/// Why a rotation or a swap was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RotationError {
    #[error("a rotation needs at least one member")]
    NoMembers,
    #[error("a person can be in the rotation only once")]
    DuplicateMember,
    #[error("an override names someone outside the rotation ({0})")]
    UnknownMember(NaiveDate),
    #[error("{0} is not an occurrence of this rotation")]
    NotAnOccurrence(NaiveDate),
    #[error("both dates already have the same person")]
    SameAssignee,
    #[error("date is more than {MAX_ROTATION_DAYS} days from the start")]
    TooFarFromStart,
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
}

/// One occurrence and who is responsible for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assignment<T> {
    pub date: NaiveDate,
    pub assignee: T,
}

/// An ordered member list, its cadence and the swaps made so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rotation<T> {
    members: Vec<T>,
    cadence: RotationCadence,
    start: NaiveDate,
    overrides: BTreeMap<NaiveDate, T>,
}

fn monday(date: NaiveDate) -> NaiveDate {
    let since_monday = u64::from(date.weekday().num_days_from_monday());
    date.checked_sub_days(Days::new(since_monday))
        .unwrap_or(NaiveDate::MIN)
}

impl<T: Copy + Eq> Rotation<T> {
    /// A rotation without swaps. The first occurrence on or after `start` is `members[0]`'s.
    ///
    /// # Errors
    /// [`RotationError::NoMembers`] or [`RotationError::DuplicateMember`].
    pub fn new(
        members: Vec<T>,
        cadence: RotationCadence,
        start: NaiveDate,
    ) -> Result<Self, RotationError> {
        if members.is_empty() {
            return Err(RotationError::NoMembers);
        }
        if members
            .iter()
            .enumerate()
            .any(|(i, member)| members[..i].contains(member))
        {
            return Err(RotationError::DuplicateMember);
        }
        Ok(Self {
            members,
            cadence,
            start,
            overrides: BTreeMap::new(),
        })
    }

    /// Restores stored swaps (`routine_rotation_overrides`).
    ///
    /// # Errors
    /// [`RotationError::UnknownMember`] for someone outside the rotation.
    pub fn with_overrides(
        mut self,
        overrides: impl IntoIterator<Item = (NaiveDate, T)>,
    ) -> Result<Self, RotationError> {
        for (date, assignee) in overrides {
            if !self.members.contains(&assignee) {
                return Err(RotationError::UnknownMember(date));
            }
            self.overrides.insert(date, assignee);
        }
        Ok(self)
    }

    /// The swaps to store, by date.
    #[must_use]
    pub const fn overrides(&self) -> &BTreeMap<NaiveDate, T> {
        &self.overrides
    }

    fn check_reach(&self, date: NaiveDate) -> Result<(), RotationError> {
        if (date - self.start).num_days() > MAX_ROTATION_DAYS {
            return Err(RotationError::TooFarFromStart);
        }
        Ok(())
    }

    /// Occurrences from the start up to, but not including, `date`.
    fn occurrences_before(&self, schedule: &Schedule, date: NaiveDate) -> usize {
        self.start
            .iter_days()
            .take_while(|day| *day < date)
            .filter(|day| schedule.occurs_on(*day))
            .count()
    }

    fn member_for_turn(&self, turn: usize) -> T {
        self.members[turn % self.members.len()]
    }

    fn weekly_member(&self, date: NaiveDate) -> T {
        let weeks = (monday(date) - monday(self.start)).num_days() / 7;
        self.member_for_turn(usize::try_from(weeks).unwrap_or(0))
    }

    /// Who the rotation itself gives `date` to, ignoring swaps.
    fn scheduled(&self, schedule: &Schedule, date: NaiveDate) -> Result<T, RotationError> {
        self.check_reach(date)?;
        if date < self.start || !schedule.occurs_on(date) {
            return Err(RotationError::NotAnOccurrence(date));
        }
        Ok(match self.cadence {
            RotationCadence::EveryOccurrence => {
                self.member_for_turn(self.occurrences_before(schedule, date))
            }
            RotationCadence::Weekly => self.weekly_member(date),
        })
    }

    /// Who is responsible for `date`, swaps included.
    ///
    /// # Errors
    /// [`RotationError::NotAnOccurrence`] or [`RotationError::TooFarFromStart`].
    pub fn assignee_on(&self, schedule: &Schedule, date: NaiveDate) -> Result<T, RotationError> {
        let scheduled = self.scheduled(schedule, date)?;
        Ok(self.overrides.get(&date).copied().unwrap_or(scheduled))
    }

    /// The occurrences between `first` and `last` (both included) with their assignees.
    ///
    /// # Errors
    /// [`RotationError::Schedule`] for an invalid range, or [`RotationError::TooFarFromStart`].
    pub fn assignments(
        &self,
        schedule: &Schedule,
        first: NaiveDate,
        last: NaiveDate,
    ) -> Result<Vec<Assignment<T>>, RotationError> {
        let dates = schedule.occurrences(first, last)?;
        self.check_reach(last)?;
        let dates: Vec<NaiveDate> = dates.into_iter().filter(|d| *d >= self.start).collect();
        let earlier = dates
            .first()
            .map_or(0, |date| self.occurrences_before(schedule, *date));
        Ok(dates
            .into_iter()
            .enumerate()
            .map(|(i, date)| {
                let scheduled = match self.cadence {
                    RotationCadence::EveryOccurrence => self.member_for_turn(earlier + i),
                    RotationCadence::Weekly => self.weekly_member(date),
                };
                Assignment {
                    date,
                    assignee: self.overrides.get(&date).copied().unwrap_or(scheduled),
                }
            })
            .collect())
    }

    /// Exchanges the people on two occurrences; everyone else's turns stay. An override that
    /// matches the rotation again is dropped, so swapping back leaves nothing behind.
    ///
    /// # Errors
    /// [`RotationError::NotAnOccurrence`], [`RotationError::TooFarFromStart`], or
    /// [`RotationError::SameAssignee`] if the swap would change nothing.
    pub fn swap(
        &mut self,
        schedule: &Schedule,
        a: NaiveDate,
        b: NaiveDate,
    ) -> Result<(), RotationError> {
        let (on_a, on_b) = (
            self.assignee_on(schedule, a)?,
            self.assignee_on(schedule, b)?,
        );
        if on_a == on_b {
            return Err(RotationError::SameAssignee);
        }
        let (scheduled_a, scheduled_b) =
            (self.scheduled(schedule, a)?, self.scheduled(schedule, b)?);
        self.set_override(a, on_b, scheduled_a);
        self.set_override(b, on_a, scheduled_b);
        Ok(())
    }

    fn set_override(&mut self, date: NaiveDate, assignee: T, scheduled: T) {
        if assignee == scheduled {
            self.overrides.remove(&date);
        } else {
            self.overrides.insert(date, assignee);
        }
    }
}

#[cfg(test)]
mod tests;
