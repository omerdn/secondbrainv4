//! Collaborative page editing over y-sync (spike T0.6.2, ADR-0005, ARCHITECTURE §9).
//!
//! `GET /collab/{page_id}` upgrades to a WebSocket that speaks the y-sync protocol in binary
//! frames, exactly what Yjs' `y-websocket` provider expects: sync step 1/2, updates and
//! awareness (live cursors). The server keeps one [`yrs`] document per page in memory and
//! fans out every document and awareness change to all connections of that page.
//!
//! **Spike limits** (resolved in E2.5): no page permission check (VIEW/EDIT), no persistence
//! (`page_updates`, snapshots), rooms live until the process exits. The route is therefore only
//! mounted when explicitly enabled for development (`COLLAB_SPIKE_ENABLED`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use axum::Router;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRef, Path, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::Response;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use tokio::sync::broadcast;
use uuid::Uuid;
use yrs::sync::{Awareness, AwarenessUpdate, DefaultProtocol, Message, Protocol, SyncMessage};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{ClientID, Doc};

/// Largest frame accepted from a client. Large pastes arrive as a single update.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Messages buffered per connection before it is dropped as too slow (it reconnects and
/// resynchronises with sync step 1).
const BROADCAST_CAPACITY: usize = 256;

/// One page being edited: its document, awareness and the fan-out channel.
struct Room {
    awareness: Mutex<Awareness>,
    updates: broadcast::Sender<Arc<Vec<u8>>>,
}

impl Room {
    fn new() -> Arc<Self> {
        let (updates, _) = broadcast::channel(BROADCAST_CAPACITY);
        let doc = Doc::new();
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
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Awareness> {
        self.awareness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Rooms by page id. Cheap to clone.
#[derive(Clone, Default)]
pub struct Collab {
    rooms: Arc<Mutex<HashMap<Uuid, Arc<Room>>>>,
}

impl std::fmt::Debug for Collab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collab").finish_non_exhaustive()
    }
}

impl Collab {
    pub fn new() -> Self {
        Self::default()
    }

    fn room(&self, page: Uuid) -> Arc<Room> {
        self.rooms
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(page)
            .or_insert_with(Room::new)
            .clone()
    }

    /// The page's current content as a Yjs update (tests and future persistence).
    pub fn snapshot(&self, page: Uuid) -> Vec<u8> {
        use yrs::{ReadTxn, StateVector, Transact};
        let room = self.room(page);
        let awareness = room.lock();
        let txn = awareness.doc().transact();
        txn.encode_state_as_update_v1(&StateVector::default())
    }
}

/// Everything the route needs.
#[derive(Debug, Clone)]
pub struct CollabState {
    pub collab: Collab,
    pub identity: IdentityService,
    /// Browser origins allowed to open a cookie-authenticated connection (the CORS origins).
    pub allowed_origins: Vec<String>,
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
/// (cross-site WebSocket hijacking).
async fn upgrade(
    State(state): State<CollabState>,
    Path(page): Path<Uuid>,
    headers: HeaderMap,
    Authenticated(_auth): Authenticated,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    if !headers.contains_key(header::AUTHORIZATION)
        && !origin_allowed(headers.get(header::ORIGIN), &state.allowed_origins)
    {
        return Err(AppError::Forbidden);
    }
    let room = state.collab.room(page);
    Ok(ws
        .max_message_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| connection(socket, room)))
}

fn origin_allowed(origin: Option<&HeaderValue>, allowed: &[String]) -> bool {
    match origin {
        None => true,
        Some(value) => value
            .to_str()
            .is_ok_and(|origin| allowed.iter().any(|a| a == origin)),
    }
}

async fn connection(socket: WebSocket, room: Arc<Room>) {
    let (mut sink, mut stream) = socket.split();
    let mut updates = room.updates.subscribe();
    // Awareness clients announced by this connection, removed when it closes.
    let mut clients: HashSet<ClientID> = HashSet::new();

    let greeting = {
        let awareness = room.lock();
        let mut encoder = EncoderV1::new();
        DefaultProtocol
            .start(&awareness, &mut encoder)
            .map(|()| encoder.to_vec())
    };
    let Ok(greeting) = greeting else { return };
    if sink.send(WsMessage::Binary(greeting.into())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let data = match incoming {
                    Some(Ok(WsMessage::Binary(data))) => data,
                    Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Text(_))) => continue,
                    Some(Ok(WsMessage::Close(_)) | Err(_)) | None => break,
                };
                remember_awareness_clients(&data, &mut clients);
                let replies = {
                    let mut awareness = room.lock();
                    DefaultProtocol.handle(&mut awareness, &data)
                };
                match replies {
                    Ok(replies) => {
                        for reply in replies {
                            if sink.send(WsMessage::Binary(reply.encode_v1().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "collab message rejected");
                        break;
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
                Err(broadcast::error::RecvError::Lagged(_)) => break,
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }

    // Live cursors of this connection disappear for everyone else.
    if !clients.is_empty() {
        let mut awareness = room.lock();
        for client in clients {
            awareness.remove_state(client);
        }
    }
}

/// Records which awareness client ids a connection speaks for.
fn remember_awareness_clients(data: &[u8], clients: &mut HashSet<ClientID>) {
    use yrs::sync::MessageReader;
    use yrs::updates::decoder::DecoderV1;
    let mut decoder = DecoderV1::from(data);
    for message in MessageReader::new(&mut decoder).flatten() {
        if let Message::Awareness(AwarenessUpdate { clients: entries }) = message {
            clients.extend(entries.keys().copied());
        }
    }
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
