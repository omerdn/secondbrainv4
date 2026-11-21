//! Transactional outbox and domain event dispatcher (ARCHITECTURE §4.5, T0.2.5).
//!
//! # Publishing
//! Call [`publish`] with the **same transaction** that performs the domain change. Either both
//! are committed or neither is, so an event is never lost and never announces a change that was
//! rolled back.
//!
//! ```no_run
//! # use kernel::DomainEvent;
//! # #[derive(serde::Serialize, serde::Deserialize)] struct ItemCreated { id: String }
//! # impl DomainEvent for ItemCreated { const EVENT_TYPE: &'static str = "item.created"; }
//! # async fn example(pool: &db::PgPool) -> Result<(), db::DbError> {
//! let mut tx = pool.begin().await?;
//! // ... INSERT INTO items ... using &mut *tx
//! db::outbox::publish(&mut tx, &ItemCreated { id: "...".into() }).await?;
//! tx.commit().await?;
//! # Ok(()) }
//! ```
//!
//! # Delivery
//! The [`Dispatcher`] claims pending rows with `FOR UPDATE SKIP LOCKED` (several instances can
//! run side by side without delivering the same row concurrently), calls every subscriber of
//! the event type, and marks the row dispatched **only after all of them succeed**.
//!
//! Guarantees:
//! - **At least once.** If a subscriber fails, or the process dies mid-batch, the event is
//!   delivered again later, to all of its subscribers. **Subscribers must be idempotent.**
//! - Failed events are retried with exponential backoff ([`kernel::retry::backoff`]); the
//!   (redacted) error is kept in `last_error`.
//! - Events are processed in creation order, but a retried event may be delivered after newer
//!   ones. Subscribers must not rely on strict ordering.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use kernel::DomainEvent;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::DbError;

/// Error returned by a subscriber.
pub type HandlerError = Box<dyn std::error::Error + Send + Sync>;
/// Result of a subscriber.
pub type HandlerResult = Result<(), HandlerError>;

type BoxFuture = Pin<Box<dyn Future<Output = HandlerResult> + Send>>;
type Handler = Arc<dyn Fn(OutboxEvent) -> BoxFuture + Send + Sync>;

/// A stored event as seen by subscribers.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxEvent {
    /// UUIDv7 of the event (stable across redeliveries; use it for idempotency).
    pub id: Uuid,
    /// [`DomainEvent::EVENT_TYPE`].
    pub event_type: String,
    /// JSON payload.
    pub payload: serde_json::Value,
    /// When the event was published.
    pub created_at: DateTime<Utc>,
    /// Failed delivery attempts so far.
    pub attempts: i32,
}

impl OutboxEvent {
    /// Deserializes the payload.
    pub fn decode<E: DomainEvent>(&self) -> Result<E, serde_json::Error> {
        serde_json::from_value(self.payload.clone())
    }
}

/// Writes `event` to the outbox using the caller's connection or transaction.
pub async fn publish<E: DomainEvent>(conn: &mut PgConnection, event: &E) -> Result<Uuid, DbError> {
    let id = Uuid::now_v7();
    let payload = serde_json::to_value(event).map_err(|err| DbError::Encode(Box::new(err)))?;
    sqlx::query("INSERT INTO outbox (id, event_type, payload) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(E::EVENT_TYPE)
        .bind(payload)
        .execute(conn)
        .await?;
    Ok(id)
}

/// Dispatcher settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchConfig {
    /// Maximum events claimed per batch.
    pub batch_size: i64,
    /// Sleep between polls when the outbox is drained.
    pub poll_interval: Duration,
    /// Backoff after the first failure; doubles per attempt.
    pub retry_base: Duration,
    /// Upper bound for the backoff.
    pub retry_max: Duration,
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            poll_interval: Duration::from_millis(500),
            retry_base: Duration::from_secs(1),
            retry_max: Duration::from_secs(60 * 60),
        }
    }
}

/// Delivers outbox events to registered subscribers.
#[derive(Clone)]
pub struct Dispatcher {
    config: DispatchConfig,
    subscribers: HashMap<&'static str, Vec<(&'static str, Handler)>>,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let subscribers: HashMap<_, Vec<_>> = self
            .subscribers
            .iter()
            .map(|(event, handlers)| (*event, handlers.iter().map(|(name, _)| *name).collect()))
            .collect();
        f.debug_struct("Dispatcher")
            .field("config", &self.config)
            .field("subscribers", &subscribers)
            .finish()
    }
}

/// Counts from one [`Dispatcher::dispatch_batch`] call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchOutcome {
    /// Events delivered to all subscribers and marked dispatched.
    pub delivered: usize,
    /// Events that failed and were rescheduled.
    pub failed: usize,
}

impl BatchOutcome {
    /// Number of events claimed.
    pub fn claimed(&self) -> usize {
        self.delivered + self.failed
    }
}

