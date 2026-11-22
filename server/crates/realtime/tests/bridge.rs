//! LISTEN/NOTIFY bridge (T0.5.2) against a real Postgres 17. Two hubs with their own bridges
//! stand in for two server instances sharing one database. Requires Docker (or
//! `TEST_DATABASE_URL`).
//!
//! Notification channels are database-wide while each test has its own schema, so every test
//! listens on a channel named after its schema.

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use db::testing::TestDb;
use identity::UserId;
use realtime::hub::ConnectionId;
use realtime::{Bridge, Channel, Event, Hub, Notifier, PublishError, ServerMessage};
use serde_json::json;
use tokio::sync::mpsc::Receiver;
use tokio::time::timeout;

const WITHIN: Duration = Duration::from_secs(5);
/// How long to wait before concluding that nothing was delivered.
const QUIET: Duration = Duration::from_millis(300);

struct Instance {
    hub: Hub,
    bridge: Bridge,
}

async fn instance(db: &TestDb) -> Instance {
    let hub = Hub::new();
    let bridge = Bridge::start(db.pool(), db.schema(), hub.clone())
        .await
        .unwrap();
    Instance { hub, bridge }
}

/// A connection on `hub` subscribed to `user`'s channel.
fn subscriber(hub: &Hub, user: UserId) -> (ConnectionId, Receiver<ServerMessage>) {
    let (id, inbox) = hub.connect(user);
    hub.subscribe(id, Channel::User(user));
    (id, inbox)
}

fn event(id: &str) -> Event {
    Event {
        name: "item.changed".into(),
        data: json!({"id": id, "kind": "task"}),
    }
}

async fn received(inbox: &mut Receiver<ServerMessage>) -> ServerMessage {
    timeout(WITHIN, inbox.recv()).await.unwrap().unwrap()
}

async fn nothing_received(inbox: &mut Receiver<ServerMessage>) {
    assert!(
        timeout(QUIET, inbox.recv()).await.is_err(),
        "expected no message"
    );
}

fn delivered(channel: Channel, event: Event) -> ServerMessage {
    ServerMessage::Event {
        channel: channel.to_string(),
        event,
    }
}

#[tokio::test]
async fn events_reach_subscribers_on_every_instance() {
    let db = TestDb::new().await;
    let (a, b) = (instance(&db).await, instance(&db).await);
    let (alice, bob) = (UserId::new(), UserId::new());
    let (_, mut on_a) = subscriber(&a.hub, alice);
    let (_, mut on_b) = subscriber(&b.hub, alice);
    let (_, mut bob_on_b) = subscriber(&b.hub, bob);

    let notifier = Notifier::new(db.schema());
    notifier
        .publish(db.pool(), &Channel::User(alice), &event("1"))
        .await
        .unwrap();

    let expected = delivered(Channel::User(alice), event("1"));
    assert_eq!(received(&mut on_a).await, expected);
    assert_eq!(received(&mut on_b).await, expected);
    nothing_received(&mut bob_on_b).await;
}

#[tokio::test]
async fn events_in_a_transaction_are_sent_on_commit_only() {
    let db = TestDb::new().await;
    let a = instance(&db).await;
    let user = UserId::new();
    let channel = Channel::User(user);
    let (_, mut inbox) = subscriber(&a.hub, user);
    let notifier = Notifier::new(db.schema());

    let mut tx = db.pool().begin().await.unwrap();
    notifier
        .publish(&mut *tx, &channel, &event("rolled-back"))
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    nothing_received(&mut inbox).await;

    let mut tx = db.pool().begin().await.unwrap();
    notifier
        .publish(&mut *tx, &channel, &event("committed"))
        .await
        .unwrap();
    nothing_received(&mut inbox).await;
    tx.commit().await.unwrap();
    assert_eq!(
        received(&mut inbox).await,
        delivered(channel, event("committed"))
    );
}

#[tokio::test]
async fn malformed_notifications_are_skipped() {
    let db = TestDb::new().await;
    let a = instance(&db).await;
    let user = UserId::new();
    let (_, mut inbox) = subscriber(&a.hub, user);

    for junk in [
        "not json",
        r#"{"channel":"team:1","event":{"name":"x","data":null}}"#,
    ] {
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(db.schema())
            .bind(junk)
            .execute(db.pool())
            .await
            .unwrap();
    }
    Notifier::new(db.schema())
        .publish(db.pool(), &Channel::User(user), &event("after"))
        .await
        .unwrap();
    assert_eq!(
        received(&mut inbox).await,
        delivered(Channel::User(user), event("after"))
    );
}

#[tokio::test]
async fn oversized_events_are_refused() {
    let db = TestDb::new().await;
    let big = Event {
        name: "item.changed".into(),
        data: json!({"blob": "x".repeat(realtime::bridge::MAX_PAYLOAD_BYTES)}),
    };
    let result = Notifier::new(db.schema())
        .publish(db.pool(), &Channel::User(UserId::new()), &big)
        .await;
    assert!(matches!(result, Err(PublishError::TooLarge { .. })));
}

#[tokio::test]
async fn a_stopped_bridge_delivers_nothing() {
    let db = TestDb::new().await;
    let (a, b) = (instance(&db).await, instance(&db).await);
    let user = UserId::new();
    let (_, mut on_a) = subscriber(&a.hub, user);
    let (_, mut on_b) = subscriber(&b.hub, user);

    a.bridge.stop().await;
    Notifier::new(db.schema())
        .publish(db.pool(), &Channel::User(user), &event("1"))
        .await
        .unwrap();
    assert_eq!(
        received(&mut on_b).await,
        delivered(Channel::User(user), event("1"))
    );
    nothing_received(&mut on_a).await;
    b.bridge.stop().await;
}

#[tokio::test]
async fn the_listener_recovers_after_losing_its_connection() {
    let db = TestDb::new().await;
    let a = instance(&db).await;
    let user = UserId::new();
    let (_, mut inbox) = subscriber(&a.hub, user);

    // Kill the backend that holds this channel's LISTEN (only the bridge listens on it).
    let killed: Vec<bool> = sqlx::query_scalar(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE pid <> pg_backend_pid() AND query = format('LISTEN \"%s\"', $1::text)",
    )
    .bind(db.schema())
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(killed, vec![true]);

    // The bridge reconnects and listens again; events sent afterwards arrive.
    let notifier = Notifier::new(db.schema());
    timeout(WITHIN, async {
        loop {
            notifier
                .publish(db.pool(), &Channel::User(user), &event("again"))
                .await
                .unwrap();
            if timeout(Duration::from_millis(200), inbox.recv())
                .await
                .is_ok()
            {
                break;
            }
        }
    })
    .await
    .expect("events flow again after a reconnect");
}
