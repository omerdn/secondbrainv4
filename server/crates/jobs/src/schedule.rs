//! Cron-style recurring jobs.
//!
//! On every tick the scheduler makes sure the **next** occurrence of each schedule is in the
//! queue, keyed by `cron:<name>:<instant>` (`dedupe_key`). Several instances can tick at the
//! same time; the unique key lets exactly one insert win. When the occurrence runs, the next
//! tick queues the following one.
//!
//! If the server is down across several occurrences, the one already queued runs late and the
//! others are skipped (no burst of catch-up runs).
//!
//! Expressions use 6 fields with seconds, in UTC: `sec min hour day-of-month month day-of-week`,
//! e.g. `0 0 3 * * *` (daily 03:00 UTC). Per-user local times (e.g. routine reminders in the
//! user's time zone) are not cron schedules; they are enqueued as individual jobs with `run_at`.

use std::fmt;
use std::future::Future;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use db::{DbError, PgPool};
use kernel::Clock;

use crate::Job;

type EnqueueFn =
    Box<dyn Fn() -> Result<(String, serde_json::Value, i32), serde_json::Error> + Send + Sync>;

struct ScheduleEntry {
    name: &'static str,
    cron: cron::Schedule,
    job: EnqueueFn,
}

/// Scheduler settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// How often schedules are checked.
    pub tick_interval: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_secs(30),
        }
    }
}

/// Recurring job definitions.
pub struct Scheduler {
    config: SchedulerConfig,
    entries: Vec<ScheduleEntry>,
}

impl fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries: Vec<_> = self
            .entries
            .iter()
            .map(|e| (e.name, e.cron.source().to_owned()))
            .collect();
        f.debug_struct("Scheduler")
            .field("config", &self.config)
            .field("entries", &entries)
            .finish()
    }
}

/// An invalid schedule definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// The cron expression could not be parsed.
    InvalidCron {
        /// Schedule name.
        name: &'static str,
        /// Parser message.
        reason: String,
    },
    /// Two schedules share a name (names are part of the dedupe key).
    DuplicateName(&'static str),
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCron { name, reason } => {
                write!(f, "invalid cron expression for schedule {name}: {reason}")
            }
            Self::DuplicateName(name) => write!(f, "duplicate schedule name {name}"),
        }
    }
}

impl std::error::Error for ScheduleError {}

/// The first occurrence of `cron` strictly after `now` (UTC).
pub fn next_occurrence(cron: &cron::Schedule, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    cron.after(&now).next()
}

/// Queue key of one occurrence of schedule `name`.
pub fn dedupe_key(name: &str, at: DateTime<Utc>) -> String {
    format!(
        "cron:{name}:{}",
        at.to_rfc3339_opts(SecondsFormat::Secs, true)
    )
}

