//! Item channels end when access does (T1.1.6, PRD V10), end to end: WebSocket subscription →
//! revoke in the access engine → `access.revoked` outbox event → NOTIFY → bridge → client.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use access::{AccessConfig, AccessEngine, DefaultAccessContext, Relation as R, RelationTuple};
use chrono::{TimeZone, Utc};
use db::outbox::{DispatchConfig, Dispatcher};
use db::testing::TestDb;
use futures_util::{SinkExt, StreamExt};
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService, UserId};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use realtime::{
    Bridge, Channel, Event, Hub, ItemViewCheck, Notifier, RealtimeConfig, RealtimeState,
};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

const WITHIN: Duration = Duration::from_secs(5);
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct World {
    db: TestDb,
    addr: SocketAddr,
    hub: Hub,
    identity: IdentityService,
    engine: AccessEngine<DefaultAccessContext>,
    dispatcher: Dispatcher,
    _bridge: Bridge,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 22, 9, 30, 0).unwrap(),
        ));
        let identity = IdentityService::new(
            db.pool().clone(),
            clock.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "http://localhost:8081".into(),
            },
        );
        let engine = AccessEngine::new(DefaultAccessContext, AccessConfig::default());
        let hub = Hub::new();
        // Notifications are database-wide: each test uses its schema name as the channel.
        let bridge = Bridge::start(db.pool(), db.schema(), hub.clone())
            .await
            .unwrap();
        let mut dispatcher = Dispatcher::new(DispatchConfig::default());
        realtime::access::register(
            &mut dispatcher,
            db.pool().clone(),
            Notifier::new(db.schema()),
        );
        let app = realtime::router(RealtimeState {
            hub: hub.clone(),
            identity: identity.clone(),
            items: ItemViewCheck::new(engine.permissions(), db.pool().clone()),
            clock,
            config: RealtimeConfig::new(vec!["http://localhost:8081".to_owned()]),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            db,
            addr,
            hub,
            identity,
            engine,
            dispatcher,
            _bridge: bridge,
        }
    }

    async fn user(&self, email: &str) -> (UserId, String) {
        let account = RegisterRequest {
            email: email.into(),
            password: "correct horse".into(),
            display_name: "Test".into(),
            client: ClientKind::Native,
            device: None,
        }
        .validate()
        .unwrap();
        let (user, session) = self.identity.register(account).await.unwrap();
        (user.id, session.token.expose().to_owned())
    }

    async fn grant(&self, tuple: RelationTuple) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.engine.grant(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    async fn revoke(&self, tuple: RelationTuple) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.engine.revoke(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    /// Connects, skips `hello`.
    async fn socket(&self, token: &str) -> Socket {
        let mut request = format!("ws://{}/ws", self.addr)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let (mut socket, _) = timeout(WITHIN, tokio_tungstenite::connect_async(request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recv(&mut socket).await["type"], "hello");
        socket
    }
}

async fn recv(socket: &mut Socket) -> Value {
    loop {
        match timeout(WITHIN, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(_) | Message::Pong(_) => {}
            other => panic!("unexpected frame: {other:?}"),
        }
    }
}

async fn subscribe(socket: &mut Socket, channel: &str) -> Value {
    socket
        .send(Message::text(
            json!({ "type": "subscribe", "channel": channel }).to_string(),
        ))
        .await
        .unwrap();
    recv(socket).await
}

fn t(object: Uuid, relation: R, subject: Uuid) -> RelationTuple {
    RelationTuple::new(object, relation, subject)
}

fn changed() -> Event {
    Event {
        name: "item.changed".into(),
        data: json!({}),
    }
}

#[tokio::test]
async fn item_channels_need_view_access() {
    let w = World::new().await;
    let (ali, ali_token) = w.user("ali@example.test").await;
    let task = Uuid::now_v7();
    w.grant(t(task, R::Owner, ali.as_uuid())).await;

    let mut socket = w.socket(&ali_token).await;
    let channel = format!("item:{task}");
    assert_eq!(
        subscribe(&mut socket, &channel).await,
        json!({ "type": "subscribed", "channel": channel })
    );
    let hidden = format!("item:{}", Uuid::now_v7());
    let reply = subscribe(&mut socket, &hidden).await;
    assert_eq!(reply["type"], "error");
    assert_eq!(reply["code"], "forbidden");
    assert_eq!(w.hub.publish(&Channel::Item(task), &changed()), 1);
}

#[tokio::test]
async fn losing_access_ends_the_open_subscription() {
    let w = World::new().await;
    let (ali, ali_token) = w.user("ali@example.test").await;
    let (ayse, ayse_token) = w.user("ayse@example.test").await;
    let task = Uuid::now_v7();
    w.grant(t(task, R::Owner, ali.as_uuid())).await;
    w.grant(t(task, R::Watcher, ayse.as_uuid())).await;

    let channel = format!("item:{task}");
    let mut ali_socket = w.socket(&ali_token).await;
    let mut ayse_socket = w.socket(&ayse_token).await;
    assert_eq!(
        subscribe(&mut ali_socket, &channel).await["type"],
        "subscribed"
    );
    assert_eq!(
        subscribe(&mut ayse_socket, &channel).await["type"],
        "subscribed"
    );
    assert_eq!(w.hub.publish(&Channel::Item(task), &changed()), 2);
    recv(&mut ali_socket).await;
    recv(&mut ayse_socket).await;

    // Ali removes Ayşe; the outbox dispatcher (one instance) forwards the event to every
    // instance's hub through NOTIFY.
    w.revoke(t(task, R::Watcher, ayse.as_uuid())).await;
    let outcome = w.dispatcher.dispatch_batch(w.db.pool()).await.unwrap();
    assert_eq!(outcome.delivered, 1);

    // V10: her subscription ends at once, with the reason; Ali keeps his.
    assert_eq!(
        recv(&mut ayse_socket).await,
        json!({ "type": "unsubscribed", "channel": channel, "reason": "access_revoked" })
    );
    assert_eq!(w.hub.publish(&Channel::Item(task), &changed()), 1);
    assert_eq!(recv(&mut ali_socket).await["type"], "event");

    // Re-subscribing is refused now.
    let reply = subscribe(&mut ayse_socket, &channel).await;
    assert_eq!(reply["code"], "forbidden");
}
