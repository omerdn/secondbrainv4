//! `GET /ws`: authentication at the handshake, then the per-connection loop.

use std::time::Duration;

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRef, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::Response;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use identity::http::Authenticated;
use identity::{IdentityService, SessionId, UserId};
use kernel::{AppError, SharedClock};

use crate::access::ItemViewCheck;
use crate::hub::{ConnectionId, Hub, Registered, Subscribe};
use crate::protocol::{Channel, ClientMessage, ErrorCode, ServerMessage};

/// Largest frame accepted from a client; real messages are tiny.
const MAX_MESSAGE_BYTES: usize = 4 * 1024;

/// Close code for a connection that stopped sending heartbeats (4000–4999: application use).
pub const CLOSE_IDLE: u16 = 4000;
/// Close code for a connection the hub dropped as too slow.
pub const CLOSE_SLOW: u16 = 4001;
/// Close code when the connection's session ended (sign-out, revocation, password reset or
/// expiry): reconnecting will not help, the user has to sign in again (T0.5.6).
pub const CLOSE_SESSION_ENDED: u16 = 4003;

/// Timing and origin settings.
#[derive(Debug, Clone)]
pub struct RealtimeConfig {
    /// Browser origins allowed to open a cookie-authenticated connection (the CORS origins).
    pub allowed_origins: Vec<String>,
    /// Client ping interval announced in `hello` (ARCHITECTURE §10.2: 25 s).
    pub heartbeat: Duration,
    /// Silence after which the server closes the connection. Must exceed `heartbeat` plus
    /// the client's own 10 s timeout.
    pub idle_timeout: Duration,
}

impl RealtimeConfig {
    pub const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(25);
    pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self {
            allowed_origins,
            heartbeat: Self::DEFAULT_HEARTBEAT,
            idle_timeout: Self::DEFAULT_IDLE_TIMEOUT,
        }
    }
}

/// Everything the `/ws` route needs.
#[derive(Debug, Clone)]
pub struct RealtimeState {
    pub hub: Hub,
    pub identity: IdentityService,
    /// Who may subscribe to `item:<id>` (VIEW through `PermissionService`).
    pub items: ItemViewCheck,
    pub clock: SharedClock,
    pub config: RealtimeConfig,
}

impl FromRef<RealtimeState> for IdentityService {
    fn from_ref(state: &RealtimeState) -> Self {
        state.identity.clone()
    }
}

/// The `/ws` route with its state applied.
pub fn router(state: RealtimeState) -> Router {
    Router::new().route("/ws", get(upgrade)).with_state(state)
}

/// Authenticates like any API request (bearer token, or the session cookie on web). A
/// cookie-authenticated handshake must come from an allowed origin: browsers attach cookies
/// to cross-site WebSocket handshakes, and CORS does not apply to them (cross-site WebSocket
/// hijacking). Bearer clients (native apps) are not subject to the check.
async fn upgrade(
    State(state): State<RealtimeState>,
    headers: HeaderMap,
    Authenticated(auth): Authenticated,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    if !headers.contains_key(header::AUTHORIZATION)
        && !origin_allowed(headers.get(header::ORIGIN), &state.config.allowed_origins)
    {
        return Err(AppError::Forbidden);
    }
    let (user, session) = (auth.user_id, auth.session_id);
    Ok(ws
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| connection(socket, state, user, session)))
}

/// No `Origin` (non-browser client) or an exactly matching allowed origin.
fn origin_allowed(origin: Option<&HeaderValue>, allowed: &[String]) -> bool {
    match origin {
        None => true,
        Some(value) => value
            .to_str()
            .is_ok_and(|origin| allowed.iter().any(|a| a == origin)),
    }
}

/// Removes the connection from the hub however the loop ends.
struct Registration {
    hub: Hub,
    id: ConnectionId,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.hub.disconnect(self.id);
    }
}

async fn connection(socket: WebSocket, state: RealtimeState, user: UserId, session: SessionId) {
    let (mut sink, mut stream) = socket.split();
    let Registered {
        id,
        mut outbox,
        mut session_ended,
    } = state.hub.connect(user, session);
    let _registration = Registration {
        hub: state.hub.clone(),
        id,
    };
    tracing::debug!(connection = ?id, "realtime connection opened");

    let hello = ServerMessage::Hello {
        server_time: state.clock.now(),
        heartbeat_secs: secs(state.config.heartbeat),
        idle_timeout_secs: secs(state.config.idle_timeout),
    };
    if send(&mut sink, &hello).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            biased;
            // `Err` (sender dropped: removed as too slow) disables this branch; the outbox
            // branch then closes with `CLOSE_SLOW`.
            Ok(()) = &mut session_ended => {
                close(&mut sink, CLOSE_SESSION_ENDED, "session ended").await;
                break;
            }
            incoming = tokio::time::timeout(state.config.idle_timeout, stream.next()) => {
                let message = match incoming {
                    Err(_) => {
                        close(&mut sink, CLOSE_IDLE, "idle timeout").await;
                        break;
                    }
                    Ok(None | Some(Err(_))) => break,
                    Ok(Some(Ok(message))) => message,
                };
                // Heartbeats double as a session check (expiry, or a revocation whose
                // notification was missed): every `heartbeat` seconds, never per second (BP5).
                if let Message::Text(text) = &message
                    && is_ping(text.as_str())
                    && !session_still_active(&state.identity, session).await
                {
                    close(&mut sink, CLOSE_SESSION_ENDED, "session ended").await;
                    break;
                }
                let reply = match message {
                    Message::Text(text) => handle(&state, id, user, text.as_str()).await,
                    Message::Binary(_) => error(ErrorCode::BadRequest, "binary frames are not supported", None),
                    Message::Close(_) => break,
                    // Protocol-level ping/pong is answered by the WebSocket layer; it also
                    // counts as activity for the idle timeout.
                    Message::Ping(_) | Message::Pong(_) => continue,
                };
                if send(&mut sink, &reply).await.is_err() {
                    break;
                }
            }
            outgoing = outbox.recv() => match outgoing {
                Some(message) => {
                    if send(&mut sink, &message).await.is_err() {
                        break;
                    }
                }
                None => {
                    close(&mut sink, CLOSE_SLOW, "too slow").await;
                    break;
                }
            },
        }
    }
    tracing::debug!(connection = ?id, "realtime connection closed");
}

