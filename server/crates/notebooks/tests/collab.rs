//! `/collab/{page_id}` end to end (spike T0.6.2): two y-sync peers built on yrs edit the same
//! page through the server. Requires Docker (or `TEST_DATABASE_URL`) for authentication.

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use futures_util::{SinkExt, StreamExt};
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use notebooks::collab::{Collab, CollabState};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;
use yrs::sync::{Awareness, DefaultProtocol, Message, Protocol, SyncMessage};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, Text, Transact, Update};

const WITHIN: Duration = Duration::from_secs(5);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Server {
    addr: SocketAddr,
    identity: IdentityService,
    collab: Collab,
    _db: TestDb,
}

async fn server() -> Server {
    let db = TestDb::new().await;
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 11, 22, 9, 0, 0).unwrap(),
    ));
    let identity = IdentityService::new(
        db.pool().clone(),
        clock,
        EmailSettings {
            sender: Arc::new(RecordingEmailSender::default()),
            web_url: "http://localhost:8081".into(),
        },
    );
    let collab = Collab::new();
    let app = notebooks::collab::router(CollabState {
        collab: collab.clone(),
        identity: identity.clone(),
        allowed_origins: vec!["http://localhost:8081".into()],
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server {
        addr,
        identity,
        collab,
        _db: db,
    }
}

async fn token(identity: &IdentityService, email: &str) -> String {
    let account = RegisterRequest {
        email: email.into(),
        password: "correct horse".into(),
        display_name: "Ada".into(),
        client: ClientKind::Native,
        device: None,
    }
    .validate()
    .unwrap();
    identity
        .register(account)
        .await
        .unwrap()
        .1
        .token
        .expose()
        .to_owned()
}

/// A y-sync client: its own document plus the socket.
struct Peer {
    awareness: Awareness,
    socket: Socket,
}

impl Peer {
    async fn connect(addr: SocketAddr, page: Uuid, token: &str) -> Self {
        let mut request = format!("ws://{addr}/collab/{page}")
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
        let doc = Doc::new();
        // Like y-websocket: start with our own sync step 1.
        let sv = doc.transact().state_vector();
        socket
            .send(WsMessage::Binary(
                Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1().into(),
            ))
            .await
            .unwrap();
        Self {
            awareness: Awareness::new(doc),
            socket,
        }
    }

    fn text(&self) -> String {
        let text = self.awareness.doc().get_or_insert_text("content");
        text.get_string(&self.awareness.doc().transact())
    }

    /// Edits locally and sends the change as an update.
    async fn insert(&mut self, at: u32, chunk: &str) {
        let doc = self.awareness.doc();
        let text = doc.get_or_insert_text("content");
        let before = doc.transact().state_vector();
        {
            let mut txn = doc.transact_mut();
            text.insert(&mut txn, at, chunk);
        }
        let update = doc.transact().encode_state_as_update_v1(&before);
        self.socket
            .send(WsMessage::Binary(
                Message::Sync(SyncMessage::Update(update))
                    .encode_v1()
                    .into(),
            ))
            .await
            .unwrap();
    }

    /// Processes server messages until `done` holds.
    async fn until(&mut self, done: impl Fn(&Self) -> bool) {
        timeout(WITHIN, async {
            while !done(self) {
                let frame = self.socket.next().await.unwrap().unwrap();
                let WsMessage::Binary(data) = frame else {
                    continue;
                };
                for reply in DefaultProtocol.handle(&mut self.awareness, &data).unwrap() {
                    self.socket
                        .send(WsMessage::Binary(reply.encode_v1().into()))
                        .await
                        .unwrap();
                }
            }
        })
        .await
        .expect("condition reached in time");
    }
}

#[tokio::test]
async fn the_handshake_requires_a_session() {
    let server = server().await;
    let request = format!("ws://{}/collab/{}", server.addr, Uuid::now_v7())
        .into_client_request()
        .unwrap();
    let error = tokio_tungstenite::connect_async(request).await.unwrap_err();
    assert!(matches!(
        error,
        tokio_tungstenite::tungstenite::Error::Http(ref response) if response.status() == 401
    ));
}

#[tokio::test]
async fn two_peers_edit_the_same_page() {
    let server = server().await;
    let page = Uuid::now_v7();
    let (ada, bob) = (
        token(&server.identity, "ada@example.com").await,
        token(&server.identity, "bob@example.com").await,
    );
    let mut a = Peer::connect(server.addr, page, &ada).await;
    let mut b = Peer::connect(server.addr, page, &bob).await;

    a.insert(0, "Merhaba").await;
    b.until(|p| p.text() == "Merhaba").await;

    // Concurrent edits converge on both sides.
    b.insert(7, " dünya").await;
    a.insert(0, ">> ").await;
    a.until(|p| p.text() == ">> Merhaba dünya").await;
    b.until(|p| p.text() == ">> Merhaba dünya").await;

    // The server's copy has it too.
    let doc = Doc::new();
    doc.transact_mut()
        .apply_update(Update::decode_v1(&server.collab.snapshot(page)).unwrap())
        .unwrap();
    let text = doc.get_or_insert_text("content");
    assert_eq!(text.get_string(&doc.transact()), ">> Merhaba dünya");
}

#[tokio::test]
async fn a_late_joiner_receives_the_current_content_and_pages_are_separate() {
    let server = server().await;
    let token = token(&server.identity, "ada@example.com").await;
    let page = Uuid::now_v7();
    let mut first = Peer::connect(server.addr, page, &token).await;
    first.insert(0, "ilk satır").await;
    first.until(|_| true).await;
    // Give the server a moment to apply the update before the second peer joins.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut late = Peer::connect(server.addr, page, &token).await;
    late.until(|p| p.text() == "ilk satır").await;

    let mut elsewhere = Peer::connect(server.addr, Uuid::now_v7(), &token).await;
    elsewhere.until(|_| true).await;
    assert_eq!(elsewhere.text(), "");
}
