//! `/ws` end to end (T0.5.1) against a real Postgres 17 for authentication.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use db::testing::TestDb;
use futures_util::{SinkExt, StreamExt};
use identity::model::{ClientKind, RegisterRequest};
use identity::service::SESSION_TTL;
use identity::{EmailSettings, IdentityService, UserId};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use realtime::{Channel, Event, Hub, RealtimeConfig, RealtimeState};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const WEB: &str = "http://localhost:8081";
const WITHIN: Duration = Duration::from_secs(5);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 22, 9, 30, 0).unwrap()
}

struct Server {
    addr: SocketAddr,
    hub: Hub,
    identity: IdentityService,
    clock: FixedClock,
    _db: TestDb,
}

async fn server_with(idle_timeout: Duration) -> Server {
    let db = TestDb::new().await;
    let fixed = FixedClock::new(now());
    let clock = Arc::new(fixed.clone());
    let identity = IdentityService::new(
        db.pool().clone(),
        clock.clone(),
        EmailSettings {
            sender: Arc::new(RecordingEmailSender::default()),
            web_url: WEB.into(),
        },
    );
    let hub = Hub::new();
    let mut config = RealtimeConfig::new(vec![WEB.to_owned()]);
    config.idle_timeout = idle_timeout;
    let app = realtime::router(RealtimeState {
        hub: hub.clone(),
        identity: identity.clone(),
        items: realtime::ItemViewCheck::deny_all(),
        clock,
        config,
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server {
        addr,
        hub,
        identity,
        clock: fixed,
        _db: db,
    }
}

async fn server() -> Server {
    server_with(RealtimeConfig::DEFAULT_IDLE_TIMEOUT).await
}

/// Registers a user; returns its id and session token.
async fn user(identity: &IdentityService, email: &str) -> (UserId, String) {
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
    (user.id, session.token.expose().to_owned())
}

async fn connect(
    addr: SocketAddr,
    headers: &[(&'static str, String)],
) -> Result<Socket, StatusCode> {
    let mut request = format!("ws://{addr}/ws").into_client_request().unwrap();
    for (name, value) in headers {
        request
            .headers_mut()
            .insert(*name, HeaderValue::from_str(value).unwrap());
    }
    match timeout(WITHIN, tokio_tungstenite::connect_async(request))
        .await
        .unwrap()
    {
        Ok((socket, _)) => Ok(socket),
        Err(WsError::Http(response)) => Err(response.status()),
        Err(other) => panic!("unexpected handshake error: {other}"),
    }
}

async fn bearer(addr: SocketAddr, token: &str) -> Socket {
    connect(addr, &[("authorization", format!("Bearer {token}"))])
        .await
        .unwrap()
}

/// Next text message as JSON.
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

async fn send(socket: &mut Socket, message: Value) {
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .unwrap();
}

/// Connects with a bearer token and consumes `hello`.
async fn ready(addr: SocketAddr, token: &str) -> Socket {
    let mut socket = bearer(addr, token).await;
    assert_eq!(recv(&mut socket).await["type"], "hello");
    socket
}

fn event() -> Event {
    Event {
        name: "item.changed".into(),
        data: json!({"id": "0193", "kind": "task"}),
    }
}

#[tokio::test]
async fn handshake_requires_a_session() {
    let server = server().await;
    assert_eq!(
        connect(server.addr, &[]).await.err(),
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        connect(server.addr, &[("authorization", "Bearer nope".into())])
            .await
            .err(),
        Some(StatusCode::UNAUTHORIZED)
    );
}

#[tokio::test]
async fn hello_carries_the_server_clock_and_heartbeat() {
    let server = server().await;
    let (_, token) = user(&server.identity, "ada@example.com").await;
    let mut socket = bearer(server.addr, &token).await;
    assert_eq!(
        recv(&mut socket).await,
        json!({
            "type": "hello",
            "server_time": "2026-11-22T09:30:00Z",
            "heartbeat_secs": 25,
            "idle_timeout_secs": 60,
        })
    );
}

#[tokio::test]
async fn ping_is_answered_with_the_server_time() {
    let server = server().await;
    let (_, token) = user(&server.identity, "ada@example.com").await;
    let mut socket = ready(server.addr, &token).await;
    send(&mut socket, json!({"type": "ping"})).await;
    assert_eq!(
        recv(&mut socket).await,
        json!({"type": "pong", "server_time": "2026-11-22T09:30:00Z"})
    );
}

#[tokio::test]
async fn subscribed_channels_receive_events_until_unsubscribed() {
    let server = server().await;
    let (id, token) = user(&server.identity, "ada@example.com").await;
    let channel = format!("user:{id}");
    let mut socket = ready(server.addr, &token).await;

    send(
        &mut socket,
        json!({"type": "subscribe", "channel": channel}),
    )
    .await;
    assert_eq!(
        recv(&mut socket).await,
        json!({"type": "subscribed", "channel": channel})
    );
    assert_eq!(server.hub.publish(&Channel::User(id), &event()), 1);
    assert_eq!(
        recv(&mut socket).await,
        json!({
            "type": "event",
            "channel": channel,
            "event": {"name": "item.changed", "data": {"id": "0193", "kind": "task"}},
        })
    );

    send(
        &mut socket,
        json!({"type": "unsubscribe", "channel": channel}),
    )
    .await;
    assert_eq!(
        recv(&mut socket).await,
        json!({"type": "unsubscribed", "channel": channel})
    );
    assert_eq!(server.hub.publish(&Channel::User(id), &event()), 0);
}

#[tokio::test]
async fn another_users_channel_is_forbidden() {
    let server = server().await;
    let (_, token) = user(&server.identity, "ada@example.com").await;
    let (other, _) = user(&server.identity, "bob@example.com").await;
    let mut socket = ready(server.addr, &token).await;
    let channel = format!("user:{other}");
    send(
        &mut socket,
        json!({"type": "subscribe", "channel": channel}),
    )
    .await;
    let reply = recv(&mut socket).await;
    assert_eq!(reply["type"], "error");
    assert_eq!(reply["code"], "forbidden");
    assert_eq!(reply["channel"], channel.as_str());
    assert_eq!(server.hub.publish(&Channel::User(other), &event()), 0);
}

#[tokio::test]
async fn malformed_messages_get_bad_request_and_keep_the_connection() {
    let server = server().await;
    let (_, token) = user(&server.identity, "ada@example.com").await;
    let mut socket = ready(server.addr, &token).await;

    for text in [
        "not json".to_owned(),
        json!({"type": "nope"}).to_string(),
        json!({"type": "subscribe", "channel": "team:1"}).to_string(),
    ] {
        socket.send(Message::Text(text.into())).await.unwrap();
        let reply = recv(&mut socket).await;
        assert_eq!(reply["type"], "error");
        assert_eq!(reply["code"], "bad_request");
    }
    socket
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    assert_eq!(recv(&mut socket).await["code"], "bad_request");

    send(&mut socket, json!({"type": "ping"})).await;
    assert_eq!(recv(&mut socket).await["type"], "pong");
}

#[tokio::test]
async fn cookie_handshakes_must_come_from_an_allowed_origin() {
    let server = server().await;
    let (_, token) = user(&server.identity, "ada@example.com").await;
    let cookie = ("cookie", format!("sb_session={token}"));

    assert!(
        connect(server.addr, &[cookie.clone(), ("origin", WEB.into())])
            .await
            .is_ok()
    );
    // Cross-site WebSocket hijacking: a foreign page's handshake carries the cookie too.
    assert_eq!(
        connect(
            server.addr,
            &[cookie.clone(), ("origin", "https://evil.example".into())]
        )
        .await
        .err(),
        Some(StatusCode::FORBIDDEN)
    );
    // Non-browser clients send no Origin.
    assert!(connect(server.addr, &[cookie]).await.is_ok());
    // Bearer clients are not checked (React Native sends its own Origin).
    assert!(
        connect(
            server.addr,
            &[
                ("authorization", format!("Bearer {token}")),
                ("origin", "http://localhost:8080".into())
            ]
        )
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn silent_connections_are_closed() {
    let server = server_with(Duration::from_millis(300)).await;
    let (_, token) = user(&server.identity, "ada@example.com").await;
    let mut socket = bearer(server.addr, &token).await;
    let hello = recv(&mut socket).await;
    assert_eq!(hello["idle_timeout_secs"], 0);
    match timeout(WITHIN, socket.next()).await.unwrap() {
        Some(Ok(Message::Close(Some(frame)))) => {
            assert_eq!(frame.code, CloseCode::from(4000));
            assert_eq!(frame.reason.as_str(), "idle timeout");
        }
        other => panic!("expected a close frame, got {other:?}"),
    }
}

#[tokio::test]
async fn heartbeats_keep_a_connection_open() {
    let server = server_with(Duration::from_millis(400)).await;
    let (_, token) = user(&server.identity, "ada@example.com").await;
    let mut socket = ready(server.addr, &token).await;
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        send(&mut socket, json!({"type": "ping"})).await;
        assert_eq!(recv(&mut socket).await["type"], "pong");
    }
}

#[tokio::test]
async fn closed_connections_leave_the_hub() {
    let server = server().await;
    let (id, token) = user(&server.identity, "ada@example.com").await;
    let mut socket = ready(server.addr, &token).await;
    send(
        &mut socket,
        json!({"type": "subscribe", "channel": format!("user:{id}")}),
    )
    .await;
    recv(&mut socket).await;
    assert_eq!(server.hub.connection_count(), 1);
    socket.close(None).await.unwrap();
    timeout(WITHIN, async {
        while server.hub.connection_count() > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(server.hub.publish(&Channel::User(id), &event()), 0);
}

/// Next frame must be a close frame with `code`.
async fn closed_with(socket: &mut Socket, code: u16) {
    loop {
        match timeout(WITHIN, socket.next()).await.unwrap() {
            Some(Ok(Message::Close(Some(frame)))) => {
                assert_eq!(frame.code, CloseCode::from(code));
                return;
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            other => panic!("expected a close frame, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_revoked_session_closes_its_connections_only() {
    let server = server().await;
    let (id, token) = user(&server.identity, "ada@example.com").await;
    let other_token = server
        .identity
        .login(
            identity::model::LoginRequest {
                email: "ada@example.com".into(),
                password: "correct horse".into(),
                client: ClientKind::Native,
                device: None,
            }
            .validate()
            .unwrap(),
        )
        .await
        .unwrap()
        .1
        .token
        .expose()
        .to_owned();
    let mut revoked = ready(server.addr, &token).await;
    let mut kept = ready(server.addr, &other_token).await;

    let session = server
        .identity
        .authenticate(&token)
        .await
        .unwrap()
        .session_id;
    server.identity.revoke_session(id, session).await.unwrap();
    // The outbox handler forwards this through NOTIFY; the hub is what it reaches.
    assert_eq!(server.hub.end_sessions(id, Some(session)), 1);
    closed_with(&mut revoked, realtime::CLOSE_SESSION_ENDED).await;

    send(&mut kept, json!({"type": "ping"})).await;
    assert_eq!(recv(&mut kept).await["type"], "pong");
}

#[tokio::test]
async fn a_ping_after_the_session_expired_closes_the_connection() {
    let server = server().await;
    let (_, token) = user(&server.identity, "ada@example.com").await;
    let mut socket = ready(server.addr, &token).await;
    send(&mut socket, json!({"type": "ping"})).await;
    assert_eq!(recv(&mut socket).await["type"], "pong");

    server.clock.advance(SESSION_TTL);
    send(&mut socket, json!({"type": "ping"})).await;
    closed_with(&mut socket, realtime::CLOSE_SESSION_ENDED).await;
}

#[tokio::test]
async fn a_ping_after_revocation_closes_the_connection_even_without_the_notification() {
    let server = server().await;
    let (id, token) = user(&server.identity, "ada@example.com").await;
    let mut socket = ready(server.addr, &token).await;
    let session = server
        .identity
        .authenticate(&token)
        .await
        .unwrap()
        .session_id;
    server.identity.revoke_session(id, session).await.unwrap();
    send(&mut socket, json!({"type": "ping"})).await;
    closed_with(&mut socket, realtime::CLOSE_SESSION_ENDED).await;
}
