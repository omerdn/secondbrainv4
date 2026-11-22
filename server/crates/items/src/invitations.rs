//! Item invitations (PRD G5, V1, V5, T1.2.4). An invitation grants nothing until it is accepted;
//! declining changes nothing. Whether two people may share (V1: friends or teammates) is asked
//! through [`SharingPolicy`], because friendships arrive with E3.1. Until then the default
//! [`NoSharing`] refuses every invitation.

use std::future::Future;
use std::pin::Pin;

use access::{Perms, Reason, Relation, RelationTuple};
use chrono::{DateTime, Duration, Utc};
use kernel::{AppError, DomainEvent, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use ts_rs::TS;
use uuid::Uuid;

use crate::activity::{self, ActivityKind, NewActivity};
use crate::service::{ItemsService, LIST_LIMIT};
use crate::touch::{self, TouchAction};

/// Most invitations one person can send per [`INVITE_WINDOW`] (ARCHITECTURE §15). Answered ones
/// count too, so declining does not give the sender another try.
pub const INVITE_LIMIT: i64 = 30;
/// The sliding window of [`INVITE_LIMIT`].
pub const INVITE_WINDOW: Duration = Duration::hours(1);

/// Seconds until the oldest invitation in the window leaves it; at least 1.
fn retry_after_secs(oldest: DateTime<Utc>, now: DateTime<Utc>) -> u64 {
    u64::try_from((oldest + INVITE_WINDOW - now).num_seconds())
        .unwrap_or(0)
        .max(1)
}

/// Result of a [`SharingPolicy`] check.
pub type SharingFuture<'a> = Pin<Box<dyn Future<Output = Result<bool, AppError>> + Send + 'a>>;

/// The V1 precondition: whether `from` may share an item with `to`.
pub trait SharingPolicy: Send + Sync {
    /// `true` if the two are friends or teammates.
    fn can_share(&self, from: Uuid, to: Uuid) -> SharingFuture<'_>;
}

/// Refuses everyone. The safe default until friendships (E3.1) and teams (F5) exist.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSharing;

impl SharingPolicy for NoSharing {
    fn can_share(&self, _from: Uuid, _to: Uuid) -> SharingFuture<'_> {
        Box::pin(async { Ok(false) })
    }
}

/// Published in the invitation's transaction; the notification center (E2.8) subscribes (G5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitationCreated {
    pub invitation_id: Uuid,
    pub item_id: Uuid,
    pub from_user: Uuid,
    pub to_user: Uuid,
}

impl DomainEvent for InvitationCreated {
    const EVENT_TYPE: &'static str = "invitation.created";
}

/// The membership an invitation offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum InvitationRole {
    Assignee,
    Watcher,
    /// Notebook member who edits its pages.
    Editor,
    /// Notebook member who views its pages.
    Viewer,
}

impl InvitationRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Assignee => "assignee",
            Self::Watcher => "watcher",
            Self::Editor => "editor",
            Self::Viewer => "viewer",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "assignee" => Some(Self::Assignee),
            "watcher" => Some(Self::Watcher),
            "editor" => Some(Self::Editor),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }

    const fn relation(self) -> Relation {
        match self {
            Self::Assignee => Relation::Assignee,
            Self::Watcher => Relation::Watcher,
            Self::Editor => Relation::Editor,
            Self::Viewer => Relation::Viewer,
        }
    }
}

/// Where an invitation stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
    /// No longer valid when answered: the inviter lost the right to share, V1 stopped holding,
    /// or the item went to the trash.
    Expired,
}

impl InvitationStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "accepted" => Some(Self::Accepted),
            "declined" => Some(Self::Declined),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

/// An invitation as the API returns it. The title is what the invitee decides on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Invitation {
    pub id: Uuid,
    pub item_id: Uuid,
    pub item_kind: String,
    pub item_title: String,
    pub from_user: Uuid,
    pub to_user: Uuid,
    pub role: InvitationRole,
    pub status: InvitationStatus,
    pub created_at: DateTime<Utc>,
    pub responded_at: Option<DateTime<Utc>>,
}

/// `POST /items/{id}/invitations` body.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct CreateInvitationRequest {
    pub to_user: Uuid,
    pub role: InvitationRole,
}

/// Whether an item of `kind` can have a member with `role`. Notebooks have editors and viewers.
/// Pages have no members of their own: their viewers come from the notebook and links (V2).
#[must_use]
pub fn role_allowed(kind: &str, role: InvitationRole) -> bool {
    match kind {
        "page" => false,
        "notebook" => matches!(role, InvitationRole::Editor | InvitationRole::Viewer),
        _ => matches!(role, InvitationRole::Assignee | InvitationRole::Watcher),
    }
}

const SELECT: &str = "SELECT inv.id, inv.object_id, i.kind AS item_kind, i.title AS item_title,
                             inv.from_user, inv.to_user, inv.role, inv.status, inv.created_at,
                             inv.responded_at
                      FROM invitations inv JOIN items i ON i.id = inv.object_id
                      WHERE inv.kind = 'item_member'";

