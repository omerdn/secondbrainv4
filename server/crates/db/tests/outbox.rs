//! Transactional outbox and dispatcher against a real Postgres (T0.2.5).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use db::outbox::{BatchOutcome, DispatchConfig, Dispatcher, publish};
use db::testing::TestDb;
use kernel::DomainEvent;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ItemCreated {
    title: String,
}

impl DomainEvent for ItemCreated {
    const EVENT_TYPE: &'static str = "test.item_created";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Unobserved;

impl DomainEvent for Unobserved {
    const EVENT_TYPE: &'static str = "test.unobserved";
}

fn config() -> DispatchConfig {
    DispatchConfig {
        batch_size: 100,
        poll_interval: Duration::from_millis(20),
        retry_base: Duration::ZERO,
        retry_max: Duration::ZERO,
    }
}

/// Records every delivery (event id → title list).
#[derive(Clone, Default)]
struct Deliveries(Arc<Mutex<HashMap<Uuid, Vec<String>>>>);

impl Deliveries {
    fn record(&self, id: Uuid, title: String) {
        self.0.lock().unwrap().entry(id).or_default().push(title);
    }

    fn count(&self, id: Uuid) -> usize {
        self.0.lock().unwrap().get(&id).map_or(0, Vec::len)
    }

    fn total(&self) -> usize {
        self.0.lock().unwrap().values().map(Vec::len).sum()
    }
}

fn recording_dispatcher(deliveries: &Deliveries) -> Dispatcher {
    let mut dispatcher = Dispatcher::new(config());
    let sink = deliveries.clone();
    dispatcher.subscribe("recorder", move |event: ItemCreated, stored| {
        let sink = sink.clone();
        async move {
            sink.record(stored.id, event.title);
            Ok(())
        }
    });
    dispatcher
}

async fn publish_committed(db: &TestDb, title: &str) -> Uuid {
    let mut tx = db.pool().begin().await.unwrap();
    let id = publish(
        &mut tx,
        &ItemCreated {
            title: title.into(),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn row(db: &TestDb, id: Uuid) -> (bool, i32, Option<String>) {
    sqlx::query_as(
        "SELECT dispatched_at IS NOT NULL, attempts, last_error FROM outbox WHERE id = $1",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn rolled_back_transaction_publishes_nothing() {
    let db = TestDb::new().await;
    let mut tx = db.pool().begin().await.unwrap();
    publish(&mut tx, &ItemCreated { title: "x".into() })
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn event_is_invisible_until_commit() {
    let db = TestDb::new().await;
    let deliveries = Deliveries::default();
    let dispatcher = recording_dispatcher(&deliveries);

    let mut tx = db.pool().begin().await.unwrap();
    publish(&mut tx, &ItemCreated { title: "x".into() })
        .await
        .unwrap();
    assert_eq!(
        dispatcher.dispatch_batch(db.pool()).await.unwrap(),
        BatchOutcome::default()
    );
    tx.commit().await.unwrap();

    assert_eq!(
        dispatcher
            .dispatch_batch(db.pool())
            .await
            .unwrap()
            .delivered,
        1
    );
    assert_eq!(deliveries.total(), 1);
}

#[tokio::test]
async fn delivers_payload_to_every_subscriber_once_and_marks_dispatched() {
    let db = TestDb::new().await;
    let deliveries = Deliveries::default();
    let mut dispatcher = recording_dispatcher(&deliveries);
    let second = Deliveries::default();
    let sink = second.clone();
    dispatcher.subscribe("second", move |event: ItemCreated, stored| {
        let sink = sink.clone();
        async move {
            sink.record(stored.id, event.title);
            Ok(())
        }
    });

    let id = publish_committed(&db, "Gitar çalış").await;
    let outcome = dispatcher.dispatch_batch(db.pool()).await.unwrap();

    assert_eq!(
        outcome,
        BatchOutcome {
            delivered: 1,
            failed: 0
        }
    );
    assert_eq!(deliveries.0.lock().unwrap()[&id], ["Gitar çalış"]);
    assert_eq!(second.count(id), 1);
    assert_eq!(row(&db, id).await, (true, 0, None));

    // Dispatched events are not delivered again.
    assert_eq!(
        dispatcher
            .dispatch_batch(db.pool())
            .await
            .unwrap()
            .claimed(),
        0
    );
    assert_eq!(deliveries.count(id), 1);
}

#[tokio::test]
async fn events_without_subscribers_are_marked_dispatched() {
    let db = TestDb::new().await;
    let mut tx = db.pool().begin().await.unwrap();
    let id = publish(&mut tx, &Unobserved).await.unwrap();
    tx.commit().await.unwrap();

    let dispatcher = Dispatcher::new(config());
    assert_eq!(
        dispatcher
            .dispatch_batch(db.pool())
            .await
            .unwrap()
            .delivered,
        1
    );
    assert!(row(&db, id).await.0);
}

#[tokio::test]
async fn failed_delivery_is_retried_with_redacted_error() {
    let db = TestDb::new().await;
    let calls = Arc::new(Mutex::new(0));
    let mut dispatcher = Dispatcher::new(config());
    let counter = Arc::clone(&calls);
    dispatcher.subscribe("flaky", move |_: ItemCreated, _| {
        let counter = Arc::clone(&counter);
        async move {
            let mut calls = counter.lock().unwrap();
            *calls += 1;
            if *calls == 1 {
                Err("smtp rejected ali@example.com".into())
            } else {
                Ok(())
            }
        }
    });

    let id = publish_committed(&db, "x").await;

    let first = dispatcher.dispatch_batch(db.pool()).await.unwrap();
    assert_eq!(
        first,
        BatchOutcome {
            delivered: 0,
            failed: 1
        }
    );
    let (dispatched, attempts, last_error) = row(&db, id).await;
    assert!(!dispatched);
    assert_eq!(attempts, 1);
    assert_eq!(last_error.as_deref(), Some("flaky: smtp rejected <email>"));

    let second = dispatcher.dispatch_batch(db.pool()).await.unwrap();
    assert_eq!(second.delivered, 1);
    assert!(row(&db, id).await.0);
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[tokio::test]
async fn backoff_delays_the_next_attempt() {
    let db = TestDb::new().await;
    let mut dispatcher = Dispatcher::new(DispatchConfig {
        retry_base: Duration::from_secs(3600),
        retry_max: Duration::from_secs(3600),
        ..config()
    });
    dispatcher.subscribe("broken", |_: ItemCreated, _| async { Err("down".into()) });
    let id = publish_committed(&db, "x").await;

    assert_eq!(
        dispatcher.dispatch_batch(db.pool()).await.unwrap().failed,
        1
    );
    let delay_secs: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM available_at - now())::float8 FROM outbox WHERE id = $1",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert!((3500.0..=3600.0).contains(&delay_secs), "{delay_secs}");

    // Not due yet: the next batch does not claim it.
    assert_eq!(
        dispatcher
            .dispatch_batch(db.pool())
            .await
            .unwrap()
            .claimed(),
        0
    );
}

#[tokio::test]
async fn undecodable_payload_counts_as_failure() {
    let db = TestDb::new().await;
    sqlx::query("INSERT INTO outbox (id, event_type, payload) VALUES ($1, $2, '{\"wrong\": 1}')")
        .bind(Uuid::now_v7())
        .bind(ItemCreated::EVENT_TYPE)
        .execute(db.pool())
        .await
        .unwrap();

    let deliveries = Deliveries::default();
    let outcome = recording_dispatcher(&deliveries)
        .dispatch_batch(db.pool())
        .await
        .unwrap();
    assert_eq!(outcome.failed, 1);
    assert_eq!(deliveries.total(), 0);
}

/// A crash after the subscriber ran but before the batch committed must lead to redelivery.
#[tokio::test]
async fn delivery_is_at_least_once_when_the_batch_never_commits() {
    let db = TestDb::new().await;
    let id = publish_committed(&db, "x").await;

    let deliveries = Deliveries::default();
    let entered = Arc::new(Notify::new());
    let mut stalling = Dispatcher::new(config());
    let (sink, signal) = (deliveries.clone(), Arc::clone(&entered));
    stalling.subscribe("stalls", move |event: ItemCreated, stored| {
        let (sink, signal) = (sink.clone(), Arc::clone(&signal));
        async move {
            sink.record(stored.id, event.title);
            signal.notify_one();
            std::future::pending::<()>().await; // "crash" happens while we are here
            Ok(())
        }
    });

    let pool = db.pool().clone();
    let batch = tokio::spawn(async move { stalling.dispatch_batch(&pool).await });
    entered.notified().await;
    batch.abort(); // drops the open transaction without committing
    let _ = batch.await;

    assert_eq!(deliveries.count(id), 1);
    assert!(!row(&db, id).await.0, "not marked dispatched");

    let healthy = recording_dispatcher(&deliveries);
    assert_eq!(
        healthy.dispatch_batch(db.pool()).await.unwrap().delivered,
        1
    );
    assert_eq!(deliveries.count(id), 2, "redelivered");
}

#[tokio::test]
async fn concurrent_dispatchers_do_not_deliver_the_same_event_twice() {
    let db = TestDb::new().await;
    let mut ids = Vec::new();
    for i in 0..30 {
        ids.push(publish_committed(&db, &format!("e{i}")).await);
    }

    let deliveries = Deliveries::default();
    let make = || {
        let mut dispatcher = Dispatcher::new(DispatchConfig {
            batch_size: 5,
            ..config()
        });
        let sink = deliveries.clone();
        dispatcher.subscribe("slow", move |event: ItemCreated, stored| {
            let sink = sink.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(5)).await;
                sink.record(stored.id, event.title);
                Ok(())
            }
        });
        dispatcher
    };

    let mut workers = Vec::new();
    for _ in 0..3 {
        let dispatcher = make();
        let pool = db.pool().clone();
        workers.push(tokio::spawn(async move {
            loop {
                if dispatcher.dispatch_batch(&pool).await.unwrap().claimed() == 0 {
                    break;
                }
            }
        }));
    }
    for worker in workers {
        worker.await.unwrap();
    }

    assert_eq!(deliveries.total(), 30);
    assert!(ids.iter().all(|id| deliveries.count(*id) == 1));
}

#[tokio::test]
async fn run_loop_delivers_new_events_until_shutdown() {
    let db = TestDb::new().await;
    let deliveries = Deliveries::default();
    let delivered = Arc::new(Notify::new());
    let mut dispatcher = Dispatcher::new(config());
    let (sink, signal) = (deliveries.clone(), Arc::clone(&delivered));
    dispatcher.subscribe("notify", move |event: ItemCreated, stored| {
        let (sink, signal) = (sink.clone(), Arc::clone(&signal));
        async move {
            sink.record(stored.id, event.title);
            signal.notify_one();
            Ok(())
        }
    });

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let pool = db.pool().clone();
    let runner = tokio::spawn(async move {
        dispatcher
            .run(&pool, async {
                let _ = stopped.await;
            })
            .await;
    });

    let id = publish_committed(&db, "live").await;
    tokio::time::timeout(Duration::from_secs(5), delivered.notified())
        .await
        .expect("delivered by the run loop");
    assert_eq!(deliveries.count(id), 1);

    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), runner)
        .await
        .unwrap()
        .unwrap();
}
