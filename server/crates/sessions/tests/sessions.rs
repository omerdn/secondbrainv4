//! Work sessions against a real Postgres 17 (T2.6.1, PRD W1–W3, ADR-0015). Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{DateTime, Duration, TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{CreateItemRequest, ItemsAccessContext, ItemsService};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use sessions::timer::{Command, Mode, State};
use sessions::{OpenRequest, PomodoroConfig, WorkSessions};
use uuid::Uuid;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 26, 9, 0, 0).unwrap()
}

struct World {
    _db: TestDb,
    clock: Arc<FixedClock>,
    identity: IdentityService,
    items: ItemsService,
    sessions: WorkSessions,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let clock = Arc::new(FixedClock::new(t0()));
        let identity = IdentityService::new(
            pool.clone(),
            clock.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "http://localhost:8081".into(),
            },
        );
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(pool.clone(), clock.clone(), access.clone());
        let sessions = WorkSessions::new(
            pool,
            clock.clone(),
            access.permissions(),
            realtime::Notifier::default(),
        );
        Self {
            _db: db,
            clock,
            identity,
            items,
            sessions,
        }
    }

    async fn user(&self, email: &str) -> Uuid {
        let request = RegisterRequest {
            email: email.into(),
            password: "correct horse".into(),
            display_name: "Ada".into(),
            client: ClientKind::Native,
            device: None,
        }
        .validate()
        .unwrap();
        self.identity
            .register(request)
            .await
            .unwrap()
            .0
            .id
            .as_uuid()
    }

    async fn task(&self, owner: Uuid, title: &str) -> Uuid {
        self.items
            .create(
                owner,
                CreateItemRequest {
                    kind: "task".into(),
                    title: title.into(),
                    description: None,
                }
                .validate()
                .unwrap(),
            )
            .await
            .unwrap()
            .id
    }

    fn wait(&self, minutes: i64) {
        self.clock.advance(Duration::minutes(minutes));
    }
}

#[tokio::test]
async fn w1_a_session_stays_open_until_closed_and_opening_again_returns_it() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let first = w.sessions.open(ada, OpenRequest::default()).await.unwrap();
    assert!(first.open);
    assert_eq!(first.pomodoro, PomodoroConfig::default());
    assert_eq!(first.timer.state, State::Stopped);
    // Opening again gives the same session (the unique index also settles concurrent opens).
    let again = futures(&w, ada).await;
    assert!(again.iter().all(|id| *id == first.id));
    assert_eq!(w.sessions.current(ada).await.unwrap().id, first.id);
    w.wait(600);
    assert!(w.sessions.current(ada).await.unwrap().open);

    let closed = w.sessions.close(ada, first.id).await.unwrap();
    assert!(!closed.open);
    assert_eq!(closed.closed_at, Some(t0() + Duration::minutes(600)));
    // Closing again changes nothing; commands are refused; a new session can be opened.
    assert_eq!(
        w.sessions.close(ada, first.id).await.unwrap(),
        w.sessions.get(ada, first.id).await.unwrap()
    );
    assert!(matches!(
        w.sessions.command(ada, first.id, Command::Pause).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        w.sessions.current(ada).await,
        Err(AppError::NotFound)
    ));
    let next = w.sessions.open(ada, OpenRequest::default()).await.unwrap();
    assert_ne!(next.id, first.id);
}

async fn futures(w: &World, user: Uuid) -> Vec<Uuid> {
    let opens = (0..4).map(|_| w.sessions.open(user, OpenRequest::default()));
    let mut ids = Vec::new();
    for result in futures_join(opens).await {
        ids.push(result.unwrap().id);
    }
    ids
}

async fn futures_join<F: std::future::Future>(futures: impl Iterator<Item = F>) -> Vec<F::Output> {
    let mut out = Vec::new();
    let handles: Vec<F> = futures.collect();
    for f in handles {
        out.push(f.await);
    }
    out
}

