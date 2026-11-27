//! `Notifications`: the in-app center and channel preferences (PRD BL1–BL3, T2.8.1).

use identity::UserId;
use kernel::{AppError, SharedClock};
use realtime::{Channel as RealtimeChannel, Event, Notifier};
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{
    Channel, NOTIFICATION_PAGE, Notification, NotificationKind, NotificationList,
    NotificationPayload, NotificationPreference,
};
use crate::push::{PushPublicKey, PushSubscriptionRequest, PushTarget, SendPush};
use crate::repo::{self, NotificationRow};

/// The realtime event that announces a new notification; it carries only its id.
pub const NEW_EVENT: &str = "notification.new";

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn to_notification(row: NotificationRow) -> Result<Notification, AppError> {
    Ok(Notification {
        id: row.id,
        payload: serde_json::from_value(row.payload).map_err(internal)?,
        read_at: row.read_at,
        created_at: row.created_at,
    })
}

/// Stored channel names, or the type's defaults when there is no row.
fn channels_of(kind: NotificationKind, stored: Option<Vec<String>>) -> (Vec<Channel>, bool) {
    match stored {
        Some(names) => {
            let mut channels: Vec<Channel> =
                names.iter().filter_map(|n| Channel::parse(n)).collect();
            channels.sort_unstable();
            channels.dedup();
            (channels, false)
        }
        None => (kind.default_channels(), true),
    }
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct Notifications {
    pool: PgPool,
    clock: SharedClock,
    notifier: Notifier,
    /// The VAPID public key when Web Push is configured (T4.2.4); push is queued only then.
    push_key: Option<String>,
}

impl Notifications {
    #[must_use]
    pub const fn new(pool: PgPool, clock: SharedClock, notifier: Notifier) -> Self {
        Self {
            pool,
            clock,
            notifier,
            push_key: None,
        }
    }

    /// Turns Web Push on with the server's VAPID public key (T4.2.4).
    #[must_use]
    pub fn with_push_key(mut self, public_key: impl Into<String>) -> Self {
        self.push_key = Some(public_key.into());
        self
    }

    /// The VAPID public key browsers subscribe with; `None` when Web Push is off.
    #[must_use]
    pub fn push_public_key(&self) -> PushPublicKey {
        PushPublicKey {
            public_key: self.push_key.clone(),
        }
    }

    /// Stores the caller's browser subscription (T4.2.4). The same browser signed in as
    /// someone else before now notifies the caller only.
    ///
    /// # Errors
    /// [`AppError::Validation`] for an endpoint that is not a browser push service or bad keys;
    /// [`AppError::Internal`] on database errors.
    pub async fn subscribe_push(
        &self,
        actor: Uuid,
        request: &PushSubscriptionRequest,
    ) -> Result<(), AppError> {
        request.validate()?;
        repo::upsert_push(
            &self.pool,
            (Uuid::now_v7(), actor),
            (
                &request.endpoint,
                request.keys.p256dh.trim_end_matches('='),
                request.keys.auth.trim_end_matches('='),
            ),
            self.clock.now(),
        )
        .await
        .map_err(internal)
    }

    /// Removes the caller's subscription with this endpoint; another user's is untouched and
    /// reported the same way (no leak of whose browser it is).
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn unsubscribe_push(&self, actor: Uuid, endpoint: &str) -> Result<(), AppError> {
        repo::delete_push(&self.pool, actor, endpoint)
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// A stored subscription, for the push job.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn push_target(&self, id: Uuid) -> Result<Option<PushTarget>, AppError> {
        Ok(repo::push_by_id(&self.pool, id)
            .await
            .map_err(internal)?
            .map(|row| PushTarget {
                endpoint: row.endpoint,
                p256dh: row.p256dh,
                auth: row.auth,
            }))
    }

    /// Deletes a subscription the push service reported gone.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn forget_push_subscription(&self, id: Uuid) -> Result<(), AppError> {
        repo::delete_push_by_id(&self.pool, id)
            .await
            .map_err(internal)
    }

    /// Queues an e-mail to `user` when their channels for `kind` include e-mail (BL3); once per
    /// source event, so a redelivered event sends nothing more. Whether one was queued.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn queue_email(
        &self,
        user: Uuid,
        kind: NotificationKind,
        template: crate::email::Template,
        source_event: Uuid,
    ) -> Result<bool, AppError> {
        let stored = repo::preference(&self.pool, user, kind.as_str())
            .await
            .map_err(internal)?;
        if !channels_of(kind, stored).0.contains(&Channel::Email) {
            return Ok(false);
        }
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let queued = jobs::enqueue(
            &mut conn,
            &crate::email::SendEmail {
                user_id: user,
                template,
            },
            jobs::EnqueueOptions {
                run_at: None,
                dedupe_key: Some(format!("email:{source_event}:{user}")),
            },
        )
        .await
        .map_err(internal)?;
        Ok(queued.is_some())
    }

    /// Makes `user`'s notification from the outbox event `source_event`, unless the user turned
    /// the in-app channel off for its type or the event already made one (a redelivery). The
    /// new notification is announced on the user's channel when the transaction commits. When
    /// the type's channels include push, one push job per browser subscription is queued in the
    /// same transaction, once per event (T4.2.4).
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn deliver(
        &self,
        user: Uuid,
        payload: &NotificationPayload,
        source_event: Uuid,
    ) -> Result<Option<Notification>, AppError> {
        let kind = payload.kind();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let stored = repo::preference(&mut *tx, user, kind.as_str())
            .await
            .map_err(internal)?;
        let channels = channels_of(kind, stored).0;
        if self.push_key.is_some() && channels.contains(&Channel::Push) {
            for subscription_id in repo::push_ids(&mut *tx, user).await.map_err(internal)? {
                jobs::enqueue(
                    &mut tx,
                    &SendPush {
                        subscription_id,
                        payload: payload.clone(),
                    },
                    jobs::EnqueueOptions {
                        run_at: None,
                        dedupe_key: Some(format!("push:{source_event}:{subscription_id}")),
                    },
                )
                .await
                .map_err(internal)?;
            }
        }
        if !channels.contains(&Channel::InApp) {
            tx.commit().await.map_err(internal)?;
            return Ok(None);
        }
        let json = serde_json::to_value(payload).map_err(internal)?;
        let Some(row) = repo::insert(
            &mut *tx,
            (Uuid::now_v7(), user, kind.as_str()),
            &json,
            source_event,
            self.clock.now(),
        )
        .await
        .map_err(internal)?
        else {
            tx.commit().await.map_err(internal)?;
            return Ok(None);
        };
        self.notifier
            .publish(
                &mut *tx,
                &RealtimeChannel::User(UserId::from_uuid(user)),
                &Event {
                    name: NEW_EVENT.into(),
                    data: serde_json::json!({ "id": row.id }),
                },
            )
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        to_notification(row).map(Some)
    }

    /// The caller's notifications, newest first, and how many are unread.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn list(
        &self,
        actor: Uuid,
        before: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<NotificationList, AppError> {
        let rows = repo::page(&self.pool, actor, before, NOTIFICATION_PAGE)
            .await
            .map_err(internal)?;
        let unread = repo::unread(&self.pool, actor).await.map_err(internal)?;
        Ok(NotificationList {
            notifications: rows
                .into_iter()
                .map(to_notification)
                .collect::<Result<_, _>>()?,
            unread,
        })
    }

    /// Marks one of the caller's notifications read; anybody else's is not found.
    ///
    /// # Errors
    /// [`AppError::NotFound`], or [`AppError::Internal`].
    pub async fn mark_read(&self, actor: Uuid, id: Uuid) -> Result<Notification, AppError> {
        repo::mark_read(&self.pool, actor, id, self.clock.now())
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)
            .and_then(to_notification)
    }

    /// Marks all the caller's notifications read; how many were unread.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn mark_all_read(&self, actor: Uuid) -> Result<u64, AppError> {
        repo::mark_all_read(&self.pool, actor, self.clock.now())
            .await
            .map_err(internal)
    }

    /// Every type's channels for the caller (BL3).
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn preferences(&self, actor: Uuid) -> Result<Vec<NotificationPreference>, AppError> {
        let stored = repo::preferences(&self.pool, actor)
            .await
            .map_err(internal)?;
        Ok(NotificationKind::ALL
            .into_iter()
            .map(|kind| {
                let row = stored
                    .iter()
                    .find(|(name, _)| name == kind.as_str())
                    .map(|(_, channels)| channels.clone());
                let (channels, default) = channels_of(kind, row);
                NotificationPreference {
                    kind,
                    channels,
                    default,
                }
            })
            .collect())
    }

    /// Sets a type's channels (BL3); none turns the type off.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn set_preference(
        &self,
        actor: Uuid,
        kind: NotificationKind,
        mut channels: Vec<Channel>,
    ) -> Result<NotificationPreference, AppError> {
        channels.sort_unstable();
        channels.dedup();
        let names: Vec<String> = channels.iter().map(|c| c.as_str().to_owned()).collect();
        repo::set_preference(&self.pool, actor, kind.as_str(), &names, self.clock.now())
            .await
            .map_err(internal)?;
        Ok(NotificationPreference {
            kind,
            channels,
            default: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_channels_win_over_defaults() {
        assert_eq!(
            channels_of(NotificationKind::Nudge, None),
            (vec![Channel::InApp, Channel::Push], true)
        );
        assert_eq!(
            channels_of(
                NotificationKind::Nudge,
                Some(vec![
                    "email".into(),
                    "in_app".into(),
                    "email".into(),
                    "fax".into()
                ])
            ),
            (vec![Channel::InApp, Channel::Email], false)
        );
        assert_eq!(
            channels_of(NotificationKind::Nudge, Some(Vec::new())),
            (Vec::new(), false)
        );
    }

    #[test]
    fn kinds_round_trip() {
        for kind in NotificationKind::ALL {
            assert_eq!(NotificationKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(NotificationKind::parse("spam"), None);
    }
}
