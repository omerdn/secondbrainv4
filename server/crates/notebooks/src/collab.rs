//! Collaborative page editing over y-sync (ADR-0005, ADR-0019, ARCHITECTURE §9, T2.5.2).
//!
//! `GET /collab/{page_id}` upgrades to a WebSocket that speaks the y-sync protocol in binary
//! frames, exactly what Yjs' `y-websocket` provider expects: sync step 1/2, updates and
//! awareness (live cursors). The server keeps one [`yrs`] document per page being edited and
//! fans out every document and awareness change to all connections of that page.
//!
//! - **Permissions:** the handshake needs VIEW on the page (otherwise 404, V9). Without EDIT the
//!   connection is read-only: it receives everything, and the document updates it sends are
//!   dropped.
//! - **No data loss (Y3):** every update is written to `page_updates` *before* it is applied and
//!   passed on. If the write fails the connection closes without applying it; the client keeps
//!   the change and sends it again after reconnecting. A room is rebuilt from the stored updates
//!   when the first connection of a page opens, so a restart loses nothing.
//! - **Access loss (V10):** [`Collab::revoke`] closes the connections of users who lost access to
//!   a page; [`register_revocation`] calls it on `AccessRevoked`. A room is dropped from memory
//!   when its last connection closes.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use access::{PermissionService, Perms};
use axum::Router;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRef, Path, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::Response;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use identity::IdentityService;
use identity::http::Authenticated;
use items::ItemsAccessContext;
use kernel::{AppError, SharedClock};
use sqlx::PgPool;
use tokio::sync::{broadcast, watch};
use uuid::Uuid;
use yrs::sync::{
    Awareness, AwarenessUpdate, DefaultProtocol, Message, MessageReader, Protocol, SyncMessage,
};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{ClientID, Doc, Transact, Update};

use crate::{repo, snapshots};

/// Largest frame accepted from a client. Large pastes arrive as a single update.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Messages buffered per connection before it is dropped as too slow (it reconnects and
/// resynchronises with sync step 1).
const BROADCAST_CAPACITY: usize = 256;

/// One open connection: whose it is and how to close it.
struct Member {
    user: Uuid,
    close: watch::Sender<bool>,
}

/// One page being edited: its document, awareness, the fan-out channel and its connections.
struct Room {
    awareness: Mutex<Awareness>,
    updates: broadcast::Sender<Arc<Vec<u8>>>,
    members: Mutex<HashMap<u64, Member>>,
    /// Updates this room stored since it last asked for compaction.
    stored_since_compaction: AtomicU32,
}

