//! Shared work sessions against a real Postgres 17 (T3.6.1, PRD W6, ADR-0015, ARCHITECTURE
//! §11): one timer for everyone, each participant's own item and log, friends only. Requires
//! Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use chrono::{DateTime, Duration, TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService, SessionId, UserId};
use items::{CreateItemRequest, ItemsAccessContext, ItemsService};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use realtime::{Bridge, Channel, Hub, Notifier, ServerMessage};
use sessions::timer::{Command, Mode, State};
use sessions::{InviteRequest, JoinRequest, OpenRequest, TIMER_EVENT, WorkSessions};
use uuid::Uuid;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 27, 9, 0, 0).unwrap()
}

/// Stand-in for friendships: who may share with whom (V1).
#[derive(Clone, Default)]
struct Friends(Arc<Mutex<HashSet<(Uuid, Uuid)>>>);

impl Friends {
    fn befriend(&self, a: Uuid, b: Uuid) {
        let mut pairs = self.0.lock().unwrap();
        pairs.insert((a, b));
        pairs.insert((b, a));
    }

    fn unfriend(&self, a: Uuid, b: Uuid) {
        let mut pairs = self.0.lock().unwrap();
        pairs.remove(&(a, b));
        pairs.remove(&(b, a));
    }
}

impl items::SharingPolicy for Friends {
    fn can_share(&self, from: Uuid, to: Uuid) -> items::SharingFuture<'_> {
        let allowed = self.0.lock().unwrap().contains(&(from, to));
        Box::pin(async move { Ok(allowed) })
    }
}

