//! An item's members (PRD N2, G5, V5, T3.2.1): who is related to it and as what, the invitations
//! still waiting, and removing a member. Invitations themselves are [`crate::invitations`].
//!
//! Anyone who can view the item sees its members; only those who may share it (the owner, V5)
//! see the pending invitations and remove others. A member can always leave; the owner cannot.

use access::{Perms, Relation, RelationTuple};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::service::ItemsService;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// What a member is to the item (N2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MemberRole {
    Owner,
    Assignee,
    Editor,
    Watcher,
    Viewer,
}

impl MemberRole {
    const fn of(relation: Relation) -> Option<Self> {
        match relation {
            Relation::Owner => Some(Self::Owner),
            Relation::Assignee => Some(Self::Assignee),
            Relation::Watcher => Some(Self::Watcher),
            Relation::Editor => Some(Self::Editor),
            Relation::Viewer => Some(Self::Viewer),
            _ => None,
        }
    }
}

/// One person and their roles on the item, strongest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ItemMember {
    pub user_id: Uuid,
    pub display_name: String,
    pub roles: Vec<MemberRole>,
}

/// An invitation not answered yet; only the owner sees these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PendingMember {
    pub invitation_id: Uuid,
    pub user_id: Uuid,
    pub display_name: String,
    pub role: crate::invitations::InvitationRole,
}

/// `GET /items/{id}/members`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ItemMembers {
    pub members: Vec<ItemMember>,
    pub pending: Vec<PendingMember>,
    /// Whether the caller may invite and remove others (V5).
    pub can_manage: bool,
}

impl ItemsService {
    /// Whether anyone besides the owner is a member of the item: a shared notebook shows an
    /// "ortak" badge (Y2, T3.4.1). Call it only for items the caller can view.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn has_members(&self, item: Uuid) -> Result<bool, AppError> {
        let tuples = self
            .access
            .item_members(&self.pool, item)
            .await
            .map_err(internal)?;
        Ok(tuples
            .iter()
            .any(|t| t.relation() != Relation::Owner && MemberRole::of(t.relation()).is_some()))
    }

    /// The item's owner (a person) and assignees, with no caller: for background reads that act
    /// for the item's members (a shared routine's day). Never answer a request with this.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn owner_and_assignees(
        &self,
        item: Uuid,
    ) -> Result<(Option<Uuid>, Vec<Uuid>), AppError> {
        let tuples = self
            .access
            .item_members(&self.pool, item)
            .await
            .map_err(internal)?;
        let owner = tuples
            .iter()
            .find(|t| t.relation() == Relation::Owner)
            .map(RelationTuple::subject_id);
        let mut assignees: Vec<Uuid> = tuples
            .iter()
            .filter(|t| t.relation() == Relation::Assignee)
            .map(RelationTuple::subject_id)
            .collect();
        assignees.sort_unstable();
        assignees.dedup();
        Ok((owner, assignees))
    }

    /// The item's members, and for its owner the pending invitations. Names come from
    /// `names`, which is given every id at once.
    ///
    /// # Errors
    /// [`AppError::NotFound`] when the caller cannot view the item, or [`AppError::Internal`].
    pub async fn members<F, Fut>(
        &self,
        actor: Uuid,
        item: Uuid,
        names: F,
    ) -> Result<ItemMembers, AppError>
    where
        F: FnOnce(Vec<Uuid>) -> Fut,
        Fut:
            std::future::Future<Output = Result<std::collections::HashMap<Uuid, String>, AppError>>,
    {
        self.require(actor, item, Perms::VIEW).await?;
        let can_manage = self
            .access
            .permissions()
            .check(&self.pool, actor, item, Perms::SHARE)
            .await
            .map_err(internal)?;
        let tuples = self
            .access
            .item_members(&self.pool, item)
            .await
            .map_err(internal)?;
        let mut members: Vec<(Uuid, Vec<MemberRole>)> = Vec::new();
        for tuple in tuples {
            let Some(role) = MemberRole::of(tuple.relation()) else {
                continue;
            };
            match members
                .iter_mut()
                .find(|(user, _)| *user == tuple.subject_id())
            {
                Some((_, roles)) => roles.push(role),
                None => members.push((tuple.subject_id(), vec![role])),
            }
        }
        for (_, roles) in &mut members {
            roles.sort_unstable();
            roles.dedup();
        }
        members.sort_by(|a, b| a.1.first().cmp(&b.1.first()).then(a.0.cmp(&b.0)));
        let pending: Vec<(Uuid, Uuid, String)> = if can_manage {
            sqlx::query_as(
                "SELECT id, to_user, role FROM invitations
                 WHERE kind = 'item_member' AND object_id = $1 AND status = 'pending'
                 ORDER BY created_at, id",
            )
            .bind(item)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?
        } else {
            Vec::new()
        };
        let mut ids: Vec<Uuid> = members.iter().map(|(user, _)| *user).collect();
        ids.extend(pending.iter().map(|(_, user, _)| *user));
        let names = names(ids).await?;
        let name = |user: &Uuid| names.get(user).cloned().unwrap_or_default();
        Ok(ItemMembers {
            members: members
                .into_iter()
                .map(|(user_id, roles)| ItemMember {
                    display_name: name(&user_id),
                    user_id,
                    roles,
                })
                .collect(),
            pending: pending
                .into_iter()
                .filter_map(|(invitation_id, user_id, role)| {
                    crate::invitations::InvitationRole::parse(&role).map(|role| PendingMember {
                        invitation_id,
                        display_name: name(&user_id),
                        user_id,
                        role,
                    })
                })
                .collect(),
            can_manage,
        })
    }

    /// Removes `member` from the item: every role but ownership ends and access is recomputed
    /// (V8). The owner removes anyone (V5); a member removes only themselves (leaving).
    ///
    /// # Errors
    /// [`AppError::NotFound`] when the caller cannot view the item or `member` is not one,
    /// [`AppError::Forbidden`] for someone else's membership without the right to manage it,
    /// [`AppError::Validation`] for the owner, or [`AppError::Internal`].
    pub async fn remove_member(
        &self,
        actor: Uuid,
        item: Uuid,
        member: Uuid,
    ) -> Result<(), AppError> {
        self.require(actor, item, Perms::VIEW).await?;
        if actor != member {
            self.require(actor, item, Perms::SHARE).await?;
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let tuples = self
            .access
            .item_members(&mut *tx, item)
            .await
            .map_err(internal)?;
        let theirs: Vec<RelationTuple> = tuples
            .into_iter()
            .filter(|tuple| tuple.subject_id() == member)
            .collect();
        if theirs
            .iter()
            .any(|tuple| tuple.relation() == Relation::Owner)
        {
            return Err(ValidationErrors::from(FieldError::new(
                "user_id",
                FieldErrorCode::Invalid,
                "the owner stays on the item",
            ))
            .into());
        }
        if theirs.is_empty() {
            return Err(AppError::NotFound);
        }
        for tuple in theirs {
            self.access.revoke(&mut tx, tuple).await.map_err(internal)?;
        }
        tx.commit().await.map_err(internal)
    }
}
