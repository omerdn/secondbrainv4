//! Messages on `/ws` (ARCHITECTURE §10.1). JSON text frames, tagged by `type`.
//!
//! The channel only says *what changed*; clients refetch data over HTTP. Payloads therefore
//! carry ids, never content (Work Session timers are the documented exception, added later).

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use identity::UserId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A stream of events a connection can subscribe to.
///
/// String form `kind:id`: `user:<own id>` and `item:<id>`. Item channels are authorised through
/// `PermissionService` (VIEW) and end when the access does (V10). Team channels arrive later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Events for one user (notifications, own objects), e.g. `user:0193…`.
    User(UserId),
    /// Changes to one item (task, page, …), e.g. `item:0193…`.
    Item(uuid::Uuid),
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(id) => write!(f, "user:{id}"),
            Self::Item(id) => write!(f, "item:{id}"),
        }
    }
}

/// Why a channel name was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidChannel;

impl FromStr for Channel {
    type Err = InvalidChannel;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some(("user", id)) => id.parse().map(Self::User).map_err(|_| InvalidChannel),
            Some(("item", id)) => id.parse().map(Self::Item).map_err(|_| InvalidChannel),
            _ => Err(InvalidChannel),
        }
    }
}

/// Client → server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ClientMessage {
    /// Start receiving a channel's events. Answered with `subscribed` or `error`.
    Subscribe { channel: String },
    /// Stop receiving a channel's events. Always answered with `unsubscribed`.
    Unsubscribe { channel: String },
    /// Liveness check (every `heartbeat_secs`). Answered with `pong`. Application-level
    /// because browsers cannot send WebSocket ping frames.
    Ping,
}

/// An event on a channel: a name such as `item.changed` and a small JSON payload of ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "WsEvent")]
pub struct Event {
    pub name: String,
    #[ts(type = "unknown")]
    pub data: serde_json::Value,
}

/// Machine-readable reason in an `error` message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "WsErrorCode")]
pub enum ErrorCode {
    /// Not JSON, an unknown message type, a binary frame or an unknown channel name.
    BadRequest,
    /// The user may not subscribe to that channel.
    Forbidden,
    /// The per-connection subscription limit is reached.
    TooManySubscriptions,
}

/// Why the server ended a subscription the client did not cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "WsUnsubscribeReason")]
pub enum UnsubscribeReason {
    /// The user can no longer view the item (V10): drop it from the screen.
    AccessRevoked,
}

/// Server → client.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ServerMessage {
    /// First message on every connection. Clients compute their clock offset from
    /// `server_time` (Work Session timers, ARCHITECTURE §11).
    Hello {
        server_time: DateTime<Utc>,
        /// How often the client should send `ping`.
        heartbeat_secs: u32,
        /// The server closes a connection that sent nothing for this long.
        idle_timeout_secs: u32,
    },
    Pong {
        server_time: DateTime<Utc>,
    },
    Subscribed {
        channel: String,
    },
    Unsubscribed {
        channel: String,
        /// Set when the server ended the subscription; absent for the client's own `unsubscribe`.
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<UnsubscribeReason>,
    },
    Event {
        channel: String,
        event: Event,
    },
    Error {
        code: ErrorCode,
        /// Developer-facing English description.
        message: String,
        /// The channel the failed request named, if any.
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_round_trip() {
        let id = UserId::new();
        let channel = Channel::User(id);
        assert_eq!(channel.to_string(), format!("user:{id}"));
        assert_eq!(channel.to_string().parse::<Channel>(), Ok(channel));
    }

    #[test]
    fn unknown_or_malformed_channels_are_rejected() {
        for name in [
            "",
            "user",
            "user:",
            "user:not-a-uuid",
            "team:x",
            "item:1",
            "user",
        ] {
            assert_eq!(name.parse::<Channel>(), Err(InvalidChannel), "{name}");
        }
    }

    #[test]
    fn client_messages_are_tagged() {
        let parsed: ClientMessage =
            serde_json::from_str(r#"{"type":"subscribe","channel":"user:x"}"#)
                .unwrap_or(ClientMessage::Ping);
        assert_eq!(
            parsed,
            ClientMessage::Subscribe {
                channel: "user:x".into()
            }
        );
        assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"nope"}"#).is_err());
    }

    #[test]
    fn server_messages_are_tagged() {
        let json = serde_json::to_value(ServerMessage::Error {
            code: ErrorCode::TooManySubscriptions,
            message: "m".into(),
            channel: None,
        })
        .unwrap_or_default();
        assert_eq!(
            json,
            serde_json::json!({"type": "error", "code": "too_many_subscriptions", "message": "m"})
        );
    }
}
