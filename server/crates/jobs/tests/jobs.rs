//! Job queue, worker and scheduler against a real Postgres (T0.2.6).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use jobs::{
    EnqueueOptions, Job, RunOutcome, Scheduler, SchedulerConfig, Worker, WorkerConfig, enqueue,
};
use kernel::FixedClock;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SendDigest {
    user: String,
}

impl Job for SendDigest {
    const KIND: &'static str = "test.send_digest";
    const MAX_ATTEMPTS: u32 = 3;
}

fn config() -> WorkerConfig {
    WorkerConfig {
        batch_size: 10,
        lease: Duration::from_secs(30),
        poll_interval: Duration::from_millis(20),
        retry_base: Duration::ZERO,
        retry_max: Duration::ZERO,
    }
}

#[derive(Clone, Default)]
struct Runs(Arc<Mutex<HashMap<Uuid, usize>>>);

impl Runs {
    fn record(&self, id: Uuid) {
        *self.0.lock().unwrap().entry(id).or_default() += 1;
    }
    fn count(&self, id: Uuid) -> usize {
        self.0.lock().unwrap().get(&id).copied().unwrap_or(0)
    }
    fn total(&self) -> usize {
        self.0.lock().unwrap().values().sum()
    }
}

fn counting_worker(runs: &Runs, config: WorkerConfig) -> Worker {
    let mut worker = Worker::new(config);
    let runs = runs.clone();
    worker.register(move |_: SendDigest, job| {
        let runs = runs.clone();
        async move {
            runs.record(job.id);
            Ok(())
        }
    });
    worker
}

async fn enqueue_now(db: &TestDb, user: &str) -> Uuid {
    let mut conn = db.pool().acquire().await.unwrap();
    enqueue(
        &mut conn,
        &SendDigest { user: user.into() },
        EnqueueOptions::default(),
    )
    .await
    .unwrap()
    .unwrap()
}

