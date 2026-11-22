//! Fan-out across server instances through Postgres `LISTEN/NOTIFY` (T0.5.2, ADR-0009).
//!
//! Modules never call [`Hub::publish`] directly. They publish through a [`Notifier`], which
//! sends a `NOTIFY` on one database channel; every instance (the sender included) runs a
//! [`Bridge`] that `LISTEN`s on it and hands each event to its local [`Hub`]. So an event
//! reaches every connection, whichever instance holds it.
//!
//! A `NOTIFY` sent inside a transaction is delivered only when it commits (and dropped on
//! rollback), so publishing in the same transaction as the change never announces a change
//! that did not happen.
//!
//! Delivery is best effort: notifications sent while a listener's connection is down are lost.
//! That is acceptable because the channel only says *what changed*, and clients refetch
//! everything after a reconnect (ARCHITECTURE §10.2).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgListener, PgPool};
use sqlx::{Error as SqlxError, PgExecutor};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use identity::{SessionId, UserId};
use uuid::Uuid;

use crate::hub::Hub;
use crate::protocol::{Channel, Event};

/// Database notification channel used in production.
pub const DEFAULT_NOTIFY_CHANNEL: &str = "realtime";

/// Largest payload accepted. Postgres rejects payloads of 8000 bytes or more; events carry
/// ids only, so anything near this is a bug.
pub const MAX_PAYLOAD_BYTES: usize = 7_900;

/// Wait before retrying after the listener failed to get a connection.
const RETRY_DELAY: Duration = Duration::from_secs(1);

/// What travels in a notification payload.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Envelope {
    /// An event for a channel's subscribers.
    Event { channel: String, event: Event },
    /// Close the connections of ended sessions (`session: None`: all of the user's).
    SessionsEnded {
        user: UserId,
        session: Option<SessionId>,
    },
    /// End these users' subscriptions to an item they can no longer view (V10).
    AccessRevoked { item: Uuid, users: Vec<UserId> },
}

/// Users per `AccessRevoked` notification, keeping payloads far below [`MAX_PAYLOAD_BYTES`].
const REVOKED_USERS_PER_NOTIFICATION: usize = 100;

/// Why an event could not be published.
#[derive(Debug)]
pub enum PublishError {
    /// The serialized event exceeds [`MAX_PAYLOAD_BYTES`].
    TooLarge {
        bytes: usize,
    },
    Serialize(serde_json::Error),
    Database(SqlxError),
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(
                f,
                "realtime event payload is {bytes} bytes (limit {MAX_PAYLOAD_BYTES})"
            ),
            Self::Serialize(err) => write!(f, "realtime event could not be serialized: {err}"),
            Self::Database(err) => write!(f, "realtime event could not be sent: {err}"),
        }
    }
}

impl std::error::Error for PublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TooLarge { .. } => None,
            Self::Serialize(err) => Some(err),
            Self::Database(err) => Some(err),
        }
    }
}

impl From<PublishError> for kernel::AppError {
    fn from(err: PublishError) -> Self {
        Self::internal(err)
    }
}

/// Publishes events to every instance. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Notifier {
    notify_channel: Arc<str>,
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new(DEFAULT_NOTIFY_CHANNEL)
    }
}

impl Notifier {
    /// A notifier on `notify_channel`; it must match the [`Bridge`]s' channel.
    pub fn new(notify_channel: impl Into<Arc<str>>) -> Self {
        Self {
            notify_channel: notify_channel.into(),
        }
    }

    /// Sends `event` on `channel` to all instances. Pass the pool to send now, or
    /// `&mut *tx` to send when the transaction commits.
    pub async fn publish<'e, E>(
        &self,
        executor: E,
        channel: &Channel,
        event: &Event,
    ) -> Result<(), PublishError>
    where
        E: PgExecutor<'e>,
    {
        self.send(
            executor,
            &Envelope::Event {
                channel: channel.to_string(),
                event: event.clone(),
            },
        )
        .await
    }

    /// Closes, on every instance, the connections of `user` authenticated by `session`
    /// (`None`: all of the user's sessions) with [`crate::ws::CLOSE_SESSION_ENDED`].
    pub async fn end_sessions<'e, E>(
        &self,
        executor: E,
        user: UserId,
        session: Option<SessionId>,
    ) -> Result<(), PublishError>
    where
        E: PgExecutor<'e>,
    {
        self.send(executor, &Envelope::SessionsEnded { user, session })
            .await
    }

    /// Ends, on every instance, the subscriptions of `users` to `item`'s channel (V10). Large
    /// user lists are split over several notifications.
    pub async fn revoke_access(
        &self,
        pool: &PgPool,
        item: Uuid,
        users: &[UserId],
    ) -> Result<(), PublishError> {
        for chunk in users.chunks(REVOKED_USERS_PER_NOTIFICATION) {
            let envelope = Envelope::AccessRevoked {
                item,
                users: chunk.to_vec(),
            };
            self.send(pool, &envelope).await?;
        }
        Ok(())
    }

    async fn send<'e, E>(&self, executor: E, envelope: &Envelope) -> Result<(), PublishError>
    where
        E: PgExecutor<'e>,
    {
        let payload = encode(envelope)?;
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(&*self.notify_channel)
            .bind(payload)
            .execute(executor)
            .await
            .map_err(PublishError::Database)?;
        Ok(())
    }
}

