//! `Friendships`: requests, friendships and blocks (PRD AK1, AK2, V1, T3.1.1).

use std::collections::HashMap;

use identity::IdentityService;
use items::{SharingFuture, SharingPolicy};
use kernel::{AppError, FieldError, FieldErrorCode, SharedClock, ValidationErrors};
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{
    BlockedUser, DECLINE_COOLDOWN, Friend, FriendRequestSent, IncomingRequest, Nudged,
    OutgoingRequest, REQUESTS_PER_DAY, normalize_email, nudge_day, pair,
};
use crate::repo;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn invalid(field: &str, message: &str) -> AppError {
    ValidationErrors::from(FieldError::new(field, FieldErrorCode::Invalid, message)).into()
}

/// What the sender hears for a duplicate or a request too soon after a decline; the same words,
/// so a decline is not told apart (AK2).
const ALREADY_ASKED: &str = "a request to this address was sent recently";

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct Friendships {
    pool: PgPool,
    clock: SharedClock,
    identity: IdentityService,
}

impl Friendships {
    #[must_use]
    pub const fn new(pool: PgPool, clock: SharedClock, identity: IdentityService) -> Self {
        Self {
            pool,
            clock,
            identity,
        }
    }

    async fn names(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, String>, AppError> {
        self.identity.display_names(ids).await
    }

    /// Sends a friend request to `email` (AK1). The answer is the same whether the address has
    /// an account, and whether its owner blocked the sender: those requests never arrive (AK2).
    ///
    /// # Errors
    /// [`AppError::Validation`] for a bad address, one's own, or someone the caller blocked;
    /// [`AppError::Conflict`] for a friend or an address asked recently; [`AppError::RateLimited`]
    /// past [`REQUESTS_PER_DAY`]; [`AppError::Internal`].
    pub async fn send(&self, actor: Uuid, email: &str) -> Result<OutgoingRequest, AppError> {
        let email = normalize_email(email)?;
        let now = self.clock.now();
        let to = self.identity.user_id_by_email(&email).await?;
        if to == Some(actor) {
            return Err(invalid("email", "that is your own address"));
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let sent = repo::sent_since(&mut *tx, actor, now - chrono::Duration::days(1))
            .await
            .map_err(internal)?;
        if sent >= REQUESTS_PER_DAY {
            return Err(AppError::RateLimited {
                retry_after_secs: None,
            });
        }
        if let Some(to) = to {
            if repo::are_friends(&mut *tx, pair(actor, to))
                .await
                .map_err(internal)?
            {
                return Err(AppError::Conflict("you are friends already".into()));
            }
            if repo::has_blocked(&mut *tx, actor, to)
                .await
                .map_err(internal)?
            {
                return Err(invalid("email", "unblock this person first"));
            }
        }
        if repo::recently_asked(&mut *tx, actor, &email, now - DECLINE_COOLDOWN)
            .await
            .map_err(internal)?
        {
            return Err(AppError::Conflict(ALREADY_ASKED.into()));
        }
        let silenced = match to {
            Some(to) => repo::has_blocked(&mut *tx, to, actor)
                .await
                .map_err(internal)?,
            None => false,
        };
        let id = Uuid::now_v7();
        if !repo::insert_request(&mut *tx, id, actor, &email, to, silenced, now)
            .await
            .map_err(internal)?
        {
            return Err(AppError::Conflict(ALREADY_ASKED.into()));
        }
        if let (Some(to_user), false) = (to, silenced) {
            db::outbox::publish(
                &mut tx,
                &FriendRequestSent {
                    request_id: id,
                    from_user: actor,
                    to_user,
                },
            )
            .await
            .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(OutgoingRequest {
            id,
            to_email: email,
            created_at: now,
        })
    }

    /// The caller's open requests, newest first.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn outgoing(&self, actor: Uuid) -> Result<Vec<OutgoingRequest>, AppError> {
        Ok(repo::outgoing(&self.pool, actor)
            .await
            .map_err(internal)?
            .into_iter()
            .map(|(id, to_email, created_at)| OutgoingRequest {
                id,
                to_email,
                created_at,
            })
            .collect())
    }

    /// Requests that reached the caller, newest first.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn incoming(&self, actor: Uuid) -> Result<Vec<IncomingRequest>, AppError> {
        let rows = repo::incoming(&self.pool, actor).await.map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|(_, from, _)| *from).collect();
        let names = self.names(&ids).await?;
        Ok(rows
            .into_iter()
            .map(|(id, from_user, created_at)| IncomingRequest {
                id,
                from_user,
                from_name: names.get(&from_user).cloned().unwrap_or_default(),
                created_at,
            })
            .collect())
    }

    /// Accepts a request that reached the caller: they are friends (AK1).
    ///
    /// # Errors
    /// [`AppError::NotFound`] for anything but a pending request to the caller, or
    /// [`AppError::Internal`].
    pub async fn accept(&self, actor: Uuid, id: Uuid) -> Result<Friend, AppError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let from = repo::lock_incoming(&mut tx, actor, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        if repo::blocked_between(&mut *tx, actor, from)
            .await
            .map_err(internal)?
        {
            return Err(AppError::NotFound);
        }
        repo::respond(&mut *tx, id, "accepted", now)
            .await
            .map_err(internal)?;
        repo::insert_friendship(&mut *tx, pair(actor, from), now)
            .await
            .map_err(internal)?;
        // Their own open request the other way is answered too.
        repo::cancel_between(&mut *tx, actor, from, now)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        let name = self.names(&[from]).await?.remove(&from).unwrap_or_default();
        Ok(Friend {
            user_id: from,
            display_name: name,
            since: now,
        })
    }

