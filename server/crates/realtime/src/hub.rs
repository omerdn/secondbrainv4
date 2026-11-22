//! In-process registry of open connections and their channel subscriptions.
//!
//! One [`Hub`] per server instance. It only reaches this instance's connections; modules
//! publish through [`crate::Notifier`] and the [`crate::Bridge`] calls [`Hub::publish`].

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, PoisonError, RwLock};

use identity::{SessionId, UserId};
use tokio::sync::{mpsc, oneshot};

use crate::protocol::{Channel, Event, ServerMessage};

/// Messages buffered per connection before it counts as too slow and is dropped.
pub const OUTBOX_CAPACITY: usize = 64;

/// Subscriptions one connection may hold.
pub const MAX_SUBSCRIPTIONS: usize = 100;

/// Identifies one open connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(u64);

#[derive(Debug)]
struct Connection {
    user: UserId,
    session: SessionId,
    outbox: mpsc::Sender<ServerMessage>,
    /// Fired when the connection's session ended (T0.5.6).
    ended: Option<oneshot::Sender<()>>,
    channels: HashSet<Channel>,
}

/// What [`Hub::connect`] hands to the connection loop.
#[derive(Debug)]
pub struct Registered {
    pub id: ConnectionId,
    /// Messages for this connection; yields `None` once the hub dropped it as too slow.
    pub outbox: mpsc::Receiver<ServerMessage>,
    /// Resolves when the connection's session ended (sign-out, revocation, password reset).
    pub session_ended: oneshot::Receiver<()>,
}

#[derive(Debug, Default)]
struct State {
    next_id: u64,
    connections: HashMap<ConnectionId, Connection>,
}

/// Result of [`Hub::subscribe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subscribe {
    Added,
    AlreadySubscribed,
    LimitReached,
    /// The connection is gone (dropped as too slow).
    Closed,
}

