//! Item channels and access loss (T1.1.6, PRD V10).
//!
//! Subscribing to `item:<id>` needs VIEW, checked through `access::PermissionService`. When a
//! recompute takes VIEW away, `access` publishes the `AccessRevoked` outbox event; the outbox
//! dispatcher delivers it on one instance, so the handler forwards it with the [`Notifier`]
//! (NOTIFY) and every instance's [`crate::Bridge`] ends the affected subscriptions.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use access::{AccessContext, AccessRevoked, PermissionService, Perms};
use db::PgPool;
use db::outbox::Dispatcher;
use identity::UserId;
use uuid::Uuid;

use crate::bridge::Notifier;

type CheckFuture = Pin<Box<dyn Future<Output = bool> + Send>>;
type CheckFn = dyn Fn(UserId, Uuid) -> CheckFuture + Send + Sync;

/// Decides whether a user may subscribe to an item's channel (VIEW). Cheap to clone.
#[derive(Clone)]
pub struct ItemViewCheck(Arc<CheckFn>);

impl ItemViewCheck {
    /// Checks through `permissions` on `pool`. A database error denies the subscription.
    pub fn new<C: AccessContext>(permissions: PermissionService<C>, pool: PgPool) -> Self {
        Self(Arc::new(move |user, item| {
            let (permissions, pool) = (permissions.clone(), pool.clone());
            Box::pin(async move {
                match permissions
                    .check(&pool, user.as_uuid(), item, Perms::VIEW)
                    .await
                {
                    Ok(allowed) => allowed,
                    Err(err) => {
                        tracing::warn!(error = %err, "realtime item access check failed");
                        false
                    }
                }
            })
        }))
    }

    /// Denies every item channel (tests and tools without the access engine).
    #[must_use]
    pub fn deny_all() -> Self {
        Self(Arc::new(|_, _| Box::pin(async { false })))
    }

    pub(crate) async fn can_view(&self, user: UserId, item: Uuid) -> bool {
        (self.0)(user, item).await
    }
}

impl std::fmt::Debug for ItemViewCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ItemViewCheck")
    }
}

/// Subscribes to `AccessRevoked` (call from `app::run`).
pub fn register(dispatcher: &mut Dispatcher, pool: PgPool, notifier: Notifier) {
    dispatcher.subscribe(
        "realtime.end-revoked-subscriptions",
        move |event: AccessRevoked, _| {
            let (pool, notifier) = (pool.clone(), notifier.clone());
            async move {
                let users: Vec<UserId> =
                    event.user_ids.into_iter().map(UserId::from_uuid).collect();
                notifier.revoke_access(&pool, event.item_id, &users).await?;
                Ok(())
            }
        },
    );
}