impl Dispatcher {
    /// A dispatcher without subscribers.
    pub fn new(config: DispatchConfig) -> Self {
        Self {
            config,
            subscribers: HashMap::new(),
        }
    }

    /// Registers `handler` for events of type `E`. `name` identifies the subscriber in logs and
    /// in `last_error`.
    pub fn subscribe<E, F, Fut>(&mut self, name: &'static str, handler: F) -> &mut Self
    where
        E: DomainEvent,
        F: Fn(E, OutboxEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        let handler = Arc::new(handler);
        let erased: Handler = Arc::new(move |stored: OutboxEvent| {
            let handler = Arc::clone(&handler);
            Box::pin(async move {
                let event: E = stored.decode()?;
                handler(event, stored).await
            })
        });
        self.subscribers
            .entry(E::EVENT_TYPE)
            .or_default()
            .push((name, erased));
        self
    }

    /// Claims up to `batch_size` due events, delivers them and records the outcome, all in one
    /// transaction. Events without subscribers are marked dispatched.
    pub async fn dispatch_batch(&self, pool: &PgPool) -> Result<BatchOutcome, DbError> {
        let mut tx = pool.begin().await?;
        let events: Vec<OutboxEvent> = sqlx::query_as(
            "SELECT id, event_type, payload, created_at, attempts FROM outbox \
             WHERE dispatched_at IS NULL AND available_at <= now() \
             ORDER BY id LIMIT $1 FOR UPDATE SKIP LOCKED",
        )
        .bind(self.config.batch_size)
        .fetch_all(&mut *tx)
        .await?;

        let mut outcome = BatchOutcome::default();
        for event in events {
            match self.deliver(&event).await {
                Ok(()) => {
                    sqlx::query("UPDATE outbox SET dispatched_at = now() WHERE id = $1")
                        .bind(event.id)
                        .execute(&mut *tx)
                        .await?;
                    outcome.delivered += 1;
                }
                Err(error) => {
                    let attempt = u32::try_from(event.attempts.saturating_add(1)).unwrap_or(1);
                    let delay = kernel::retry::backoff(
                        attempt,
                        self.config.retry_base,
                        self.config.retry_max,
                    );
                    let delay_ms = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
                    tracing::warn!(
                        event_id = %event.id,
                        event_type = %event.event_type,
                        attempt,
                        retry_in_ms = delay_ms,
                        error = %error,
                        "outbox delivery failed"
                    );
                    sqlx::query(
                        "UPDATE outbox SET attempts = attempts + 1, last_error = $2, \
                         available_at = now() + ($3::bigint * interval '1 millisecond') \
                         WHERE id = $1",
                    )
                    .bind(event.id)
                    .bind(&error)
                    .bind(delay_ms)
                    .execute(&mut *tx)
                    .await?;
                    outcome.failed += 1;
                }
            }
        }
        tx.commit().await?;
        Ok(outcome)
    }

    /// Calls every subscriber; returns the (redacted) first error.
    async fn deliver(&self, event: &OutboxEvent) -> Result<(), String> {
        let Some(handlers) = self.subscribers.get(event.event_type.as_str()) else {
            return Ok(());
        };
        for (name, handler) in handlers {
            if let Err(err) = handler(event.clone()).await {
                return Err(kernel::redact::redact(&format!("{name}: {err}")));
            }
        }
        Ok(())
    }

    /// Dispatches continuously until `shutdown` resolves: full batches are followed immediately
    /// by the next one, otherwise the loop sleeps for `poll_interval`.
    pub async fn run<S>(&self, pool: &PgPool, shutdown: S)
    where
        S: Future<Output = ()>,
    {
        tokio::pin!(shutdown);
        let full_batch = usize::try_from(self.config.batch_size).unwrap_or(usize::MAX);
        loop {
            let claimed = match self.dispatch_batch(pool).await {
                Ok(outcome) => outcome.claimed(),
                Err(err) => {
                    tracing::error!(
                        error = %kernel::redact::redact(&err.to_string()),
                        "outbox dispatch failed"
                    );
                    0
                }
            };
            let pause = if claimed >= full_batch {
                Duration::ZERO
            } else {
                self.config.poll_interval
            };
            tokio::select! {
                () = &mut shutdown => break,
                () = tokio::time::sleep(pause) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_lists_subscribers_by_name() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Ping;
        impl DomainEvent for Ping {
            const EVENT_TYPE: &'static str = "test.ping";
        }

        let mut dispatcher = Dispatcher::new(DispatchConfig::default());
        dispatcher.subscribe("audit", |_: Ping, _| async { Ok(()) });
        let debug = format!("{dispatcher:?}");
        assert!(debug.contains("test.ping"));
        assert!(debug.contains("audit"));
    }
}