/// Shared handle; cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct Hub {
    state: Arc<RwLock<State>>,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a connection of `user` authenticated by `session`.
    pub fn connect(&self, user: UserId, session: SessionId) -> Registered {
        let (outbox, inbox) = mpsc::channel(OUTBOX_CAPACITY);
        let (ended, session_ended) = oneshot::channel();
        let mut state = self.write();
        state.next_id += 1;
        let id = ConnectionId(state.next_id);
        state.connections.insert(
            id,
            Connection {
                user,
                session,
                outbox,
                ended: Some(ended),
                channels: HashSet::new(),
            },
        );
        Registered {
            id,
            outbox: inbox,
            session_ended,
        }
    }

    /// Ends the connections of `user` authenticated by `session` (`None`: all of the user's
    /// sessions); returns how many. Their loops close them with
    /// [`crate::ws::CLOSE_SESSION_ENDED`].
    pub fn end_sessions(&self, user: UserId, session: Option<SessionId>) -> usize {
        let mut state = self.write();
        let ended: Vec<ConnectionId> = state
            .connections
            .iter()
            .filter(|(_, c)| c.user == user && session.is_none_or(|s| s == c.session))
            .map(|(id, _)| *id)
            .collect();
        for id in &ended {
            if let Some(mut connection) = state.connections.remove(id)
                && let Some(signal) = connection.ended.take()
            {
                let _ = signal.send(());
            }
        }
        ended.len()
    }

    /// Forgets a connection and all its subscriptions.
    pub fn disconnect(&self, id: ConnectionId) {
        self.write().connections.remove(&id);
    }

    /// The user a connection belongs to.
    pub fn user_of(&self, id: ConnectionId) -> Option<UserId> {
        self.read().connections.get(&id).map(|c| c.user)
    }

    pub fn subscribe(&self, id: ConnectionId, channel: Channel) -> Subscribe {
        let mut state = self.write();
        let Some(connection) = state.connections.get_mut(&id) else {
            return Subscribe::Closed;
        };
        if connection.channels.contains(&channel) {
            Subscribe::AlreadySubscribed
        } else if connection.channels.len() >= MAX_SUBSCRIPTIONS {
            Subscribe::LimitReached
        } else {
            connection.channels.insert(channel);
            Subscribe::Added
        }
    }

    pub fn unsubscribe(&self, id: ConnectionId, channel: &Channel) {
        if let Some(connection) = self.write().connections.get_mut(&id) {
            connection.channels.remove(channel);
        }
    }

    /// Sends `event` to every connection subscribed to `channel`; returns how many received
    /// it. A connection whose outbox is full is dropped (its client reconnects and refetches,
    /// ARCHITECTURE §10.2), so one slow client cannot hold up the others.
    pub fn publish(&self, channel: &Channel, event: &Event) -> usize {
        let mut delivered = 0;
        let mut slow = Vec::new();
        {
            let state = self.read();
            for (id, connection) in &state.connections {
                if !connection.channels.contains(channel) {
                    continue;
                }
                let message = ServerMessage::Event {
                    channel: channel.to_string(),
                    event: event.clone(),
                };
                match connection.outbox.try_send(message) {
                    Ok(()) => delivered += 1,
                    Err(mpsc::error::TrySendError::Full(_)) => slow.push(*id),
                    Err(mpsc::error::TrySendError::Closed(_)) => {}
                }
            }
        }
        if !slow.is_empty() {
            let mut state = self.write();
            for id in slow {
                state.connections.remove(&id);
                tracing::warn!(connection = id.0, "realtime connection dropped: too slow");
            }
        }
        delivered
    }

    /// Open connections on this instance.
    pub fn connection_count(&self) -> usize {
        self.read().connections.len()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, State> {
        self.state.write().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Event {
        Event {
            name: "item.changed".into(),
            data: serde_json::json!({"id": "x"}),
        }
    }

    #[test]
    fn publishes_only_to_subscribers_of_the_channel() {
        let hub = Hub::new();
        let (alice, bob) = (UserId::new(), UserId::new());
        let Registered {
            id: a,
            outbox: mut a_rx,
            ..
        } = hub.connect(alice, SessionId::new());
        let Registered {
            outbox: mut b_rx, ..
        } = hub.connect(bob, SessionId::new());
        assert_eq!(hub.subscribe(a, Channel::User(alice)), Subscribe::Added);
        assert_eq!(
            hub.subscribe(a, Channel::User(alice)),
            Subscribe::AlreadySubscribed
        );

        assert_eq!(hub.publish(&Channel::User(alice), &event()), 1);
        assert_eq!(hub.publish(&Channel::User(bob), &event()), 0);
        assert!(matches!(a_rx.try_recv(), Ok(ServerMessage::Event { .. })));
        assert!(a_rx.try_recv().is_err());
        assert!(b_rx.try_recv().is_err());

        hub.unsubscribe(a, &Channel::User(alice));
        assert_eq!(hub.publish(&Channel::User(alice), &event()), 0);
    }

    #[test]
    fn disconnect_forgets_subscriptions() {
        let hub = Hub::new();
        let user = UserId::new();
        let id = hub.connect(user, SessionId::new()).id;
        assert_eq!(hub.user_of(id), Some(user));
        hub.subscribe(id, Channel::User(user));
        hub.disconnect(id);
        assert_eq!(hub.connection_count(), 0);
        assert_eq!(hub.publish(&Channel::User(user), &event()), 0);
        assert_eq!(hub.subscribe(id, Channel::User(user)), Subscribe::Closed);
    }

    #[test]
    fn subscriptions_are_limited() {
        let hub = Hub::new();
        let id = hub.connect(UserId::new(), SessionId::new()).id;
        for _ in 0..MAX_SUBSCRIPTIONS {
            assert_eq!(
                hub.subscribe(id, Channel::User(UserId::new())),
                Subscribe::Added
            );
        }
        assert_eq!(
            hub.subscribe(id, Channel::User(UserId::new())),
            Subscribe::LimitReached
        );
    }

    #[test]
    fn a_slow_connection_is_dropped_without_blocking_others() {
        let hub = Hub::new();
        let user = UserId::new();
        let channel = Channel::User(user);
        let Registered {
            id: slow,
            outbox: mut slow_rx,
            ..
        } = hub.connect(user, SessionId::new());
        let Registered {
            id: fast,
            outbox: mut fast_rx,
            ..
        } = hub.connect(user, SessionId::new());
        hub.subscribe(slow, channel);
        hub.subscribe(fast, channel);
        for _ in 0..OUTBOX_CAPACITY {
            hub.publish(&channel, &event());
            assert!(fast_rx.try_recv().is_ok());
        }
        // The slow outbox is full now: the next publish drops it, the fast one still receives.
        assert_eq!(hub.publish(&channel, &event()), 1);
        assert_eq!(hub.connection_count(), 1);
        assert_eq!(hub.user_of(slow), None);
        let mut buffered = 0;
        while slow_rx.try_recv().is_ok() {
            buffered += 1;
        }
        assert_eq!(buffered, OUTBOX_CAPACITY);
        assert!(
            slow_rx.try_recv().is_err(),
            "closed after the buffer drains"
        );
    }

    #[test]
    fn ending_sessions_signals_and_removes_their_connections() {
        let hub = Hub::new();
        let (ada, bob) = (UserId::new(), UserId::new());
        let (phone, laptop) = (SessionId::new(), SessionId::new());
        let mut on_phone = hub.connect(ada, phone);
        let mut on_laptop = hub.connect(ada, laptop);
        let mut bobs = hub.connect(bob, SessionId::new());

        assert_eq!(hub.end_sessions(ada, Some(phone)), 1);
        assert!(on_phone.session_ended.try_recv().is_ok());
        assert!(on_laptop.session_ended.try_recv().is_err());
        assert_eq!(hub.user_of(on_phone.id), None);

        // A session of another user with the same id filter is not touched.
        assert_eq!(hub.end_sessions(bob, Some(laptop)), 0);
        assert_eq!(hub.end_sessions(ada, None), 1);
        assert!(on_laptop.session_ended.try_recv().is_ok());
        assert!(bobs.session_ended.try_recv().is_err());
        assert_eq!(hub.connection_count(), 1);
    }
}
