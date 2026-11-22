//! Plan versions and occurrence generation in the routine's time zone (PRD R2, R11, ADR-0008).

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;

use crate::rule::RuleSet;

/// Longest range [`Schedule::occurrences`] accepts, in days (about ten years).
pub const MAX_RANGE_DAYS: i64 = 3_660;

/// Why a schedule or a query was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("a plan version ends before it starts ({valid_from} > {valid_to})")]
    InvertedVersion {
        valid_from: NaiveDate,
        valid_to: NaiveDate,
    },
    #[error("plan versions starting on {0} and {1} overlap")]
    Overlap(NaiveDate, NaiveDate),
    #[error("a plan change must not start before the current version ({0})")]
    ChangeBeforeCurrentVersion(NaiveDate),
    #[error("a plan change must start on or before the routine's last day ({0})")]
    ChangeAfterEnd(NaiveDate),
    #[error("range ends before it starts")]
    InvertedRange,
    #[error("range is longer than {MAX_RANGE_DAYS} days")]
    RangeTooLong,
}

/// One row of `routine_schedules`: the rules in force from `valid_from` to `valid_to`, both
/// inclusive local dates. `valid_to = None` means open-ended. `valid_from` is also the rules'
/// start (DTSTART): it anchors intervals and defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanVersion {
    pub rules: RuleSet,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
}

impl PlanVersion {
    fn contains(&self, date: NaiveDate) -> bool {
        date >= self.valid_from && self.valid_to.is_none_or(|end| date <= end)
    }
}

/// A routine's plan history and time zone. Occurrences are local dates in that zone; they are
/// never stored, only computed (ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    timezone: Tz,
    versions: Vec<PlanVersion>,
}

/// The local date of `instant` in `timezone`: the day a completion at that instant counts for (R2).
#[must_use]
pub fn local_date(instant: DateTime<Utc>, timezone: Tz) -> NaiveDate {
    instant.with_timezone(&timezone).date_naive()
}

impl Schedule {
    /// Versions may come in any order; they must not overlap. Gaps (a paused routine) are allowed.
    ///
    /// # Errors
    /// [`ScheduleError::InvertedVersion`] or [`ScheduleError::Overlap`].
    pub fn new(timezone: Tz, mut versions: Vec<PlanVersion>) -> Result<Self, ScheduleError> {
        versions.sort_by_key(|v| v.valid_from);
        for v in &versions {
            if let Some(end) = v.valid_to.filter(|end| *end < v.valid_from) {
                return Err(ScheduleError::InvertedVersion {
                    valid_from: v.valid_from,
                    valid_to: end,
                });
            }
        }
        for pair in versions.windows(2) {
            if let [a, b] = pair
                && a.valid_to.is_none_or(|end| end >= b.valid_from)
            {
                return Err(ScheduleError::Overlap(a.valid_from, b.valid_from));
            }
        }
        Ok(Self { timezone, versions })
    }

    #[must_use]
    pub const fn timezone(&self) -> Tz {
        self.timezone
    }

    /// Versions ordered by `valid_from`.
    #[must_use]
    pub fn versions(&self) -> &[PlanVersion] {
        &self.versions
    }

    /// Changes the plan from `from` on (R11): the current (last) version ends the day before and a
    /// new version with `rules` starts on `from`, keeping the routine's end date. Earlier days keep
    /// their old rules, so past occurrences never change. A change on the current version's first
    /// day replaces it. Whether `from` may lie in the past is the caller's decision.
    ///
    /// # Errors
    /// [`ScheduleError::ChangeBeforeCurrentVersion`] if `from` is before the current version, and
    /// [`ScheduleError::ChangeAfterEnd`] if it is after the routine's last day.
    pub fn change_plan(&mut self, rules: RuleSet, from: NaiveDate) -> Result<(), ScheduleError> {
        let mut valid_to = None;
        if let Some(current) = self.versions.last_mut() {
            if from < current.valid_from {
                return Err(ScheduleError::ChangeBeforeCurrentVersion(
                    current.valid_from,
                ));
            }
            if let Some(end) = current.valid_to.filter(|end| from > *end) {
                return Err(ScheduleError::ChangeAfterEnd(end));
            }
            valid_to = current.valid_to;
            match from.pred_opt().filter(|_| from > current.valid_from) {
                Some(day_before) => current.valid_to = Some(day_before),
                None => {
                    self.versions.pop();
                }
            }
        }
        self.versions.push(PlanVersion {
            rules,
            valid_from: from,
            valid_to,
        });
        Ok(())
    }

    /// Whether there is an occurrence on the local date `date`.
    #[must_use]
    pub fn occurs_on(&self, date: NaiveDate) -> bool {
        self.versions
            .iter()
            .find(|v| v.contains(date))
            .is_some_and(|v| v.rules.matches(v.valid_from, date))
    }

    /// Occurrences between the local dates `first` and `last`, both inclusive, in order.
    ///
    /// # Errors
    /// [`ScheduleError::InvertedRange`] or [`ScheduleError::RangeTooLong`].
    pub fn occurrences(
        &self,
        first: NaiveDate,
        last: NaiveDate,
    ) -> Result<Vec<NaiveDate>, ScheduleError> {
        let days = (last - first).num_days();
        if days < 0 {
            return Err(ScheduleError::InvertedRange);
        }
        if days >= MAX_RANGE_DAYS {
            return Err(ScheduleError::RangeTooLong);
        }
        Ok(first
            .iter_days()
            .take_while(|d| *d <= last)
            .filter(|d| self.occurs_on(*d))
            .collect())
    }

    /// Occurrences on the local days (in the routine's zone) touched by the instants `from` to
    /// `to`. Days are calendar dates, so DST transitions never duplicate or drop a day.
    ///
    /// # Errors
    /// As [`Schedule::occurrences`].
    pub fn occurrences_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<NaiveDate>, ScheduleError> {
        self.occurrences(
            local_date(from, self.timezone),
            local_date(to, self.timezone),
        )
    }

    /// The routine's current local date.
    #[must_use]
    pub fn today(&self, now: DateTime<Utc>) -> NaiveDate {
        local_date(now, self.timezone)
    }
}

#[cfg(test)]
mod tests;
