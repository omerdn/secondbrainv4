//! Job worker: claim under a lease, run, record the outcome.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use db::{DbError, PgPool};
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::Job;

/// Error returned by a job handler.
pub type HandlerError = Box<dyn std::error::Error + Send + Sync>;
/// Result of a job handler.
pub type HandlerResult = Result<(), HandlerError>;

type BoxFuture = Pin<Box<dyn Future<Output = HandlerResult> + Send>>;
type Handler = Arc<dyn Fn(ClaimedJob) -> BoxFuture + Send + Sync>;

/// A job claimed by this worker.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClaimedJob {
    /// Job id (stable across retries; use it for idempotency).
    pub id: Uuid,
    /// [`Job::KIND`].
    pub kind: String,
    /// JSON payload.
    pub payload: serde_json::Value,
    /// Executions so far, including this one.
    pub attempts: i32,
    /// Executions allowed in total.
    pub max_attempts: i32,
    /// Lease token; the outcome is only recorded while this token still holds the lease.
    pub locked_by: Uuid,
}

/// Worker settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    /// Maximum jobs claimed (and run concurrently) per batch.
    pub batch_size: i64,
    /// How long a claimed job is reserved. A handler that runs longer is cancelled and the job
    /// becomes available again.
    pub lease: Duration,
    /// Sleep between polls when nothing is due.
    pub poll_interval: Duration,
    /// Backoff after the first failure; doubles per attempt.
    pub retry_base: Duration,
    /// Upper bound for the backoff.
    pub retry_max: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            lease: Duration::from_secs(5 * 60),
            poll_interval: Duration::from_secs(1),
            retry_base: Duration::from_secs(5),
            retry_max: Duration::from_secs(60 * 60),
        }
    }
}

/// Counts from one [`Worker::run_batch`] call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunOutcome {
    /// Jobs that succeeded.
    pub completed: usize,
    /// Jobs that failed and were rescheduled.
    pub retried: usize,
    /// Jobs that failed for the last time (`max_attempts` reached).
    pub failed: usize,
    /// Outcomes not recorded because the lease had been lost in the meantime.
    pub lease_lost: usize,
}

impl RunOutcome {
    /// Number of jobs claimed.
    pub fn claimed(&self) -> usize {
        self.completed + self.retried + self.failed + self.lease_lost
    }
}

/// Runs jobs of registered kinds.
#[derive(Clone)]
pub struct Worker {
    config: WorkerConfig,
    handlers: HashMap<&'static str, Handler>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut kinds: Vec<_> = self.handlers.keys().copied().collect();
        kinds.sort_unstable();
        f.debug_struct("Worker")
            .field("config", &self.config)
            .field("kinds", &kinds)
            .finish()
    }
}

enum Outcome {
    Completed,
    Retried,
    Failed,
    LeaseLost,
}