struct World {
    db: TestDb,
    clock: Arc<FixedClock>,
    identity: IdentityService,
    items: ItemsService,
    access: AccessEngine<ItemsAccessContext>,
    friends: Friends,
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
        let friends = Friends::default();
        let sessions = WorkSessions::new(
            pool,
            clock.clone(),
            access.permissions(),
            Notifier::new(db.schema()),
        )
        .with_sharing(Arc::new(friends.clone()));
        Self {
            db,
            clock,
            identity,
            items,
            access,
            friends,
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
            accepted_legal: identity::legal::current_acceptances(),
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

    /// Ali's shared session that Ayşe joined: their ids and the session's.
    async fn pair(&self) -> (Uuid, Uuid, Uuid) {
        let ali = self.user("ali@example.com").await;
        let ayse = self.user("ayse@example.com").await;
        self.friends.befriend(ali, ayse);
        let id = self
            .sessions
            .open_shared(ali, OpenRequest::default())
            .await
            .unwrap()
            .id;
        self.sessions
            .invite(ali, id, InviteRequest { user_id: ayse })
            .await
            .unwrap();
        (ali, ayse, id)
    }
}

fn start(item: Option<Uuid>) -> Command {
    Command::Start {
        mode: Mode::Stopwatch,
        item_id: item,
        planned_ms: None,
    }
}

#[tokio::test]
async fn w6_one_timer_for_everyone_and_a_log_of_ones_own() {
    let w = World::new().await;
    let (ali, ayse, id) = w.pair().await;
    let report = w.task(ali, "Rapor").await;
    let reading = w.task(ayse, "Okuma").await;
    // Ali starts before Ayşe joins; she joins 10 minutes in with her own item.
    w.sessions
        .command(ali, id, start(Some(report)))
        .await
        .unwrap();
    w.wait(10);
    let joined = w
        .sessions
        .join(
            ayse,
            id,
            JoinRequest {
                item_id: Some(reading),
            },
        )
        .await
        .unwrap();
    assert!(joined.shared);
    assert_eq!(joined.participants, [ali, ayse]);
    assert_eq!(joined.timer.state, State::Running);
    assert_eq!(
        joined.timer.item_id,
        Some(reading),
        "each sees their own item"
    );
    w.wait(5);
    // Anyone pauses and stops for everyone.
    let paused = w.sessions.command(ayse, id, Command::Pause).await.unwrap();
    assert_eq!(paused.timer.state, State::Paused);
    assert_eq!(
        w.sessions.get(ali, id).await.unwrap().timer.state,
        State::Paused
    );
    w.wait(3);
    w.sessions.command(ali, id, Command::Resume).await.unwrap();
    w.wait(5);
    w.sessions.command(ayse, id, Command::Stop).await.unwrap();

    let his = w.sessions.get(ali, id).await.unwrap();
    let hers = w.sessions.get(ayse, id).await.unwrap();
    assert_eq!(his.timer.item_id, Some(report));
    assert_eq!(his.segments.len(), 1);
    assert_eq!(his.segments[0].item_id, Some(report));
    assert_eq!(
        his.segments[0].active_ms,
        20 * 60_000,
        "10 + 5 + 5 minutes, pause left out"
    );
    assert_eq!(hers.segments.len(), 1, "W6: only her own segments");
    assert_eq!(hers.segments[0].item_id, Some(reading));
    assert_eq!(
        hers.segments[0].active_ms,
        10 * 60_000,
        "from when she joined"
    );
}

#[tokio::test]
async fn w6_switching_ones_item_does_not_touch_the_others() {
    let w = World::new().await;
    let (ali, ayse, id) = w.pair().await;
    w.sessions
        .join(ayse, id, JoinRequest::default())
        .await
        .unwrap();
    let first = w.task(ayse, "İlk").await;
    let second = w.task(ayse, "İkinci").await;
    w.sessions
        .command(ayse, id, start(Some(first)))
        .await
        .unwrap();
    w.wait(4);
    w.sessions.command(ali, id, Command::Pause).await.unwrap();
    // Paused: Ayşe moves her time to another item; Ali's log is untouched.
    w.sessions
        .command(
            ayse,
            id,
            Command::SwitchItem {
                item_id: Some(second),
            },
        )
        .await
        .unwrap();
    w.sessions.command(ali, id, Command::Resume).await.unwrap();
    w.wait(6);
    w.sessions.command(ali, id, Command::Stop).await.unwrap();
    let hers = w.sessions.get(ayse, id).await.unwrap().segments;
    assert_eq!(
        hers.iter()
            .map(|s| (s.item_id, s.active_ms))
            .collect::<Vec<_>>(),
        [(Some(first), 4 * 60_000), (Some(second), 6 * 60_000)]
    );
    let his = w.sessions.get(ali, id).await.unwrap().segments;
    assert_eq!(
        his.iter()
            .map(|s| (s.item_id, s.active_ms))
            .collect::<Vec<_>>(),
        [(None, 10 * 60_000)]
    );
}

#[tokio::test]
async fn w6_timer_events_reach_every_participant() {
    let w = World::new().await;
    let (ali, ayse, id) = w.pair().await;
    w.sessions
        .join(ayse, id, JoinRequest::default())
        .await
        .unwrap();
    let hub = Hub::new();
    let bridge = Bridge::start(w.db.pool(), w.db.schema(), hub.clone())
        .await
        .unwrap();
    let mut listeners = Vec::new();
    for user in [ali, ayse] {
        let connection = hub.connect(UserId::from_uuid(user), SessionId::new());
        hub.subscribe(connection.id, Channel::User(UserId::from_uuid(user)));
        listeners.push(connection);
    }
    w.sessions.command(ayse, id, start(None)).await.unwrap();
    for connection in &mut listeners {
        let message = loop {
            let next =
                tokio::time::timeout(std::time::Duration::from_secs(5), connection.outbox.recv())
                    .await
                    .unwrap()
                    .unwrap();
            if let ServerMessage::Event { event, .. } = next {
                break event;
            }
        };
        assert_eq!(message.name, TIMER_EVENT);
        assert_eq!(message.data["session_id"], id.to_string());
        assert_eq!(message.data["timer"]["state"], "running");
        assert!(
            message.data["timer"]["item_id"].is_null(),
            "no one's item is shared"
        );
    }
    bridge.stop().await;
}

#[tokio::test]
async fn w6_only_invited_friends_join() {
    let w = World::new().await;
    let (ali, ayse, id) = w.pair().await;
    let stranger = w.user("x@example.com").await;
    // A stranger can be neither invited nor join, nor see the session.
    assert!(matches!(
        w.sessions
            .invite(ali, id, InviteRequest { user_id: stranger })
            .await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        w.sessions.join(stranger, id, JoinRequest::default()).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.sessions.get(stranger, id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.sessions.command(stranger, id, Command::Pause).await,
        Err(AppError::NotFound)
    ));
    // The invitation shows; once the friendship ends, it can no longer be used.
    let invited = w.sessions.invitations(ayse).await.unwrap();
    assert_eq!(invited.len(), 1);
    assert_eq!((invited[0].session_id, invited[0].invited_by), (id, ali));
    w.friends.unfriend(ali, ayse);
    assert!(matches!(
        w.sessions.join(ayse, id, JoinRequest::default()).await,
        Err(AppError::Forbidden)
    ));
    // A personal session takes no guests.
    let own = w
        .sessions
        .open(ali, OpenRequest::default())
        .await
        .unwrap()
        .id;
    w.friends.befriend(ali, ayse);
    assert!(matches!(
        w.sessions
            .invite(ali, own, InviteRequest { user_id: ayse })
            .await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn w6_an_item_one_cannot_see_cannot_be_brought_in() {
    let w = World::new().await;
    let (ali, ayse, id) = w.pair().await;
    let secret = w.task(ali, "Gizli").await;
    assert!(matches!(
        w.sessions
            .join(
                ayse,
                id,
                JoinRequest {
                    item_id: Some(secret)
                }
            )
            .await,
        Err(AppError::NotFound)
    ));
    // Shared with her, it can.
    let mut tx = w.db.pool().begin().await.unwrap();
    w.access
        .grant(&mut tx, RelationTuple::new(secret, Relation::Watcher, ayse))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    w.sessions
        .join(
            ayse,
            id,
            JoinRequest {
                item_id: Some(secret),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn w6_leaving_ends_ones_own_segment_and_the_last_one_closes_it() {
    let w = World::new().await;
    let (ali, ayse, id) = w.pair().await;
    w.sessions
        .join(ayse, id, JoinRequest::default())
        .await
        .unwrap();
    w.sessions.command(ali, id, start(None)).await.unwrap();
    w.wait(7);
    w.sessions.leave(ayse, id).await.unwrap();
    // Gone: she no longer sees it, Ali's timer goes on.
    assert!(matches!(
        w.sessions.get(ayse, id).await,
        Err(AppError::NotFound)
    ));
    let his = w.sessions.get(ali, id).await.unwrap();
    assert_eq!(his.participants, [ali]);
    assert_eq!(his.timer.state, State::Running);
    w.wait(3);
    w.sessions.leave(ali, id).await.unwrap();
    // Her history keeps her 7 minutes; the session is closed.
    let history = w.sessions.history(ayse, None).await.unwrap();
    assert_eq!(history[0].id, id);
    assert_eq!(history[0].work_ms, 7 * 60_000);
    assert!(!history[0].open);
    assert!(w.sessions.shared(ali).await.unwrap().is_empty());
}
