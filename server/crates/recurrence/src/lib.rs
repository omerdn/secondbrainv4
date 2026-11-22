//! `recurrence`: RRULE recurrence generation and schedule versions; pure library (ADR-0008).
//!
//! - [`Rule`] / [`RuleSet`]: the supported RRULE subset (PRD R1), parsed from and printed to the
//!   text stored in `routine_schedules.rrule`.
//! - [`Schedule`]: a routine's plan versions (R11) and time zone (R2); computes occurrences as local
//!   dates. Occurrences are never stored.
//!
//! No I/O and no clock: callers pass "now" (from `kernel::Clock`) where needed.

mod rule;
mod schedule;

pub use rule::{Frequency, Rule, RuleError, RuleSet};
pub use schedule::{MAX_RANGE_DAYS, PlanVersion, Schedule, ScheduleError, local_date};