impl Worker {
    /// A worker without handlers.
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            config,
            handlers: HashMap::new(),
        }
    }

    /// Registers the handler for jobs of type `J` (one handler per kind).
    pub fn register<J, F, Fut>(&mut self, handler: F) -> &mut Self
    where
        J: Job,
        F: Fn(J, ClaimedJob) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        let handler = Arc::new(handler);
        let erased: Handler = Arc::new(move |claimed: ClaimedJob| {
            let handler = Arc::clone(&handler);
            Box::pin(async move {
                let job: J = serde_json::from_value(claimed.payload.clone())?;
                handler(job, claimed).await
            })
        });
        self.handlers.insert(J::KIND, erased);
        self
    }

    /// Claims due jobs (only kinds this worker can run) under a fresh lease.
    pub async fn claim(&self, pool: &PgPool) -> Result<Vec<ClaimedJob>, DbError> {
        let kinds: Vec<&str> = self.handlers.keys().copied().collect();
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let lease_ms = i64::try_from(self.config.lease.as_millis()).unwrap_or(i64::MAX);
        sqlx::query_as(
            "UPDATE jobs SET attempts = attempts + 1, locked_by = $1, \
                 locked_until = now() + ($2::bigint * interval '1 millisecond') \
             WHERE id IN ( \
                 SELECT id FROM jobs \
                 WHERE completed_at IS NULL AND failed_at IS NULL AND run_at <= now() \
                   AND (locked_until IS NULL OR locked_until < now()) \
                   AND kind = ANY($3) \
                 ORDER BY run_at, id LIMIT $4 FOR UPDATE SKIP LOCKED) \
             RETURNING id, kind, payload, attempts, max_attempts, locked_by",
        )
        .bind(Uuid::now_v7())
        .bind(lease_ms)
        .bind(&kinds)
        .bind(self.config.batch_size)
        .fetch_all(pool)
        .await
    }

    /// Claims a batch, runs the jobs concurrently and records each outcome.
    pub async fn run_batch(&self, pool: &PgPool) -> Result<RunOutcome, DbError> {
        let claimed = self.claim(pool).await?;
        let mut tasks = JoinSet::new();
        for job in claimed {
            let handler = self.handlers.get(job.kind.as_str()).cloned();
            let (pool, config) = (pool.clone(), self.config.clone());
            tasks.spawn(async move { execute(&pool, &config, handler, job).await });
        }

        let mut outcome = RunOutcome::default();
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok(Ok(Outcome::Completed)) => outcome.completed += 1,
                Ok(Ok(Outcome::Retried)) => outcome.retried += 1,
                Ok(Ok(Outcome::Failed)) => outcome.failed += 1,
                Ok(Ok(Outcome::LeaseLost)) => outcome.lease_lost += 1,
                Ok(Err(err)) => return Err(err),
                // A panicking handler: the job stays leased and runs again after the lease.
                Err(_) => outcome.lease_lost += 1,
            }
        }
        Ok(outcome)
    }

    /// Runs batches until `shutdown` resolves.
    pub async fn run<S>(&self, pool: &PgPool, shutdown: S)
    where
        S: Future<Output = ()>,
    {
        tokio::pin!(shutdown);
        let full_batch = usize::try_from(self.config.batch_size).unwrap_or(usize::MAX);
        loop {
            let claimed = match self.run_batch(pool).await {
                Ok(outcome) => outcome.claimed(),
                Err(err) => {
                    tracing::error!(
                        error = %kernel::redact::redact(&err.to_string()),
                        "job batch failed"
                    );
                    0
                }
            };
            let pause = if claimed >= full_batch {
                Duration::ZERO
            } else {
                self.config.poll_interval
            };
            tokio::select! {
                () = &mut shutdown => break,
                () = tokio::time::sleep(pause) => {}
            }
        }
    }
}

async fn execute(
    pool: &PgPool,
    config: &WorkerConfig,
    handler: Option<Handler>,
    job: ClaimedJob,
) -> Result<Outcome, DbError> {
    let result = match handler {
        None => Err(format!("no handler for job kind {}", job.kind)),
        Some(handler) => match tokio::time::timeout(config.lease, handler(job.clone())).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err.to_string()),
            Err(_) => Err(format!("timed out after {:?} (lease)", config.lease)),
        },
    };

    match result {
        Ok(()) => {
            let rows = sqlx::query(
                "UPDATE jobs SET completed_at = now(), locked_until = NULL, locked_by = NULL, \
                     last_error = NULL \
                 WHERE id = $1 AND locked_by = $2",
            )
            .bind(job.id)
            .bind(job.locked_by)
            .execute(pool)
            .await?
            .rows_affected();
            Ok(if rows == 0 {
                Outcome::LeaseLost
            } else {
                Outcome::Completed
            })
        }
        Err(error) => {
            let error = kernel::redact::redact(&error);
            let attempt = u32::try_from(job.attempts).unwrap_or(1);
            let delay = kernel::retry::backoff(attempt, config.retry_base, config.retry_max);
            let delay_ms = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
            let final_attempt = job.attempts >= job.max_attempts;
            tracing::warn!(
                job_id = %job.id,
                kind = %job.kind,
                attempt,
                final_attempt,
                error = %error,
                "job failed"
            );
            let rows = sqlx::query(
                "UPDATE jobs SET locked_until = NULL, locked_by = NULL, last_error = $3, \
                     failed_at = CASE WHEN $4 THEN now() END, \
                     run_at = CASE WHEN $4 THEN run_at \
                                   ELSE now() + ($5::bigint * interval '1 millisecond') END \
                 WHERE id = $1 AND locked_by = $2",
            )
            .bind(job.id)
            .bind(job.locked_by)
            .bind(&error)
            .bind(final_attempt)
            .bind(delay_ms)
            .execute(pool)
            .await?
            .rows_affected();
            Ok(match (rows, final_attempt) {
                (0, _) => Outcome::LeaseLost,
                (_, true) => Outcome::Failed,
                (_, false) => Outcome::Retried,
            })
        }
    }
}