#[tokio::test]
async fn w3_switching_the_item_starts_a_new_segment_and_the_log_adds_up() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let (gitar, rapor) = (w.task(ada, "Gitar").await, w.task(ada, "Rapor").await);
    let s = w
        .sessions
        .open(ada, OpenRequest::default())
        .await
        .unwrap()
        .id;
    let start = Command::Start {
        mode: Mode::Stopwatch,
        item_id: Some(gitar),
        planned_ms: None,
    };
    let running = w.sessions.command(ada, s, start).await.unwrap();
    assert_eq!(running.timer.state, State::Running);
    assert_eq!(running.timer.started_at, Some(t0()));
    // A retried start changes nothing.
    assert_eq!(
        w.sessions
            .command(ada, s, start)
            .await
            .unwrap()
            .segments
            .len(),
        1
    );
    w.wait(45);
    w.sessions.command(ada, s, Command::Pause).await.unwrap();
    // Not while running, then a new segment once paused (W3).
    let switch = Command::SwitchItem {
        item_id: Some(rapor),
    };
    w.wait(1);
    let switched = w.sessions.command(ada, s, switch).await.unwrap();
    assert_eq!(switched.segments.len(), 2);
    assert_eq!(switched.segments[0].item_id, Some(gitar));
    assert_eq!(switched.segments[0].active_ms, 45 * 60_000);
    assert_eq!(switched.segments[1].item_id, Some(rapor));
    w.sessions.command(ada, s, Command::Resume).await.unwrap();
    w.wait(25);
    let stopped = w.sessions.command(ada, s, Command::Stop).await.unwrap();
    assert_eq!(stopped.timer.state, State::Stopped);
    let log: Vec<i64> = stopped.segments.iter().map(|seg| seg.active_ms).collect();
    assert_eq!(log, vec![45 * 60_000, 25 * 60_000]);
    assert!(stopped.segments.iter().all(|seg| seg.ended_at.is_some()));
    // Stopping again is a no-op; switching while running is refused.
    assert_eq!(
        w.sessions
            .command(ada, s, Command::Stop)
            .await
            .unwrap()
            .segments
            .len(),
        2
    );
    let pomodoro = Command::Start {
        mode: Mode::Pomodoro,
        item_id: None,
        planned_ms: None,
    };
    let counting = w.sessions.command(ada, s, pomodoro).await.unwrap();
    assert_eq!(counting.timer.planned_ms, Some(25 * 60_000));
    assert!(matches!(
        w.sessions.command(ada, s, switch).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn sessions_and_items_of_others_stay_hidden() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let bob = w.user("bob@example.com").await;
    let secret = w.task(ada, "Gizli").await;
    let s = w
        .sessions
        .open(ada, OpenRequest::default())
        .await
        .unwrap()
        .id;
    let bobs = w
        .sessions
        .open(bob, OpenRequest::default())
        .await
        .unwrap()
        .id;
    assert!(matches!(
        w.sessions.get(bob, s).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.sessions.command(bob, s, Command::Stop).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.sessions.close(bob, s).await,
        Err(AppError::NotFound)
    ));
    // An item Bob cannot see cannot be timed in his session (V9).
    let start = Command::Start {
        mode: Mode::Stopwatch,
        item_id: Some(secret),
        planned_ms: None,
    };
    assert!(matches!(
        w.sessions.command(bob, bobs, start).await,
        Err(AppError::NotFound)
    ));
    // Bad pomodoro lengths are refused.
    let bad = OpenRequest {
        pomodoro: Some(PomodoroConfig {
            work_ms: 10,
            short_break_ms: 60_000,
            long_break_ms: 60_000,
        }),
    };
    assert!(matches!(
        w.sessions.open(w.user("cem@example.com").await, bad).await,
        Err(AppError::Validation(_))
    ));
}

fn stopwatch(item: Option<Uuid>) -> Command {
    Command::Start {
        mode: Mode::Stopwatch,
        item_id: item,
        planned_ms: None,
    }
}

#[tokio::test]
async fn w4_the_log_is_in_order_and_w5_nothing_is_completed() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let tasks = tasks::TasksService::new(
        w._db.pool().clone(),
        w.clock.clone(),
        AccessEngine::new(ItemsAccessContext, AccessConfig::default()).permissions(),
        w.items.clone(),
    );
    let task = tasks
        .create(
            ada,
            tasks::CreateTaskRequest {
                title: "Rapor".into(),
                description: None,
                start_date: None,
                due_date: None,
                subtasks: None,
            },
        )
        .await
        .unwrap()
        .item
        .id;
    let gitar = w.task(ada, "Gitar").await;
    let s = w
        .sessions
        .open(ada, OpenRequest::default())
        .await
        .unwrap()
        .id;
    // 45 min Gitar → 5 min break → 25 min Rapor (W4's example).
    w.sessions
        .command(ada, s, stopwatch(Some(gitar)))
        .await
        .unwrap();
    w.wait(45);
    w.sessions.command(ada, s, Command::Stop).await.unwrap();
    let pause = Command::Start {
        mode: Mode::Break,
        item_id: None,
        planned_ms: Some(5 * 60_000),
    };
    w.sessions.command(ada, s, pause).await.unwrap();
    w.wait(5);
    w.sessions.command(ada, s, Command::Stop).await.unwrap();
    let pomodoro = Command::Start {
        mode: Mode::Pomodoro,
        item_id: Some(task),
        planned_ms: None,
    };
    w.sessions.command(ada, s, pomodoro).await.unwrap();
    w.wait(25);
    let session = w.sessions.command(ada, s, Command::Stop).await.unwrap();
    let log: Vec<(Mode, Option<Uuid>, i64)> = session
        .segments
        .iter()
        .map(|seg| (seg.mode, seg.item_id, seg.active_ms / 60_000))
        .collect();
    assert_eq!(
        log,
        vec![
            (Mode::Stopwatch, Some(gitar), 45),
            (Mode::Break, None, 5),
            (Mode::Pomodoro, Some(task), 25),
        ]
    );
    // W5: the time is statistics; the task is not done, nor any of its subtasks.
    let view = tasks.task(ada, task).await.unwrap();
    assert!(view.subtasks.iter().all(|sub| sub.done_at.is_none()));
    w.sessions.close(ada, s).await.unwrap();
    let history = w.sessions.history(ada, None).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        (history[0].work_ms, history[0].break_ms),
        (70 * 60_000, 5 * 60_000)
    );
    assert_eq!(history[0].segments, 3);
    assert!(!history[0].open);
}