    /// Declines a request that reached the caller; the sender waits [`DECLINE_COOLDOWN`] to ask
    /// again and is not told (AK2).
    ///
    /// # Errors
    /// [`AppError::NotFound`] for anything but a pending request to the caller, or
    /// [`AppError::Internal`].
    pub async fn decline(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        repo::lock_incoming(&mut tx, actor, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        repo::respond(&mut *tx, id, "declined", self.clock.now())
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// Takes back one of the caller's open requests.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for anything but the caller's open request, or
    /// [`AppError::Internal`].
    pub async fn cancel(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        if repo::cancel_own(&self.pool, actor, id, self.clock.now())
            .await
            .map_err(internal)?
        {
            Ok(())
        } else {
            Err(AppError::NotFound)
        }
    }

    /// The caller's friends, newest first.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn friends(&self, actor: Uuid) -> Result<Vec<Friend>, AppError> {
        let rows = repo::friends(&self.pool, actor).await.map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
        let names = self.names(&ids).await?;
        Ok(rows
            .into_iter()
            .map(|(user_id, since)| Friend {
                user_id,
                display_name: names.get(&user_id).cloned().unwrap_or_default(),
                since,
            })
            .collect())
    }

    /// Ends a friendship (AK1). Items shared before stay shared (V8).
    ///
    /// # Errors
    /// [`AppError::NotFound`] when they are not friends, or [`AppError::Internal`].
    pub async fn unfriend(&self, actor: Uuid, other: Uuid) -> Result<(), AppError> {
        if repo::delete_friendship(&self.pool, pair(actor, other))
            .await
            .map_err(internal)?
        {
            Ok(())
        } else {
            Err(AppError::NotFound)
        }
    }

    /// Blocks someone (AK1, AK2): the friendship and open requests between them end, and they
    /// can no longer ask or share. Blocking an unknown id changes nothing and says nothing.
    ///
    /// # Errors
    /// [`AppError::Validation`] for oneself, or [`AppError::Internal`].
    pub async fn block(&self, actor: Uuid, other: Uuid) -> Result<(), AppError> {
        if actor == other {
            return Err(invalid("user_id", "you cannot block yourself"));
        }
        if self.names(&[other]).await?.is_empty() {
            return Ok(());
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        repo::insert_block(&mut *tx, actor, other, now)
            .await
            .map_err(internal)?;
        repo::delete_friendship(&mut *tx, pair(actor, other))
            .await
            .map_err(internal)?;
        repo::cancel_between(&mut *tx, actor, other, now)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// Lifts a block.
    ///
    /// # Errors
    /// [`AppError::NotFound`] when the caller did not block them, or [`AppError::Internal`].
    pub async fn unblock(&self, actor: Uuid, other: Uuid) -> Result<(), AppError> {
        if repo::delete_block(&self.pool, actor, other)
            .await
            .map_err(internal)?
        {
            Ok(())
        } else {
            Err(AppError::NotFound)
        }
    }

    /// The people the caller blocked, newest first.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn blocked(&self, actor: Uuid) -> Result<Vec<BlockedUser>, AppError> {
        let rows = repo::blocks(&self.pool, actor).await.map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
        let names = self.names(&ids).await?;
        Ok(rows
            .into_iter()
            .map(|(user_id, since)| BlockedUser {
                user_id,
                display_name: names.get(&user_id).cloned().unwrap_or_default(),
                since,
            })
            .collect())
    }

    /// Nudges a friend (AK3): at most once a day per person, the day being the sender's own.
    /// Someone who is not a friend, or blocked either way, cannot be nudged (AK2); the answer is
    /// the same in both cases, so a block does not show.
    ///
    /// # Errors
    /// [`AppError::Validation`] for oneself, [`AppError::Forbidden`] for someone who is not a
    /// friend, [`AppError::RateLimited`] for a second nudge the same day, or
    /// [`AppError::Internal`].
    pub async fn nudge(&self, actor: Uuid, other: Uuid) -> Result<(), AppError> {
        if actor == other {
            return Err(invalid("user_id", "you cannot nudge yourself"));
        }
        if !self.may_share(actor, other).await? {
            return Err(AppError::Forbidden);
        }
        let timezone = self
            .identity
            .current_user(identity::UserId::from_uuid(actor))
            .await?
            .timezone
            .parse()
            .unwrap_or(chrono_tz::UTC);
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        if !repo::insert_nudge(&mut *tx, (actor, other), nudge_day(now, timezone), now)
            .await
            .map_err(internal)?
        {
            return Err(AppError::RateLimited {
                retry_after_secs: None,
            });
        }
        db::outbox::publish(
            &mut tx,
            &Nudged {
                from_user: actor,
                to_user: other,
            },
        )
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// V1 without teams: friends, and neither blocked the other.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn may_share(&self, from: Uuid, to: Uuid) -> Result<bool, AppError> {
        if from == to {
            return Ok(false);
        }
        let friends = repo::are_friends(&self.pool, pair(from, to))
            .await
            .map_err(internal)?;
        Ok(friends
            && !repo::blocked_between(&self.pool, from, to)
                .await
                .map_err(internal)?)
    }
}

/// The real V1 check for invitations (T1.2.4): friends who did not block each other.
impl SharingPolicy for Friendships {
    fn can_share(&self, from: Uuid, to: Uuid) -> SharingFuture<'_> {
        Box::pin(self.may_share(from, to))
    }
}
