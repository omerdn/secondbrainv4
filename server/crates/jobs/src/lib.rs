//! `jobs`: background job runner and scheduled jobs (ARCHITECTURE §14, ADR-0009).
//!
//! - [`Job`] + [`enqueue`]: put work on the Postgres-backed queue, transactionally.
//! - [`Worker`]: claims due jobs with `FOR UPDATE SKIP LOCKED`, runs them under a lease,
//!   retries failures with exponential backoff and gives up after `max_attempts`.
//! - [`Scheduler`]: cron-style recurring jobs; each occurrence is enqueued exactly once even
//!   with several server instances.
//!
//! Jobs run **at least once**: a job whose worker dies (or exceeds its lease) runs again.
//! Handlers must be idempotent.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::PgConnection;
use uuid::Uuid;

mod schedule;
mod worker;

pub use schedule::{ScheduleError, Scheduler, SchedulerConfig, dedupe_key, next_occurrence};
pub use worker::{ClaimedJob, HandlerError, HandlerResult, RunOutcome, Worker, WorkerConfig};

/// A unit of background work.
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Stable identifier stored with each job, e.g. `email.send`. Never rename.
    const KIND: &'static str;
    /// Executions before the job is marked failed.
    const MAX_ATTEMPTS: u32 = 10;
}

/// Optional enqueue settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnqueueOptions {
    /// Earliest execution time; `None` = now.
    pub run_at: Option<DateTime<Utc>>,
    /// If set, a second job with the same key is not enqueued.
    pub dedupe_key: Option<String>,
}

/// Enqueues `job` using the caller's connection or transaction (so it is only queued if the
/// surrounding change commits). Returns `None` if a job with the same `dedupe_key` exists.
pub async fn enqueue<J: Job>(
    conn: &mut PgConnection,
    job: &J,
    options: EnqueueOptions,
) -> Result<Option<Uuid>, db::DbError> {
    let payload = serde_json::to_value(job).map_err(|err| db::DbError::Encode(Box::new(err)))?;
    let max_attempts = i32::try_from(J::MAX_ATTEMPTS).unwrap_or(i32::MAX);
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO jobs (id, kind, payload, run_at, max_attempts, dedupe_key) \
         VALUES ($1, $2, $3, COALESCE($4, now()), $5, $6) \
         ON CONFLICT (dedupe_key) DO NOTHING RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(J::KIND)
    .bind(payload)
    .bind(options.run_at)
    .bind(max_attempts)
    .bind(options.dedupe_key)
    .fetch_optional(conn)
    .await?;
    Ok(inserted)
}