#[derive(FromRow)]
struct InvitationRow {
    id: Uuid,
    object_id: Uuid,
    item_kind: String,
    item_title: String,
    from_user: Uuid,
    to_user: Uuid,
    role: String,
    status: String,
    created_at: DateTime<Utc>,
    responded_at: Option<DateTime<Utc>>,
}

impl TryFrom<InvitationRow> for Invitation {
    type Error = AppError;

    fn try_from(row: InvitationRow) -> Result<Self, AppError> {
        // Both are excluded by the table's CHECK constraints.
        let role = InvitationRole::parse(&row.role).ok_or_else(|| corrupt("role"))?;
        let status = InvitationStatus::parse(&row.status).ok_or_else(|| corrupt("status"))?;
        Ok(Self {
            id: row.id,
            item_id: row.object_id,
            item_kind: row.item_kind,
            item_title: row.item_title,
            from_user: row.from_user,
            to_user: row.to_user,
            role,
            status,
            created_at: row.created_at,
            responded_at: row.responded_at,
        })
    }
}

fn corrupt(column: &str) -> AppError {
    internal(sqlx::Error::Decode(
        format!("unknown invitation {column}").into(),
    ))
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn invalid(field: &str, message: &str) -> AppError {
    let mut errors = ValidationErrors::new();
    errors.push(FieldError::new(field, FieldErrorCode::Invalid, message));
    match errors.into_result() {
        Err(err) => err,
        Ok(()) => AppError::NotFound,
    }
}

impl ItemsService {
    /// Invites `new.to_user` to an item. Needs SHARE (V5) and the V1 precondition. Creates no
    /// access; publishes [`InvitationCreated`] in the same transaction (G5).
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an item the actor cannot see, [`AppError::Forbidden`] without
    /// SHARE or when V1 does not hold, [`AppError::Validation`] for a role the kind does not
    /// have or an invitation to oneself, [`AppError::Conflict`] for an item in the trash, an
    /// existing member or an open invitation, or [`AppError::Internal`].
    pub async fn invite(
        &self,
        actor: Uuid,
        item_id: Uuid,
        new: CreateInvitationRequest,
    ) -> Result<Invitation, AppError> {
        self.require(actor, item_id, Perms::SHARE).await?;
        let item = self.load(item_id).await?.ok_or(AppError::NotFound)?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        if new.to_user == actor {
            return Err(invalid("to_user", "you cannot invite yourself"));
        }
        if !role_allowed(&item.kind, new.role) {
            return Err(invalid("role", "this item kind has no such role"));
        }
        // Before anything about the person is read: a stranger gets the same answer always.
        if !self.sharing.can_share(actor, new.to_user).await? {
            return Err(AppError::Forbidden);
        }
        if self.is_member(new.to_user, item_id).await? {
            return Err(AppError::Conflict("already a member of this item".into()));
        }
        let now = self.clock.now();
        let id = kernel::Id::<()>::new_at(now).as_uuid();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        // Serializes one sender's invitations, so parallel requests cannot pass the limit.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 0))")
            .bind(actor)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        let (recent, oldest): (i64, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT count(*), min(created_at) FROM invitations
             WHERE from_user = $1 AND created_at > $2",
        )
        .bind(actor)
        .bind(now - INVITE_WINDOW)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        if recent >= INVITE_LIMIT {
            return Err(AppError::RateLimited {
                retry_after_secs: oldest.map(|oldest| retry_after_secs(oldest, now)),
            });
        }
        sqlx::query(
            "INSERT INTO invitations (id, kind, object_id, from_user, to_user, role, created_at)
             VALUES ($1, 'item_member', $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(item_id)
        .bind(actor)
        .bind(new.to_user)
        .bind(new.role.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            if err
                .as_database_error()
                .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
            {
                AppError::Conflict("an invitation is already pending".into())
            } else {
                internal(err)
            }
        })?;
        db::outbox::publish(
            &mut tx,
            &InvitationCreated {
                invitation_id: id,
                item_id,
                from_user: actor,
                to_user: new.to_user,
            },
        )
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.load_invitation(id).await
    }

    /// `actor`'s pending invitations, newest first. Items in the trash are left out.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn pending_invitations(&self, actor: Uuid) -> Result<Vec<Invitation>, AppError> {
        let rows = sqlx::query_as::<_, InvitationRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT} AND inv.to_user = $1 AND inv.status = 'pending' AND i.deleted_at IS NULL
             ORDER BY inv.created_at DESC, inv.id DESC LIMIT $2"
        )))
        .bind(actor)
        .bind(LIST_LIMIT)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        rows.into_iter().map(Invitation::try_from).collect()
    }

    /// Accepts an invitation: the membership is added and access recomputed in one transaction
    /// (G5, V8). The invitation is re-validated first, because time has passed: if the inviter
    /// can no longer share, V1 no longer holds or the item is in the trash, it expires instead.
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not `actor`'s, [`AppError::Conflict`] if it was already
    /// answered or has just expired, or [`AppError::Internal`].
    pub async fn accept_invitation(&self, actor: Uuid, id: Uuid) -> Result<Invitation, AppError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row: Option<(Uuid, Uuid, String, String)> = sqlx::query_as(
            "SELECT object_id, from_user, role, status FROM invitations
             WHERE id = $1 AND to_user = $2 AND kind = 'item_member' FOR UPDATE",
        )
        .bind(id)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        let (item_id, from_user, role, status) = row.ok_or(AppError::NotFound)?;
        if status != "pending" {
            return Err(AppError::Conflict(
                "this invitation was already answered".into(),
            ));
        }
        let role = InvitationRole::parse(&role).ok_or_else(|| corrupt("role"))?;
        let in_trash = self
            .load(item_id)
            .await?
            .is_none_or(|item| item.deleted_at.is_some());
        let inviter_can_share = self
            .access
            .permissions()
            .check(&mut *tx, from_user, item_id, Perms::SHARE)
            .await
            .map_err(internal)?;
        let valid =
            !in_trash && inviter_can_share && self.sharing.can_share(from_user, actor).await?;
        set_status(&mut tx, id, if valid { "accepted" } else { "expired" }, now).await?;
        if valid {
            self.access
                .grant(&mut tx, RelationTuple::new(item_id, role.relation(), actor))
                .await
                .map_err(internal)?;
            let event = NewActivity {
                member: Some((actor, role)),
                ..NewActivity::new(Some(from_user), item_id, ActivityKind::for_role(role))
            };
            activity::record(&mut tx, now, event)
                .await
                .map_err(internal)?;
            touch::apply(&mut tx, item_id, TouchAction::MemberJoined, now)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        if !valid {
            return Err(AppError::Conflict(
                "this invitation is no longer valid".into(),
            ));
        }
        self.load_invitation(id).await
    }

    /// Declines an invitation; nothing else changes (G5).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not `actor`'s, [`AppError::Conflict`] if it was already
    /// answered, or [`AppError::Internal`].
    pub async fn decline_invitation(&self, actor: Uuid, id: Uuid) -> Result<Invitation, AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM invitations
             WHERE id = $1 AND to_user = $2 AND kind = 'item_member' FOR UPDATE",
        )
        .bind(id)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        if status.ok_or(AppError::NotFound)? != "pending" {
            return Err(AppError::Conflict(
                "this invitation was already answered".into(),
            ));
        }
        set_status(&mut tx, id, "declined", self.clock.now()).await?;
        tx.commit().await.map_err(internal)?;
        self.load_invitation(id).await
    }

    /// A direct role on the item; access inherited through a link or notebook does not count.
    async fn is_member(&self, user: Uuid, item_id: Uuid) -> Result<bool, AppError> {
        let explanation = self
            .access
            .permissions()
            .explain(&self.pool, user, item_id)
            .await
            .map_err(internal)?;
        Ok(explanation.is_some_and(|e| {
            e.reasons.iter().any(|reason| {
                matches!(
                    reason,
                    Reason::Owner
                        | Reason::Assignee
                        | Reason::Watcher
                        | Reason::Editor
                        | Reason::Viewer
                )
            })
        }))
    }

    async fn load_invitation(&self, id: Uuid) -> Result<Invitation, AppError> {
        sqlx::query_as::<_, InvitationRow>(sqlx::AssertSqlSafe(format!("{SELECT} AND inv.id = $1")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?
            .try_into()
    }
}

async fn set_status(
    tx: &mut sqlx::PgConnection,
    id: Uuid,
    status: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE invitations SET status = $2, responded_at = $3 WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(now)
        .execute(tx)
        .await
        .map_err(internal)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use InvitationRole::{Assignee, Editor, Viewer, Watcher};

    #[test]
    fn roles_by_kind() {
        for kind in ["task", "routine", "goal", "tracker", "module:budget"] {
            assert!(role_allowed(kind, Assignee), "{kind}");
            assert!(role_allowed(kind, Watcher), "{kind}");
            assert!(!role_allowed(kind, Editor), "{kind}");
            assert!(!role_allowed(kind, Viewer), "{kind}");
        }
        assert!(role_allowed("notebook", Editor));
        assert!(role_allowed("notebook", Viewer));
        assert!(!role_allowed("notebook", Assignee));
        for role in [Assignee, Watcher, Editor, Viewer] {
            assert!(!role_allowed("page", role), "pages have no direct members");
        }
    }

    #[test]
    fn retry_after_counts_down_to_at_least_one() {
        let oldest = DateTime::<Utc>::UNIX_EPOCH;
        assert_eq!(retry_after_secs(oldest, oldest), 3600);
        assert_eq!(retry_after_secs(oldest, oldest + Duration::minutes(59)), 60);
        assert_eq!(retry_after_secs(oldest, oldest + INVITE_WINDOW), 1);
        assert_eq!(retry_after_secs(oldest, oldest + Duration::hours(2)), 1);
    }

    #[test]
    fn role_round_trip() {
        for role in [Assignee, Watcher, Editor, Viewer] {
            assert_eq!(InvitationRole::parse(role.as_str()), Some(role));
        }
        assert_eq!(InvitationRole::parse("owner"), None);
    }
}
