//! Injectable time source (CLAUDE.md: "`Clock` dışarıdan enjekte edilir").
//!
//! Domain logic never calls `Utc::now()` directly; it receives a [`Clock`]. Production uses
//! [`SystemClock`], tests use [`FixedClock`].

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;

/// A source of the current time.
pub trait Clock: Send + Sync + fmt::Debug {
    /// The current instant in UTC.
    fn now(&self) -> DateTime<Utc>;

    /// The current local date in `tz`. "Today" for a routine is evaluated in the routine's
    /// time zone (PRD R2, R7).
    fn today_in(&self, tz: Tz) -> NaiveDate {
        self.now().with_timezone(&tz).date_naive()
    }
}

/// Shared, dynamically dispatched clock for application state.
pub type SharedClock = Arc<dyn Clock>;

/// The real wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that only moves when told to. Clones share the same time, so a test can hand a clone
/// to the code under test and advance it afterwards.
#[derive(Debug, Clone)]
pub struct FixedClock {
    now: Arc<Mutex<DateTime<Utc>>>,
}

impl FixedClock {
    /// A clock frozen at `at`.
    pub fn new(at: DateTime<Utc>) -> Self {
        Self {
            now: Arc::new(Mutex::new(at)),
        }
    }

    /// Moves the clock to `at` (may go backwards).
    pub fn set(&self, at: DateTime<Utc>) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = at;
    }

    /// Moves the clock forward by `by` (negative durations move it back).
    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now += by;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn system_clock_tracks_wall_time() {
        let before = Utc::now();
        let now = SystemClock.now();
        assert!(now >= before);
        assert!(now - before < Duration::seconds(5));
    }

    #[test]
    fn fixed_clock_is_frozen_until_moved() {
        let clock = FixedClock::new(utc(2026, 11, 21, 12, 0));
        assert_eq!(clock.now(), utc(2026, 11, 21, 12, 0));
        assert_eq!(clock.now(), utc(2026, 11, 21, 12, 0));

        clock.advance(Duration::minutes(90));
        assert_eq!(clock.now(), utc(2026, 11, 21, 13, 30));

        clock.set(utc(2020, 1, 1, 0, 0));
        assert_eq!(clock.now(), utc(2020, 1, 1, 0, 0));
    }

    #[test]
    fn fixed_clock_clones_share_time() {
        let clock = FixedClock::new(utc(2026, 11, 21, 12, 0));
        let shared: SharedClock = Arc::new(clock.clone());
        clock.advance(Duration::days(1));
        assert_eq!(shared.now(), utc(2026, 11, 22, 12, 0));
    }

    #[test]
    fn today_depends_on_time_zone() {
        // 22:30 UTC on Nov 21 is already Nov 22 in Istanbul (UTC+3) but still Nov 21 in New York.
        let clock = FixedClock::new(utc(2026, 11, 21, 22, 30));
        assert_eq!(
            clock.today_in(chrono_tz::Europe::Istanbul),
            date(2026, 11, 22)
        );
        assert_eq!(
            clock.today_in(chrono_tz::America::New_York),
            date(2026, 11, 21)
        );
        assert_eq!(clock.today_in(chrono_tz::UTC), date(2026, 11, 21));
    }

    #[test]
    fn today_handles_dst_transitions() {
        // Berlin switches to CEST at 01:00 UTC on 2026-03-29; 23:30 UTC the day before is
        // 00:30 CET on the 29th.
        let clock = FixedClock::new(utc(2026, 3, 28, 23, 30));
        assert_eq!(clock.today_in(chrono_tz::Europe::Berlin), date(2026, 3, 29));
    }
}
