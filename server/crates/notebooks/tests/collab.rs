//! `/collab/{page_id}` end to end (T2.5.2, PRD Y3, V9, V10): y-sync peers built on yrs edit a
//! real page through the server, with permissions and storage. Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use futures_util::{SinkExt, StreamExt};
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{ItemsAccessContext, ItemsService};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use notebooks::collab::{Collab, CollabState};
use notebooks::{CreateNotebookRequest, CreatePageRequest, NotebooksService};
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
    access: AccessEngine<ItemsAccessContext>,
    notebooks: NotebooksService,
    db: TestDb,
}

fn clock() -> kernel::SharedClock {
    Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 11, 22, 9, 0, 0).unwrap(),
    ))
}

/// Serves `/collab` for `collab` on a fresh port.
async fn serve(
    collab: Collab,
    identity: IdentityService,
    access: &AccessEngine<ItemsAccessContext>,
) -> SocketAddr {
    let app = notebooks::collab::router(CollabState {
        collab,
        identity,
        permissions: access.permissions(),
        allowed_origins: vec!["http://localhost:8081".into()],
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn server() -> Server {
    let db = TestDb::new().await;
    let identity = IdentityService::new(
        db.pool().clone(),
        clock(),
        EmailSettings {
            sender: Arc::new(RecordingEmailSender::default()),
            web_url: "http://localhost:8081".into(),
        },
    );
    let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
    let items = ItemsService::new(db.pool().clone(), clock(), access.clone());
    let notebooks = NotebooksService::new(db.pool().clone(), clock(), access.clone(), items);
    let collab = Collab::new(db.pool().clone(), clock());
    let addr = serve(collab.clone(), identity.clone(), &access).await;
    Server {
        addr,
        identity,
        collab,
        access,
        notebooks,
        db,
    }
}

impl Server {
    /// A page in a new notebook of `owner`.
    async fn page(&self, owner: Uuid) -> Uuid {
        let notebook = self
            .notebooks
            .create(
                owner,
                CreateNotebookRequest {
                    title: "Defter".into(),
                    description: None,
                },
            )
            .await
            .unwrap()
            .item
            .id;
        self.notebooks
            .create_page(
                owner,
                notebook,
                CreatePageRequest {
                    title: "Sayfa".into(),
                },
            )
            .await
            .unwrap()
            .item
            .id
    }

    async fn grant(&self, tuple: RelationTuple) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.access.grant(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    /// The page as stored, read back without any room.
    async fn stored_text(&self, page: Uuid) -> String {
        let doc = Doc::new();
        doc.transact_mut()
            .apply_update(Update::decode_v1(&self.collab.snapshot(page).await.unwrap()).unwrap())
            .unwrap();
        let text = doc.get_or_insert_text("content");
        text.get_string(&doc.transact())
    }
}

async fn account(identity: &IdentityService, email: &str) -> (Uuid, String) {
    let account = RegisterRequest {
        email: email.into(),
        password: "correct horse".into(),
        display_name: "Ada".into(),
        client: ClientKind::Native,
        device: None,
    }
    .validate()
    .unwrap();
    let (user, session) = identity.register(account).await.unwrap();
    (user.id.as_uuid(), session.token.expose().to_owned())
}

/// A y-sync client: its own document plus the socket.
struct Peer {
    awareness: Awareness,
    socket: Socket,
}

impl Peer {
    async fn connect(addr: SocketAddr, page: Uuid, token: &str) -> Self {
        Self::try_connect(addr, page, token).await.unwrap()
    }

    async fn try_connect(
        addr: SocketAddr,
        page: Uuid,
        token: &str,
    ) -> Result<Self, tokio_tungstenite::tungstenite::Error> {
        let mut request = format!("ws://{addr}/collab/{page}")
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let (mut socket, _) = timeout(WITHIN, tokio_tungstenite::connect_async(request))
            .await
            .unwrap()?;
        let doc = Doc::new();
        // Like y-websocket: start with our own sync step 1.
        let sv = doc.transact().state_vector();
        socket
            .send(WsMessage::Binary(
                Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1().into(),
            ))
            .await
            .unwrap();
        Ok(Self {
            awareness: Awareness::new(doc),
            socket,
        })
    }

    /// Waits for the server to close the connection.
    async fn closed(&mut self) {
        timeout(WITHIN, async {
            loop {
                match self.socket.next().await {
                    None | Some(Err(_) | Ok(WsMessage::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
        })
        .await
        .expect("closed in time");
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

fn status(error: &tokio_tungstenite::tungstenite::Error) -> u16 {
    match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => response.status().as_u16(),
        other => panic!("unexpected {other}"),
    }
}

#[tokio::test]
async fn v9_a_page_the_user_cannot_see_is_not_found() {
    let server = server().await;
    let (ada, _) = account(&server.identity, "ada@example.com").await;
    let (_, bob) = account(&server.identity, "bob@example.com").await;
    let page = server.page(ada).await;
    let hidden = Peer::try_connect(server.addr, page, &bob)
        .await
        .err()
        .unwrap();
    let missing = Peer::try_connect(server.addr, Uuid::now_v7(), &bob)
        .await
        .err()
        .unwrap();
    assert_eq!((status(&hidden), status(&missing)), (404, 404));
}

#[tokio::test]
async fn two_peers_edit_the_same_page_and_it_is_stored() {
    let server = server().await;
    let (ada_id, ada) = account(&server.identity, "ada@example.com").await;
    let (bob_id, bob) = account(&server.identity, "bob@example.com").await;
    let page = server.page(ada_id).await;
    server
        .grant(RelationTuple::new(page, Relation::Editor, bob_id))
        .await;
    let mut a = Peer::connect(server.addr, page, &ada).await;
    let mut b = Peer::connect(server.addr, page, &bob).await;

    a.insert(0, "Merhaba").await;
    b.until(|p| p.text() == "Merhaba").await;

    // Concurrent edits converge on both sides.
    b.insert(7, " dünya").await;
    a.insert(0, ">> ").await;
    a.until(|p| p.text() == ">> Merhaba dünya").await;
    b.until(|p| p.text() == ">> Merhaba dünya").await;

    // What both saw is what was stored (Y3).
    assert_eq!(server.stored_text(page).await, ">> Merhaba dünya");
}

#[tokio::test]
async fn y3_heavy_concurrent_editing_loses_nothing() {
    let server = server().await;
    let (ada_id, ada) = account(&server.identity, "ada@example.com").await;
    let (bob_id, bob) = account(&server.identity, "bob@example.com").await;
    let page = server.page(ada_id).await;
    server
        .grant(RelationTuple::new(page, Relation::Editor, bob_id))
        .await;
    let mut a = Peer::connect(server.addr, page, &ada).await;
    let mut b = Peer::connect(server.addr, page, &bob).await;
    // Both type at once without waiting for each other.
    for i in 0..40 {
        a.insert(0, &format!("a{i};")).await;
        b.insert(0, &format!("b{i};")).await;
    }
    let complete = |p: &Peer| {
        let text = p.text();
        (0..40).all(|i| text.contains(&format!("a{i};")) && text.contains(&format!("b{i};")))
    };
    a.until(complete).await;
    b.until(complete).await;
    assert_eq!(a.text(), b.text());
    // Give the last writes a moment, then check storage alone.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(server.stored_text(page).await, a.text());
}

#[tokio::test]
async fn y3_a_restart_keeps_every_update() {
    let server = server().await;
    let (ada_id, ada) = account(&server.identity, "ada@example.com").await;
    let page = server.page(ada_id).await;
    let mut a = Peer::connect(server.addr, page, &ada).await;
    a.insert(0, "kalici").await;
    a.insert(6, " metin").await;
    a.until(|p| p.text() == "kalici metin").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(a);
    // Compaction folds the updates into a snapshot meanwhile (T2.5.3).
    notebooks::snapshots::compact(server.db.pool(), page, Utc::now())
        .await
        .unwrap();

    // A new process: empty memory, same database.
    let restarted = Collab::new(server.db.pool().clone(), clock());
    let addr = serve(restarted, server.identity.clone(), &server.access).await;
    let mut back = Peer::connect(addr, page, &ada).await;
    back.until(|p| p.text() == "kalici metin").await;
}

#[tokio::test]
async fn y3_a_restored_version_reaches_open_editors() {
    let server = server().await;
    let (ada_id, ada) = account(&server.identity, "ada@example.com").await;
    let (bob_id, bob) = account(&server.identity, "bob@example.com").await;
    let page = server.page(ada_id).await;
    server
        .grant(RelationTuple::new(page, Relation::Viewer, bob_id))
        .await;
    let mut a = Peer::connect(server.addr, page, &ada).await;
    let mut b = Peer::connect(server.addr, page, &bob).await;
    a.insert(0, "ilk").await;
    a.until(|p| p.text() == "ilk").await;
    b.until(|p| p.text() == "ilk").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    notebooks::snapshots::compact(server.db.pool(), page, Utc::now())
        .await
        .unwrap();
    a.insert(3, " ve sonra").await;
    b.until(|p| p.text() == "ilk ve sonra").await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // T2.5.4: restoring version 1 while both are connected.
    server
        .notebooks
        .restore_version(ada_id, page, 1, &server.collab)
        .await
        .unwrap();
    a.until(|p| p.text() == "ilk").await;
    b.until(|p| p.text() == "ilk").await;
    assert_eq!(server.stored_text(page).await, "ilk");
    // Editing goes on from the restored content.
    a.insert(3, "!").await;
    b.until(|p| p.text() == "ilk!").await;
}

#[tokio::test]
async fn without_edit_a_connection_is_read_only() {
    let server = server().await;
    let (ada_id, ada) = account(&server.identity, "ada@example.com").await;
    let (cem_id, cem) = account(&server.identity, "cem@example.com").await;
    let page = server.page(ada_id).await;
    server
        .grant(RelationTuple::new(page, Relation::Viewer, cem_id))
        .await;
    let mut owner = Peer::connect(server.addr, page, &ada).await;
    let mut viewer = Peer::connect(server.addr, page, &cem).await;
    owner.insert(0, "sadece okunur").await;
    viewer.until(|p| p.text() == "sadece okunur").await;
    // The viewer's own change goes nowhere.
    viewer.insert(0, "XX ").await;
    owner.insert(13, ".").await;
    owner.until(|p| p.text() == "sadece okunur.").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(owner.text(), "sadece okunur.");
    assert_eq!(server.stored_text(page).await, "sadece okunur.");
}

#[tokio::test]
async fn v10_losing_access_closes_the_connection() {
    let server = server().await;
    let (ada_id, ada) = account(&server.identity, "ada@example.com").await;
    let (bob_id, bob) = account(&server.identity, "bob@example.com").await;
    let page = server.page(ada_id).await;
    server
        .grant(RelationTuple::new(page, Relation::Editor, bob_id))
        .await;
    let mut owner = Peer::connect(server.addr, page, &ada).await;
    let mut editor = Peer::connect(server.addr, page, &bob).await;
    editor.until(|_| true).await;
    assert_eq!(server.collab.revoke(page, &[bob_id]).await, 1);
    editor.closed().await;
    // The owner stays connected and keeps editing.
    owner.insert(0, "devam").await;
    owner.until(|p| p.text() == "devam").await;
}

#[tokio::test]
async fn a_late_joiner_receives_the_current_content_and_pages_are_separate() {
    let server = server().await;
    let (ada_id, token) = account(&server.identity, "ada@example.com").await;
    let page = server.page(ada_id).await;
    let other = server.page(ada_id).await;
    let mut first = Peer::connect(server.addr, page, &token).await;
    first.insert(0, "ilk satır").await;
    first.until(|_| true).await;
    // Give the server a moment to apply the update before the second peer joins.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut late = Peer::connect(server.addr, page, &token).await;
    late.until(|p| p.text() == "ilk satır").await;

    let mut elsewhere = Peer::connect(server.addr, other, &token).await;
    elsewhere.until(|_| true).await;
    assert_eq!(elsewhere.text(), "");
}