/// (completed, failed, attempts, last_error)
async fn state(db: &TestDb, id: Uuid) -> (bool, bool, i32, Option<String>) {
    sqlx::query_as(
        "SELECT completed_at IS NOT NULL, failed_at IS NOT NULL, attempts, last_error \
         FROM jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn enqueue_follows_the_surrounding_transaction() {
    let db = TestDb::new().await;
    let mut tx = db.pool().begin().await.unwrap();
    enqueue(
        &mut tx,
        &SendDigest { user: "a".into() },
        EnqueueOptions::default(),
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn runs_job_with_payload_once_and_marks_completed() {
    let db = TestDb::new().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut worker = Worker::new(config());
    let sink = Arc::clone(&seen);
    worker.register(move |job: SendDigest, _| {
        let sink = Arc::clone(&sink);
        async move {
            sink.lock().unwrap().push(job.user);
            Ok(())
        }
    });

    let id = enqueue_now(&db, "ayşe").await;
    let outcome = worker.run_batch(db.pool()).await.unwrap();

    assert_eq!(
        outcome,
        RunOutcome {
            completed: 1,
            ..RunOutcome::default()
        }
    );
    assert_eq!(*seen.lock().unwrap(), ["ayşe"]);
    assert_eq!(state(&db, id).await, (true, false, 1, None));
    assert_eq!(worker.run_batch(db.pool()).await.unwrap().claimed(), 0);
}

#[tokio::test]
async fn future_jobs_wait_until_due() {
    let db = TestDb::new().await;
    let mut conn = db.pool().acquire().await.unwrap();
    enqueue(
        &mut conn,
        &SendDigest { user: "a".into() },
        EnqueueOptions {
            run_at: Some(Utc::now() + chrono::Duration::hours(1)),
            dedupe_key: None,
        },
    )
    .await
    .unwrap();

    let runs = Runs::default();
    assert_eq!(
        counting_worker(&runs, config())
            .run_batch(db.pool())
            .await
            .unwrap()
            .claimed(),
        0
    );
}

#[tokio::test]
async fn dedupe_key_prevents_duplicates() {
    let db = TestDb::new().await;
    let mut conn = db.pool().acquire().await.unwrap();
    let options = EnqueueOptions {
        run_at: None,
        dedupe_key: Some("digest:ayse:2026-W47".into()),
    };
    let job = SendDigest { user: "a".into() };
    assert!(
        enqueue(&mut conn, &job, options.clone())
            .await
            .unwrap()
            .is_some()
    );
    assert!(enqueue(&mut conn, &job, options).await.unwrap().is_none());
}

#[tokio::test]
async fn failures_are_retried_then_marked_failed_after_max_attempts() {
    let db = TestDb::new().await;
    let mut worker = Worker::new(config());
    worker.register(|_: SendDigest, _| async { Err("smtp refused ali@example.com".into()) });
    let id = enqueue_now(&db, "a").await;

    let first = worker.run_batch(db.pool()).await.unwrap();
    assert_eq!(first.retried, 1);
    let (completed, failed, attempts, error) = state(&db, id).await;
    assert_eq!((completed, failed, attempts), (false, false, 1));
    assert_eq!(error.as_deref(), Some("smtp refused <email>"));

    assert_eq!(worker.run_batch(db.pool()).await.unwrap().retried, 1);
    // MAX_ATTEMPTS = 3: the third failure is final.
    assert_eq!(worker.run_batch(db.pool()).await.unwrap().failed, 1);
    assert!(state(&db, id).await.1, "marked failed");
    assert_eq!(worker.run_batch(db.pool()).await.unwrap().claimed(), 0);
}

#[tokio::test]
async fn backoff_delays_retries() {
    let db = TestDb::new().await;
    let mut worker = Worker::new(WorkerConfig {
        retry_base: Duration::from_secs(600),
        retry_max: Duration::from_secs(600),
        ..config()
    });
    worker.register(|_: SendDigest, _| async { Err("down".into()) });
    let id = enqueue_now(&db, "a").await;

    assert_eq!(worker.run_batch(db.pool()).await.unwrap().retried, 1);
    let delay: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM run_at - now())::float8 FROM jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert!((590.0..=600.0).contains(&delay), "{delay}");
    assert_eq!(worker.run_batch(db.pool()).await.unwrap().claimed(), 0);
}

#[tokio::test]
async fn handler_exceeding_the_lease_is_cancelled_and_retried() {
    let db = TestDb::new().await;
    let mut worker = Worker::new(WorkerConfig {
        lease: Duration::from_millis(100),
        ..config()
    });
    worker.register(|_: SendDigest, _| async {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(())
    });
    let id = enqueue_now(&db, "a").await;

    let outcome = tokio::time::timeout(Duration::from_secs(5), worker.run_batch(db.pool()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outcome.retried, 1);
    assert!(state(&db, id).await.3.unwrap().contains("timed out"));
}

/// A worker that dies mid-job: once its lease expires another worker runs the job, and the dead
/// worker can no longer record an outcome.
#[tokio::test]
async fn expired_lease_lets_another_worker_take_over() {
    let db = TestDb::new().await;
    let id = enqueue_now(&db, "a").await;

    let runs = Runs::default();
    let started = Arc::new(Notify::new());
    let mut stuck = Worker::new(WorkerConfig {
        lease: Duration::from_millis(300),
        ..config()
    });
    let (sink, signal) = (runs.clone(), Arc::clone(&started));
    stuck.register(move |_: SendDigest, job| {
        let (sink, signal) = (sink.clone(), Arc::clone(&signal));
        async move {
            sink.record(job.id);
            signal.notify_one();
            std::future::pending::<()>().await;
            Ok(())
        }
    });

    let pool = db.pool().clone();
    let crashed = tokio::spawn(async move { stuck.run_batch(&pool).await });
    started.notified().await;
    crashed.abort(); // process "dies" holding the lease
    let _ = crashed.await;

    let healthy = counting_worker(&runs, config());
    assert_eq!(
        healthy.run_batch(db.pool()).await.unwrap().claimed(),
        0,
        "lease still held"
    );

    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(healthy.run_batch(db.pool()).await.unwrap().completed, 1);
    assert_eq!(runs.count(id), 2, "ran again after the lease expired");
    assert_eq!(state(&db, id).await, (true, false, 2, None));
}

#[tokio::test]
async fn workers_only_claim_kinds_they_handle() {
    #[derive(Serialize, Deserialize)]
    struct Other;
    impl Job for Other {
        const KIND: &'static str = "test.other";
    }

    let db = TestDb::new().await;
    let mut conn = db.pool().acquire().await.unwrap();
    enqueue(&mut conn, &Other, EnqueueOptions::default())
        .await
        .unwrap();

    let runs = Runs::default();
    assert_eq!(
        counting_worker(&runs, config())
            .run_batch(db.pool())
            .await
            .unwrap()
            .claimed(),
        0
    );
    let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM jobs")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 0, "untouched");
}

#[tokio::test]
async fn concurrent_workers_run_each_job_once() {
    let db = TestDb::new().await;
    let mut ids = Vec::new();
    for i in 0..30 {
        ids.push(enqueue_now(&db, &format!("u{i}")).await);
    }

    let runs = Runs::default();
    let mut tasks = Vec::new();
    for _ in 0..3 {
        let mut worker = Worker::new(WorkerConfig {
            batch_size: 4,
            ..config()
        });
        let sink = runs.clone();
        worker.register(move |_: SendDigest, job| {
            let sink = sink.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(5)).await;
                sink.record(job.id);
                Ok(())
            }
        });
        let pool = db.pool().clone();
        tasks.push(tokio::spawn(async move {
            while worker.run_batch(&pool).await.unwrap().claimed() > 0 {}
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }

    assert_eq!(runs.total(), 30);
    assert!(ids.iter().all(|id| runs.count(*id) == 1));
}

#[tokio::test]
async fn scheduler_enqueues_each_occurrence_exactly_once() {
    let db = TestDb::new().await;
    let mut scheduler = Scheduler::new(SchedulerConfig::default());
    scheduler
        .add("digest", "0 0 8 * * *", SendDigest { user: "all".into() })
        .unwrap();
    let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 21, 7, 59, 0).unwrap());

    assert_eq!(scheduler.tick(db.pool(), &clock).await.unwrap(), 1);
    assert_eq!(
        scheduler.tick(db.pool(), &clock).await.unwrap(),
        0,
        "already queued"
    );

    // A second instance ticking at the same time does not duplicate it.
    let mut other = Scheduler::new(SchedulerConfig::default());
    other
        .add("digest", "0 0 8 * * *", SendDigest { user: "all".into() })
        .unwrap();
    assert_eq!(other.tick(db.pool(), &clock).await.unwrap(), 0);

    // After 08:00 the next day's occurrence is queued.
    clock.advance(chrono::Duration::minutes(2));
    assert_eq!(scheduler.tick(db.pool(), &clock).await.unwrap(), 1);

    let rows: Vec<(String, chrono::DateTime<Utc>)> =
        sqlx::query_as("SELECT dedupe_key, run_at FROM jobs ORDER BY run_at")
            .fetch_all(db.pool())
            .await
            .unwrap();
    assert_eq!(
        rows.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(),
        [
            "cron:digest:2026-11-21T08:00:00Z",
            "cron:digest:2026-11-22T08:00:00Z"
        ]
    );
}

#[tokio::test]
async fn scheduled_job_is_run_by_the_worker() {
    let db = TestDb::new().await;
    let mut scheduler = Scheduler::new(SchedulerConfig::default());
    scheduler
        .add("digest", "* * * * * *", SendDigest { user: "all".into() })
        .unwrap();
    // Queue an occurrence one second from now (real time, as the worker uses the DB clock).
    let clock = FixedClock::new(Utc::now());
    assert_eq!(scheduler.tick(db.pool(), &clock).await.unwrap(), 1);

    tokio::time::sleep(Duration::from_millis(1100)).await;
    let runs = Runs::default();
    assert_eq!(
        counting_worker(&runs, config())
            .run_batch(db.pool())
            .await
            .unwrap()
            .completed,
        1
    );
}

#[tokio::test]
async fn run_loop_processes_jobs_until_shutdown() {
    let db = TestDb::new().await;
    let done = Arc::new(Notify::new());
    let mut worker = Worker::new(config());
    let signal = Arc::clone(&done);
    worker.register(move |_: SendDigest, _| {
        let signal = Arc::clone(&signal);
        async move {
            signal.notify_one();
            Ok(())
        }
    });

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let pool = db.pool().clone();
    let runner = tokio::spawn(async move {
        worker
            .run(&pool, async {
                let _ = stopped.await;
            })
            .await;
    });

    let id = enqueue_now(&db, "live").await;
    tokio::time::timeout(Duration::from_secs(5), done.notified())
        .await
        .expect("job run by the loop");
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), runner)
        .await
        .unwrap()
        .unwrap();
    // The outcome is recorded right after the handler returns.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(state(&db, id).await.0);
}