fn encode(envelope: &Envelope) -> Result<String, PublishError> {
    let payload = serde_json::to_string(envelope).map_err(PublishError::Serialize)?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(PublishError::TooLarge {
            bytes: payload.len(),
        });
    }
    Ok(payload)
}

/// A running listener that feeds one instance's [`Hub`].
#[derive(Debug)]
pub struct Bridge {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Bridge {
    /// Starts listening on `notify_channel`. Returns once `LISTEN` is active, so events
    /// published afterwards are not missed. Holds one pool connection while running.
    pub async fn start(pool: &PgPool, notify_channel: &str, hub: Hub) -> Result<Self, SqlxError> {
        let mut listener = PgListener::connect_with(pool).await?;
        listener.listen(notify_channel).await?;
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(run(listener, hub, stopped));
        Ok(Self { stop, task })
    }

    /// Stops listening and waits for the task to end. Call before closing the pool.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        if self.task.await.is_err() {
            tracing::error!("realtime bridge panicked");
        }
    }
}

async fn run(mut listener: PgListener, hub: Hub, mut stopped: oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut stopped => break,
            received = listener.try_recv() => match received {
                Ok(Some(notification)) => deliver(&hub, notification.payload()),
                // Reconnected and listening again; anything sent meanwhile is lost.
                Ok(None) => tracing::warn!("realtime bridge reconnected; notifications may have been lost"),
                Err(SqlxError::PoolClosed) => break,
                Err(err) => {
                    tracing::warn!(error = %err, "realtime bridge cannot reach the database; retrying");
                    tokio::select! {
                        _ = &mut stopped => break,
                        () = tokio::time::sleep(RETRY_DELAY) => {}
                    }
                }
            },
        }
    }
}

fn deliver(hub: &Hub, payload: &str) {
    match serde_json::from_str::<Envelope>(payload) {
        Ok(Envelope::Event { channel, event }) => match channel.parse::<Channel>() {
            Ok(channel) => {
                hub.publish(&channel, &event);
            }
            Err(_) => tracing::warn!("realtime bridge ignored an event for an unknown channel"),
        },
        Ok(Envelope::SessionsEnded { user, session }) => {
            hub.end_sessions(user, session);
        }
        Ok(Envelope::AccessRevoked { item, users }) => {
            hub.revoke_item(item, &users);
        }
        // The payload is not logged: it is not ours to trust and may be arbitrary.
        Err(_) => tracing::warn!("realtime bridge ignored a malformed notification"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payloads_are_tagged_and_round_trip() {
        let user = UserId::new();
        let payload = encode(&Envelope::Event {
            channel: Channel::User(user).to_string(),
            event: Event {
                name: "item.changed".into(),
                data: json!({"id": "0193", "kind": "task"}),
            },
        })
        .unwrap_or_default();
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();
        assert_eq!(value["kind"], "event");
        assert!(matches!(
            serde_json::from_str::<Envelope>(&payload),
            Ok(Envelope::Event { .. })
        ));

        let payload = encode(&Envelope::SessionsEnded {
            user,
            session: None,
        })
        .unwrap_or_default();
        assert!(matches!(
            serde_json::from_str::<Envelope>(&payload),
            Ok(Envelope::SessionsEnded { session: None, .. })
        ));
    }

    #[test]
    fn oversized_events_are_rejected() {
        let envelope = Envelope::Event {
            channel: Channel::User(UserId::new()).to_string(),
            event: Event {
                name: "item.changed".into(),
                data: json!({"ids": "x".repeat(MAX_PAYLOAD_BYTES)}),
            },
        };
        assert!(matches!(
            encode(&envelope),
            Err(PublishError::TooLarge { .. })
        ));
    }

    #[test]
    fn malformed_payloads_change_nothing() {
        let hub = Hub::new();
        let user = UserId::new();
        let mut connection = hub.connect(user, SessionId::new());
        hub.subscribe(connection.id, Channel::User(user));
        for payload in [
            "",
            "not json",
            r#"{"kind":"event","channel":"team:1","event":{"name":"x","data":null}}"#,
            r#"{"kind":"event","channel":"user:not-a-uuid","event":{"name":"x","data":null}}"#,
            r#"{"kind":"sessions_ended","user":"nope","session":null}"#,
            r#"{"channel":"x","event":{"name":"x","data":null}}"#,
        ] {
            deliver(&hub, payload);
        }
        assert!(connection.outbox.try_recv().is_err());
        assert!(connection.session_ended.try_recv().is_err());
        assert_eq!(hub.connection_count(), 1);
    }
}