impl Scheduler {
    /// A scheduler without schedules.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            entries: Vec::new(),
        }
    }

    /// Adds a recurring job: `job` is enqueued at every occurrence of `cron` (see module docs).
    pub fn add<J>(
        &mut self,
        name: &'static str,
        cron: &str,
        job: J,
    ) -> Result<&mut Self, ScheduleError>
    where
        J: Job + Clone,
    {
        if self.entries.iter().any(|e| e.name == name) {
            return Err(ScheduleError::DuplicateName(name));
        }
        let cron = cron::Schedule::from_str(cron).map_err(|err| ScheduleError::InvalidCron {
            name,
            reason: err.to_string(),
        })?;
        let max_attempts = i32::try_from(J::MAX_ATTEMPTS).unwrap_or(i32::MAX);
        self.entries.push(ScheduleEntry {
            name,
            cron,
            job: Box::new(move || {
                Ok((
                    J::KIND.to_owned(),
                    serde_json::to_value(&job)?,
                    max_attempts,
                ))
            }),
        });
        Ok(self)
    }

    /// Ensures the next occurrence of every schedule is queued. Returns how many occurrences
    /// this call inserted (0 if another instance or an earlier tick already did).
    pub async fn tick(&self, pool: &PgPool, clock: &dyn Clock) -> Result<usize, DbError> {
        let now = clock.now();
        let mut inserted = 0;
        for entry in &self.entries {
            let Some(at) = next_occurrence(&entry.cron, now) else {
                continue;
            };
            let (kind, payload, max_attempts) =
                (entry.job)().map_err(|err| DbError::Encode(Box::new(err)))?;
            let result = sqlx::query(
                "INSERT INTO jobs (id, kind, payload, run_at, max_attempts, dedupe_key) \
                 VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (dedupe_key) DO NOTHING",
            )
            .bind(uuid::Uuid::now_v7())
            .bind(kind)
            .bind(payload)
            .bind(at)
            .bind(max_attempts)
            .bind(dedupe_key(entry.name, at))
            .execute(pool)
            .await?;
            inserted += usize::try_from(result.rows_affected()).unwrap_or(0);
        }
        Ok(inserted)
    }

    /// Ticks every `tick_interval` until `shutdown` resolves.
    pub async fn run<S>(&self, pool: &PgPool, clock: &dyn Clock, shutdown: S)
    where
        S: Future<Output = ()>,
    {
        tokio::pin!(shutdown);
        loop {
            if let Err(err) = self.tick(pool, clock).await {
                tracing::error!(
                    error = %kernel::redact::redact(&err.to_string()),
                    "scheduler tick failed"
                );
            }
            tokio::select! {
                () = &mut shutdown => break,
                () = tokio::time::sleep(self.config.tick_interval) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, TimeZone, Timelike};
    use proptest::prelude::*;

    use super::*;

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct Nightly;
    impl Job for Nightly {
        const KIND: &'static str = "test.nightly";
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    #[test]
    fn next_occurrence_is_strictly_after_now() {
        let daily_3am = cron::Schedule::from_str("0 0 3 * * *").unwrap();
        assert_eq!(
            next_occurrence(&daily_3am, utc(2026, 11, 21, 2, 59, 59)),
            Some(utc(2026, 11, 21, 3, 0, 0))
        );
        // Exactly at the occurrence: the next one is tomorrow.
        assert_eq!(
            next_occurrence(&daily_3am, utc(2026, 11, 21, 3, 0, 0)),
            Some(utc(2026, 11, 22, 3, 0, 0))
        );
    }

    #[test]
    fn weekly_schedule_on_monday() {
        let weekly = cron::Schedule::from_str("0 0 8 * * Mon").unwrap();
        // 2026-11-21 is a Saturday.
        let next = next_occurrence(&weekly, utc(2026, 11, 21, 12, 0, 0)).unwrap();
        assert_eq!(next, utc(2026, 11, 23, 8, 0, 0));
        assert_eq!(next.weekday(), chrono::Weekday::Mon);
    }

    #[test]
    fn dedupe_key_is_stable_and_second_precise() {
        assert_eq!(
            dedupe_key("weekly-summary", utc(2026, 11, 23, 8, 0, 0)),
            "cron:weekly-summary:2026-11-23T08:00:00Z"
        );
    }

    #[test]
    fn rejects_invalid_and_duplicate_schedules() {
        let mut scheduler = Scheduler::new(SchedulerConfig::default());
        assert!(matches!(
            scheduler.add("bad", "every day", Nightly),
            Err(ScheduleError::InvalidCron { name: "bad", .. })
        ));
        scheduler.add("nightly", "0 0 3 * * *", Nightly).unwrap();
        assert_eq!(
            scheduler
                .add("nightly", "0 0 4 * * *", Nightly)
                .unwrap_err(),
            ScheduleError::DuplicateName("nightly")
        );
    }

    proptest! {
        #[test]
        fn hourly_next_occurrence_matches_the_expression(secs in 0i64..4_000_000_000) {
            let hourly = cron::Schedule::from_str("0 15 * * * *").unwrap();
            let now = Utc.timestamp_opt(secs, 0).unwrap();
            let next = next_occurrence(&hourly, now).unwrap();
            prop_assert!(next > now);
            prop_assert!(next - now <= chrono::Duration::hours(1));
            prop_assert_eq!((next.minute(), next.second()), (15, 0));
            prop_assert!(hourly.includes(next));
        }
    }
}