impl Room {
    /// A room holding `doc`, the page as stored.
    fn new(doc: Doc) -> Arc<Self> {
        let (updates, _) = broadcast::channel(BROADCAST_CAPACITY);
        {
            let tx = updates.clone();
            doc.observe_update_v1("fan-out", move |_, event| {
                let message = Message::Sync(SyncMessage::Update(event.update.clone()));
                let _ = tx.send(Arc::new(message.encode_v1()));
            })
            .ok();
        }
        let mut awareness = Awareness::new(doc);
        {
            let tx = updates.clone();
            awareness.on_update("fan-out", move |awareness, event, _| {
                let changed = event.summary().all_changes();
                if let Ok(update) = awareness.update_with_clients(changed) {
                    let _ = tx.send(Arc::new(Message::Awareness(update).encode_v1()));
                }
            });
        }
        Arc::new(Self {
            awareness: Mutex::new(awareness),
            updates,
            members: Mutex::new(HashMap::new()),
            stored_since_compaction: AtomicU32::new(0),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Awareness> {
        self.awareness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn members(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Member>> {
        self.members.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Rooms by page id, plus where updates are stored. Cheap to clone.
#[derive(Clone)]
pub struct Collab {
    pool: PgPool,
    clock: SharedClock,
    rooms: Arc<tokio::sync::Mutex<HashMap<Uuid, Arc<Room>>>>,
    next_id: Arc<AtomicU64>,
}

impl std::fmt::Debug for Collab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collab").finish_non_exhaustive()
    }
}

impl Collab {
    #[must_use]
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self {
            pool,
            clock,
            rooms: Arc::default(),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// The page's room, loaded from its stored updates if nobody is editing it.
    async fn room(&self, page: Uuid) -> Result<Arc<Room>, AppError> {
        let mut rooms = self.rooms.lock().await;
        if let Some(room) = rooms.get(&page) {
            return Ok(room.clone());
        }
        let doc = snapshots::load(&self.pool, page)
            .await
            .map_err(|err| AppError::Internal(Box::new(err)))?;
        let room = Room::new(doc);
        rooms.insert(page, room.clone());
        Ok(room)
    }

    /// Forgets the room when its last connection has gone; it is reloaded on the next one.
    async fn release(&self, page: Uuid, id: u64) {
        let mut rooms = self.rooms.lock().await;
        let Some(room) = rooms.get(&page) else { return };
        let empty = {
            let mut members = room.members();
            members.remove(&id);
            members.is_empty()
        };
        if empty {
            rooms.remove(&page);
        }
    }

    /// Closes the connections of `users` to `page` at once (V10). Returns how many closed.
    pub async fn revoke(&self, page: Uuid, users: &[Uuid]) -> usize {
        let rooms = self.rooms.lock().await;
        let Some(room) = rooms.get(&page) else {
            return 0;
        };
        let members = room.members();
        members
            .values()
            .filter(|member| users.contains(&member.user))
            .map(|member| member.close.send(true))
            .filter(Result::is_ok)
            .count()
    }

    /// The page's current content as a Yjs update, as stored (tests).
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn snapshot(&self, page: Uuid) -> Result<Vec<u8>, AppError> {
        let doc = snapshots::load(&self.pool, page)
            .await
            .map_err(|err| AppError::Internal(Box::new(err)))?;
        Ok(snapshots::encode(&doc))
    }

    /// Makes `page` hold the content of `target` with a new update by `author` (Y3, T2.5.4):
    /// stored first, then applied to the open room, which passes it to every editor. Concurrent
    /// edits merge with it. Returns whether anything changed.
    ///
    /// # Errors
    /// [`AppError::Internal`]; nothing is applied then.
    pub async fn restore(&self, page: Uuid, target: &Doc, author: Uuid) -> Result<bool, AppError> {
        let internal = |err: Box<dyn std::error::Error + Send + Sync>| AppError::Internal(err);
        // Held throughout, so no room opens from storage between writing and applying.
        let rooms = self.rooms.lock().await;
        let room = rooms.get(&page).cloned();
        let current = match &room {
            Some(room) => {
                let state = snapshots::encode(room.lock().doc());
                snapshots::rebuild(Some(&state), &[]).map_err(|err| internal(Box::new(err)))?
            }
            None => snapshots::load(&self.pool, page)
                .await
                .map_err(|err| internal(Box::new(err)))?,
        };
        let update = snapshots::revert_update(&current, target);
        if update.is_empty() {
            return Ok(false);
        }
        repo::append_update(&self.pool, page, &update, author, self.clock.now())
            .await
            .map_err(|err| internal(Box::new(err)))?;
        if let Some(room) = room {
            let decoded = Update::decode_v1(&update).map_err(|err| internal(Box::new(err)))?;
            room.lock()
                .doc()
                .transact_mut()
                .apply_update(decoded)
                .map_err(|err| internal(Box::new(err)))?;
        }
        drop(rooms);
        Ok(true)
    }

    /// Writes `update` for `page` (Y3), and asks for compaction every
    /// [`snapshots::COMPACT_EVERY`] updates of the room.
    async fn persist(
        &self,
        room: &Room,
        page: Uuid,
        update: &[u8],
        author: Uuid,
    ) -> sqlx::Result<()> {
        repo::append_update(&self.pool, page, update, author, self.clock.now()).await?;
        let stored = room.stored_since_compaction.fetch_add(1, Ordering::Relaxed) + 1;
        if stored >= snapshots::COMPACT_EVERY {
            room.stored_since_compaction.store(0, Ordering::Relaxed);
            let mut conn = self.pool.acquire().await?;
            // A lost request is harmless: the 10-minute sweep compacts the page anyway.
            if let Err(err) = jobs::enqueue(
                &mut conn,
                &snapshots::CompactPage { page_id: page },
                jobs::EnqueueOptions::default(),
            )
            .await
            {
                tracing::warn!(error = %err, "page compaction not queued");
            }
        }
        Ok(())
    }
}

/// Everything the route needs.
#[derive(Clone)]
pub struct CollabState {
    pub collab: Collab,
    pub identity: IdentityService,
    pub permissions: PermissionService<ItemsAccessContext>,
    /// Browser origins allowed to open a cookie-authenticated connection (the CORS origins).
    pub allowed_origins: Vec<String>,
}

impl std::fmt::Debug for CollabState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollabState").finish_non_exhaustive()
    }
}

impl FromRef<CollabState> for IdentityService {
    fn from_ref(state: &CollabState) -> Self {
        state.identity.clone()
    }
}

/// `GET /collab/{page_id}` with its state applied.
pub fn router(state: CollabState) -> Router {
    Router::new()
        .route("/collab/{page_id}", get(upgrade))
        .with_state(state)
}

/// Authenticated like `/ws`: bearer token, or the session cookie from an allowed origin
/// (cross-site WebSocket hijacking). Needs VIEW on a page; EDIT makes it writable.
async fn upgrade(
    State(state): State<CollabState>,
    Path(page): Path<Uuid>,
    headers: HeaderMap,
    Authenticated(auth): Authenticated,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    if !headers.contains_key(header::AUTHORIZATION)
        && !origin_allowed(headers.get(header::ORIGIN), &state.allowed_origins)
    {
        return Err(AppError::Forbidden);
    }
    let user = auth.user_id.as_uuid();
    let pool = &state.collab.pool;
    let internal = |err: sqlx::Error| AppError::Internal(Box::new(err));
    if !state
        .permissions
        .check(pool, user, page, Perms::VIEW)
        .await
        .map_err(internal)?
    {
        return Err(AppError::NotFound);
    }
    repo::get_page(pool, page)
        .await
        .map_err(internal)?
        .ok_or(AppError::NotFound)?;
    let editable = state
        .permissions
        .check(pool, user, page, Perms::EDIT)
        .await
        .map_err(internal)?;
    let collab = state.collab.clone();
    let room = collab.room(page).await?;
    let id = collab.next_id.fetch_add(1, Ordering::Relaxed);
    let (close, closed) = watch::channel(false);
    room.members().insert(id, Member { user, close });
    let link = Link {
        collab,
        room,
        page,
        id,
        user,
        editable,
    };
    Ok(ws
        .max_message_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| link.run(socket, closed)))
}

fn origin_allowed(origin: Option<&HeaderValue>, allowed: &[String]) -> bool {
    match origin {
        None => true,
        Some(value) => value
            .to_str()
            .is_ok_and(|origin| allowed.iter().any(|a| a == origin)),
    }
}

/// One connection to a room.
struct Link {
    collab: Collab,
    room: Arc<Room>,
    page: Uuid,
    id: u64,
    user: Uuid,
    editable: bool,
}

/// What to do with one incoming message.
enum Step {
    /// Answer it from the room.
    Handle(Message),
    /// A document update to store before applying (writable connections only).
    Store(Vec<u8>),
    /// A document update from a read-only connection, or an empty one: dropped.
    Skip,
}

impl Link {
    fn classify(&self, message: Message) -> Result<Step, yrs::encoding::read::Error> {
        match message {
            Message::Sync(SyncMessage::SyncStep2(update) | SyncMessage::Update(update)) => {
                // Decoded first, so nothing malformed is ever stored.
                let decoded = Update::decode_v1(&update)?;
                if !self.editable || decoded.is_empty() {
                    Ok(Step::Skip)
                } else {
                    Ok(Step::Store(update))
                }
            }
            other => Ok(Step::Handle(other)),
        }
    }

    async fn run(self, socket: WebSocket, mut closed: watch::Receiver<bool>) {
        self.serve(socket, &mut closed).await;
        self.collab.release(self.page, self.id).await;
    }

    async fn serve(&self, socket: WebSocket, closed: &mut watch::Receiver<bool>) {
        let (mut sink, mut stream) = socket.split();
        let mut updates = self.room.updates.subscribe();
        // Awareness clients announced by this connection, removed when it closes.
        let mut clients: HashSet<ClientID> = HashSet::new();

        let greeting = {
            let awareness = self.room.lock();
            let mut encoder = EncoderV1::new();
            DefaultProtocol
                .start(&awareness, &mut encoder)
                .map(|()| encoder.to_vec())
        };
        let Ok(greeting) = greeting else { return };
        if sink.send(WsMessage::Binary(greeting.into())).await.is_err() {
            return;
        }

        'connection: loop {
            tokio::select! {
                _ = closed.changed() => break,
                incoming = stream.next() => {
                    let data = match incoming {
                        Some(Ok(WsMessage::Binary(data))) => data,
                        Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Text(_))) => continue,
                        Some(Ok(WsMessage::Close(_)) | Err(_)) | None => break,
                    };
                    let mut decoder = DecoderV1::from(data.as_ref());
                    for message in MessageReader::new(&mut decoder) {
                        let Ok(message) = message else { break 'connection };
                        if let Message::Awareness(AwarenessUpdate { clients: entries }) = &message {
                            clients.extend(entries.keys().copied());
                        }
                        let step = match self.classify(message) {
                            Ok(step) => step,
                            Err(err) => {
                                tracing::warn!(error = %err, "collab message rejected");
                                break 'connection;
                            }
                        };
                        let reply = match step {
                            Step::Skip => None,
                            Step::Store(update) => {
                                // Y3: stored before anyone sees it; on failure the client
                                // keeps the change and sends it again after reconnecting.
                                if let Err(err) = self.collab.persist(&self.room, self.page, &update, self.user).await {
                                    tracing::error!(error = %err, "page update not stored");
                                    break 'connection;
                                }
                                let message = Message::Sync(SyncMessage::Update(update));
                                let mut awareness = self.room.lock();
                                Some(DefaultProtocol.handle_message(&mut awareness, message))
                            }
                            Step::Handle(message) => {
                                let mut awareness = self.room.lock();
                                Some(DefaultProtocol.handle_message(&mut awareness, message))
                            }
                        };
                        match reply {
                            None | Some(Ok(None)) => {}
                            Some(Ok(Some(reply))) => {
                                if sink.send(WsMessage::Binary(reply.encode_v1().into())).await.is_err() {
                                    break 'connection;
                                }
                            }
                            Some(Err(err)) => {
                                tracing::warn!(error = %err, "collab message rejected");
                                break 'connection;
                            }
                        }
                    }
                }
                outgoing = updates.recv() => match outgoing {
                    Ok(message) => {
                        if sink.send(WsMessage::Binary(message.as_ref().clone().into())).await.is_err() {
                            break;
                        }
                    }
                    // Too slow: close; the client reconnects and resynchronises.
                    Err(broadcast::error::RecvError::Lagged(_) | broadcast::error::RecvError::Closed) => break,
                },
            }
        }
        let _ = sink.send(WsMessage::Close(None)).await;

        // Live cursors of this connection disappear for everyone else.
        if !clients.is_empty() {
            let mut awareness = self.room.lock();
            for client in clients {
                awareness.remove_state(client);
            }
        }
    }
}

/// Closes collab connections on `AccessRevoked` (V10; call from `app::run`). Rooms live in one
/// process, which is the deployment today (ADR-0021).
pub fn register_revocation(dispatcher: &mut db::outbox::Dispatcher, collab: Collab) {
    dispatcher.subscribe(
        "notebooks.close-revoked-collab",
        move |event: access::AccessRevoked, _| {
            let collab = collab.clone();
            async move {
                collab.revoke(event.item_id, &event.user_ids).await;
                Ok(())
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_check() {
        let allowed = vec!["http://localhost:8081".to_owned()];
        assert!(origin_allowed(None, &allowed));
        assert!(!origin_allowed(
            Some(&HeaderValue::from_static("https://evil.example")),
            &allowed
        ));
    }
}
