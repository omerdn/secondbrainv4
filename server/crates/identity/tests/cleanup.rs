//! Daily cleanup of expired identity rows (T0.3.8) against a real Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};
use db::PgPool;
use db::testing::TestDb;
use identity::cleanup::{BATCH, CleanupExpired, CleanupReport};
use identity::model::RegisterRequest;
use identity::service::SESSION_TTL;
use identity::{EmailSettings, IdentityService};
use kernel::email::RecordingEmailSender;
use kernel::{Clock, FixedClock};

fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 21, 12, 0, 0).unwrap()
}

fn service(pool: &PgPool, clock: &FixedClock) -> IdentityService {
    IdentityService::new(
        pool.clone(),
        Arc::new(clock.clone()),
        EmailSettings {
            sender: Arc::new(RecordingEmailSender::default()),
            web_url: "https://app.test".into(),
        },
    )
}

async fn register(service: &IdentityService, email: &str) {
    let account = RegisterRequest {
        email: email.into(),
        password: "correct horse".into(),
        display_name: "Ada".into(),
        client: identity::model::ClientKind::Native,
        device: None,
    }
    .validate()
    .unwrap();
    service.register(account).await.unwrap();
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn insert_oauth_state(pool: &PgPool, byte: u8, created_at: DateTime<Utc>) {
    sqlx::query(
        "INSERT INTO oauth_states (state_hash, provider, pkce_verifier, created_at, expires_at)
         VALUES ($1, 'google', 'verifier', $2, $3)",
    )
    .bind(vec![byte; 32])
    .bind(created_at)
    .bind(created_at + Duration::minutes(10))
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn only_expired_rows_are_deleted() {
    let db = TestDb::new().await;
    let pool = db.pool();
    let clock = FixedClock::new(start());
    let service = service(pool, &clock);

    // Old account: session and verification token created at the start.
    register(&service, "old@example.test").await;
    insert_oauth_state(pool, 1, start()).await;
    // Everything above expires (the session after SESSION_TTL, the rest much sooner).
    clock.advance(SESSION_TTL + Duration::seconds(1));
    // Fresh account and state, still valid.
    register(&service, "new@example.test").await;
    insert_oauth_state(pool, 2, clock.now()).await;

    let report = service.delete_expired().await.unwrap();
    assert_eq!(
        report,
        CleanupReport {
            sessions: 1,
            email_tokens: 1,
            oauth_states: 1,
        }
    );
    assert_eq!(count(pool, "sessions").await, 1);
    assert_eq!(count(pool, "email_tokens").await, 1);
    assert_eq!(count(pool, "oauth_states").await, 1);
    // Users are never touched.
    assert_eq!(count(pool, "users").await, 2);

    // Idempotent: a second run finds nothing.
    assert_eq!(
        service.delete_expired().await.unwrap(),
        CleanupReport::default()
    );
}

#[tokio::test]
async fn expiry_boundary_is_inclusive() {
    let db = TestDb::new().await;
    let pool = db.pool();
    let clock = FixedClock::new(start());
    let service = service(pool, &clock);
    register(&service, "ada@example.test").await;

    clock.advance(SESSION_TTL - Duration::seconds(1));
    assert_eq!(service.delete_expired().await.unwrap().sessions, 0);
    // A session is invalid from `expires_at` on (see `session_state`), so it goes then too.
    clock.advance(Duration::seconds(1));
    assert_eq!(service.delete_expired().await.unwrap().sessions, 1);
}

#[tokio::test]
async fn more_rows_than_one_batch_are_all_deleted() {
    let db = TestDb::new().await;
    let pool = db.pool();
    let clock = FixedClock::new(start());
    let service = service(pool, &clock);
    register(&service, "ada@example.test").await;

    let extra = BATCH + BATCH / 2;
    // UUIDv7-shaped ids (version nibble 7, RFC variant) for the CHECK constraint.
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, created_at, last_seen_at, expires_at)
         SELECT (lpad(to_hex(i), 12, '0') || '70008000' || lpad(to_hex(i), 12, '0'))::uuid,
                (SELECT id FROM users), sha256(i::text::bytea), $1, $1, $2
         FROM generate_series(1, $3) AS i",
    )
    .bind(start())
    .bind(start() + Duration::hours(1))
    .bind(extra)
    .execute(pool)
    .await
    .unwrap();

    clock.advance(Duration::hours(2));
    let report = service.delete_expired().await.unwrap();
    assert_eq!(report.sessions, u64::try_from(extra).unwrap());
    // The registration session (30 days) survives.
    assert_eq!(count(pool, "sessions").await, 1);
}

#[tokio::test]
async fn the_job_runs_through_the_worker() {
    let db = TestDb::new().await;
    let pool = db.pool();
    let clock = FixedClock::new(start());
    let service = service(pool, &clock);
    register(&service, "ada@example.test").await;
    clock.advance(SESSION_TTL);

    let mut worker = jobs::Worker::new(jobs::WorkerConfig::default());
    let mut scheduler = jobs::Scheduler::new(jobs::SchedulerConfig::default());
    identity::cleanup::register(service, &mut worker, &mut scheduler).unwrap();

    let mut conn = pool.acquire().await.unwrap();
    jobs::enqueue(
        &mut conn,
        &CleanupExpired {},
        jobs::EnqueueOptions::default(),
    )
    .await
    .unwrap();
    drop(conn);
    let outcome = worker.run_batch(pool).await.unwrap();
    assert_eq!(outcome.completed, 1);
    assert_eq!(count(pool, "sessions").await, 0);
    assert_eq!(count(pool, "users").await, 1);
}
