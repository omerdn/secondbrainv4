//! Closing connections whose session ended (T0.5.6).
//!
//! `identity` announces revoked sessions with the `SessionsEnded` outbox event. The outbox
//! dispatcher delivers it on one instance only, so the handler forwards it through the
//! [`Notifier`] (NOTIFY) and every instance's [`crate::Bridge`] closes its own connections.
//! Expired sessions are caught by the per-heartbeat check in the connection loop.

use db::PgPool;
use db::outbox::Dispatcher;
use identity::SessionsEnded;

use crate::bridge::Notifier;

/// Subscribes to `SessionsEnded` (call from `app::run`).
pub fn register(dispatcher: &mut Dispatcher, pool: PgPool, notifier: Notifier) {
    dispatcher.subscribe(
        "realtime.close-ended-sessions",
        move |event: SessionsEnded, _| {
            let (pool, notifier) = (pool.clone(), notifier.clone());
            async move {
                notifier
                    .end_sessions(&pool, event.user_id, event.session_id)
                    .await?;
                Ok(())
            }
        },
    );
}