#[tokio::test]
async fn statistics_follow_the_users_time_zone_and_include_the_running_segment() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let gitar = w.task(ada, "Gitar").await;
    let istanbul: chrono_tz::Tz = "Europe/Istanbul".parse().unwrap();
    // 09:00 UTC on the 26th; move to 22:30 UTC = 01:30 on the 27th in Istanbul.
    w.wait(13 * 60 + 30);
    let s = w
        .sessions
        .open(ada, OpenRequest::default())
        .await
        .unwrap()
        .id;
    w.sessions
        .command(ada, s, stopwatch(Some(gitar)))
        .await
        .unwrap();
    w.wait(20);
    // Still running: counted up to now.
    let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
    let local = w
        .sessions
        .stats(ada, (Some(day(26)), Some(day(27))), istanbul)
        .await
        .unwrap();
    assert_eq!(local.days[0].work_ms, 0);
    assert_eq!(local.days[1].work_ms, 20 * 60_000);
    assert_eq!(local.items[0].item_id, Some(gitar));
    let utc = w
        .sessions
        .stats(ada, (Some(day(26)), Some(day(27))), chrono_tz::UTC)
        .await
        .unwrap();
    assert_eq!(utc.days[0].work_ms, 20 * 60_000);
    // Defaults to the last 7 days; bad ranges are refused; others see nothing.
    assert_eq!(
        w.sessions
            .stats(ada, (None, None), istanbul)
            .await
            .unwrap()
            .days
            .len(),
        7
    );
    assert!(matches!(
        w.sessions
            .stats(ada, (Some(day(27)), Some(day(26))), istanbul)
            .await,
        Err(AppError::Validation(_))
    ));
    let bob = w.user("bob@example.com").await;
    assert_eq!(
        w.sessions
            .stats(bob, (Some(day(26)), Some(day(27))), istanbul)
            .await
            .unwrap()
            .work_ms,
        0
    );
    assert!(w.sessions.history(bob, None).await.unwrap().is_empty());
}