fn is_ping(text: &str) -> bool {
    matches!(serde_json::from_str(text), Ok(ClientMessage::Ping))
}

/// A database error keeps the connection: it is not evidence that the session ended.
async fn session_still_active(identity: &IdentityService, session: SessionId) -> bool {
    match identity.session_active(session).await {
        Ok(active) => active,
        Err(err) => {
            tracing::warn!(error = %err, "realtime session check failed");
            true
        }
    }
}

/// Answers one client message.
async fn handle(
    state: &RealtimeState,
    id: ConnectionId,
    user: UserId,
    text: &str,
) -> ServerMessage {
    let Ok(message) = serde_json::from_str::<ClientMessage>(text) else {
        return error(ErrorCode::BadRequest, "not a valid client message", None);
    };
    match message {
        ClientMessage::Ping => ServerMessage::Pong {
            server_time: state.clock.now(),
        },
        ClientMessage::Subscribe { channel: name } => {
            let Ok(channel) = name.parse::<Channel>() else {
                return error(ErrorCode::BadRequest, "unknown channel", Some(name));
            };
            if !may_subscribe(state, user, &channel).await {
                return error(ErrorCode::Forbidden, "not allowed", Some(name));
            }
            match state.hub.subscribe(id, channel) {
                Subscribe::Added | Subscribe::AlreadySubscribed | Subscribe::Closed => {
                    ServerMessage::Subscribed { channel: name }
                }
                Subscribe::LimitReached => error(
                    ErrorCode::TooManySubscriptions,
                    "subscription limit reached",
                    Some(name),
                ),
            }
        }
        ClientMessage::Unsubscribe { channel: name } => {
            if let Ok(channel) = name.parse::<Channel>() {
                state.hub.unsubscribe(id, &channel);
            }
            ServerMessage::Unsubscribed {
                channel: name,
                reason: None,
            }
        }
    }
}

/// Who may receive a channel. A user channel belongs to that user only; an item channel needs
/// VIEW on the item (`PermissionService`).
async fn may_subscribe(state: &RealtimeState, user: UserId, channel: &Channel) -> bool {
    match channel {
        Channel::User(owner) => *owner == user,
        Channel::Item(item) => state.items.can_view(user, *item).await,
    }
}

fn error(code: ErrorCode, message: &str, channel: Option<String>) -> ServerMessage {
    ServerMessage::Error {
        code,
        message: message.to_owned(),
        channel,
    }
}

fn secs(duration: Duration) -> u32 {
    u32::try_from(duration.as_secs()).unwrap_or(u32::MAX)
}

async fn send<S>(sink: &mut S, message: &ServerMessage) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    let Ok(text) = serde_json::to_string(message) else {
        return Err(());
    };
    sink.send(Message::Text(text.into())).await.map_err(|_| ())
}

async fn close<S>(sink: &mut S, code: u16, reason: &'static str)
where
    S: SinkExt<Message> + Unpin,
{
    let _ = sink
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_check() {
        let allowed = vec!["http://localhost:8081".to_owned()];
        assert!(origin_allowed(None, &allowed));
        assert!(origin_allowed(
            Some(&HeaderValue::from_static("http://localhost:8081")),
            &allowed
        ));
        assert!(!origin_allowed(
            Some(&HeaderValue::from_static("https://evil.example")),
            &allowed
        ));
        assert!(!origin_allowed(
            Some(&HeaderValue::from_static(
                "http://localhost:8081.evil.example"
            )),
            &allowed
        ));
    }

    #[test]
    fn channel_names_round_trip() {
        let user = UserId::new();
        let item = uuid::Uuid::now_v7();
        for channel in [Channel::User(user), Channel::Item(item)] {
            assert_eq!(channel.to_string().parse::<Channel>(), Ok(channel));
        }
        assert!("item:nope".parse::<Channel>().is_err());
        assert!("team:x".parse::<Channel>().is_err());
    }
}
