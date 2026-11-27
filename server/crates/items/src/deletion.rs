//! Handing over items when an account goes (PRD HS2, T4.3.1), and ownership transfer.
//!
//! - [`ItemsService::owned_shared`]: the shared items a person owns and who would get each
//!   (the guide of HS2.1).
//! - [`ItemsService::transfer_ownership`]: the owner hands an item to one of its members.
//! - The [`AccountDeletionHook`]: every item the person owns goes to its oldest member
//!   ([`heir`]); one nobody else is a member of is deleted for good. A notebook's pages follow the
//!   notebook (Y5). Then every relation of the person ends (access is recomputed, V8), and their
//!   tags, saved views, reactions and invitations are deleted.
//!
//! Team items are untouched: the team keeps them (TK7).

use access::{Relation, RelationTuple};
use chrono::{DateTime, Utc};
use identity::deletion::{AccountDeletionHook, HookFuture};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use ts_rs::TS;
use uuid::Uuid;

use crate::service::ItemsService;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

const MEMBER_ROLES: [Relation; 4] = [
    Relation::Assignee,
    Relation::Watcher,
    Relation::Editor,
    Relation::Viewer,
];

/// Who gets an item when `leaving` (its owner) goes: the person who has been a member longest,
/// by when their first membership was made, ties broken by id (HS2.2). `None` when nobody else
/// is a member.
#[must_use]
pub fn heir(members: &[(RelationTuple, DateTime<Utc>)], leaving: Uuid) -> Option<Uuid> {
    members
        .iter()
        .filter(|(tuple, _)| {
            tuple.subject_id() != leaving && MEMBER_ROLES.contains(&tuple.relation())
        })
        .min_by_key(|(tuple, at)| (*at, tuple.subject_id()))
        .map(|(tuple, _)| tuple.subject_id())
}

/// One shared item the caller owns, and who gets it if they delete their account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct OwnedSharedItem {
    pub item_id: Uuid,
    pub kind: String,
    pub title: String,
    pub heir_id: Uuid,
    pub heir_name: String,
}

/// `PUT /items/{id}/owner` body: the member who becomes the owner.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct TransferOwnershipRequest {
    pub user_id: Uuid,
}

impl ItemsService {
    /// Personal items `user` owns, notebooks first so their pages follow them.
    async fn owned_by(
        &self,
        conn: &mut PgConnection,
        user: Uuid,
    ) -> sqlx::Result<Vec<(Uuid, String)>> {
        sqlx::query_as(
            "SELECT id, kind FROM items WHERE owner_user_id = $1
             ORDER BY (kind = 'notebook') DESC, (kind = 'page'), id",
        )
        .bind(user)
        .fetch_all(conn)
        .await
    }

    /// Makes `to` the owner of `item` (and of its pages when it is a notebook) instead of
    /// `from`, recomputing access.
    async fn hand_over(
        &self,
        conn: &mut PgConnection,
        item: Uuid,
        (from, to): (Uuid, Uuid),
        now: DateTime<Utc>,
    ) -> sqlx::Result<()> {
        let mut ids = vec![item];
        ids.extend(self.access.pages_of(&mut *conn, item).await?);
        for id in ids {
            let moved = sqlx::query(
                "UPDATE items SET owner_user_id = $3, updated_at = $4
                 WHERE id = $1 AND owner_user_id = $2",
            )
            .bind(id)
            .bind(from)
            .bind(to)
            .bind(now)
            .execute(&mut *conn)
            .await?
            .rows_affected();
            if moved == 1 {
                // Grant first: the new owner never loses access in between.
                self.access
                    .grant(&mut *conn, RelationTuple::new(id, Relation::Owner, to))
                    .await?;
                self.access
                    .revoke(&mut *conn, RelationTuple::new(id, Relation::Owner, from))
                    .await?;
            }
        }
        Ok(())
    }

    /// Deletes `item` for good (and its pages when it is a notebook), as the trash purge does.
    async fn destroy(&self, conn: &mut PgConnection, item: Uuid) -> sqlx::Result<()> {
        let mut ids = self.access.pages_of(&mut *conn, item).await?;
        ids.push(item);
        for id in ids {
            let deleted = sqlx::query("DELETE FROM items WHERE id = $1")
                .bind(id)
                .execute(&mut *conn)
                .await?
                .rows_affected();
            if deleted == 1 {
                self.access.remove_item(&mut *conn, id).await?;
                sqlx::query(
                    "DELETE FROM invitations WHERE kind = 'item_member' AND object_id = $1",
                )
                .bind(id)
                .execute(&mut *conn)
                .await?;
            }
        }
        Ok(())
    }

    /// The shared items `actor` owns (not pages, which follow their notebook), and who gets
    /// each if they delete their account. Names come from `names`, given every id at once.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn owned_shared<F, Fut>(
        &self,
        actor: Uuid,
        names: F,
    ) -> Result<Vec<OwnedSharedItem>, AppError>
    where
        F: FnOnce(Vec<Uuid>) -> Fut,
        Fut:
            std::future::Future<Output = Result<std::collections::HashMap<Uuid, String>, AppError>>,
    {
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT id, kind, title FROM items
             WHERE owner_user_id = $1 AND kind <> 'page' AND deleted_at IS NULL
             ORDER BY created_at, id",
        )
        .bind(actor)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut shared = Vec::new();
        for (item_id, kind, title) in rows {
            let members = self
                .access
                .item_members_since(&self.pool, item_id)
                .await
                .map_err(internal)?;
            if let Some(heir_id) = heir(&members, actor) {
                shared.push((item_id, kind, title, heir_id));
            }
        }
        let names = names(shared.iter().map(|s| s.3).collect()).await?;
        Ok(shared
            .into_iter()
            .map(|(item_id, kind, title, heir_id)| OwnedSharedItem {
                heir_name: names.get(&heir_id).cloned().unwrap_or_default(),
                item_id,
                kind,
                title,
                heir_id,
            })
            .collect())
    }

    /// The owner of a personal item makes one of its members the owner (HS2.1). The former
    /// owner keeps no role. A notebook's pages go with it; a page itself is not transferred.
    ///
    /// # Errors
    /// [`AppError::NotFound`] when the caller does not own the item, [`AppError::Validation`]
    /// for a page or someone who is not a member, or [`AppError::Internal`].
    pub async fn transfer_ownership(
        &self,
        actor: Uuid,
        item: Uuid,
        to: Uuid,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let kind: Option<String> = sqlx::query_scalar(
            "SELECT kind FROM items WHERE id = $1 AND owner_user_id = $2 FOR UPDATE",
        )
        .bind(item)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        let Some(kind) = kind else {
            return Err(AppError::NotFound);
        };
        let invalid = |message: &str| -> AppError {
            ValidationErrors::from(FieldError::new("user_id", FieldErrorCode::Invalid, message))
                .into()
        };
        if kind == "page" {
            return Err(invalid("a page belongs to its notebook's owner"));
        }
        let members = self
            .access
            .item_members_since(&mut *tx, item)
            .await
            .map_err(internal)?;
        let is_member = members.iter().any(|(tuple, _)| {
            tuple.subject_id() == to && to != actor && MEMBER_ROLES.contains(&tuple.relation())
        });
        if !is_member {
            return Err(invalid("not a member of the item"));
        }
        self.hand_over(&mut tx, item, (actor, to), self.clock.now())
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// Everything `items` keeps about `user`, handed over or removed (HS2.2). Idempotent.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn prepare_account_deletion(&self, user: Uuid) -> Result<(), AppError> {
        let now = self.clock.now();
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let owned = self.owned_by(&mut conn, user).await.map_err(internal)?;
        drop(conn);
        for (item, _) in owned {
            // One transaction per item: a long account never holds one big lock.
            let mut tx = self.pool.begin().await.map_err(internal)?;
            // Pages handled with their notebook are gone or moved already.
            let still: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM items WHERE id = $1 AND owner_user_id = $2")
                    .bind(item)
                    .bind(user)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(internal)?;
            if still.is_some() {
                let members = self
                    .access
                    .item_members_since(&mut *tx, item)
                    .await
                    .map_err(internal)?;
                match heir(&members, user) {
                    Some(to) => self.hand_over(&mut tx, item, (user, to), now).await,
                    None => self.destroy(&mut tx, item).await,
                }
                .map_err(internal)?;
            }
            tx.commit().await.map_err(internal)?;
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        // Memberships of others' items (and any team role): access drops (V8).
        let relations = self
            .access
            .relations_of_user(&mut *tx, user)
            .await
            .map_err(internal)?;
        for tuple in relations {
            self.access.revoke(&mut tx, tuple).await.map_err(internal)?;
        }
        for sql in [
            "DELETE FROM invitations WHERE from_user = $1 OR to_user = $1",
            "DELETE FROM tags WHERE owner_user_id = $1",
            "DELETE FROM saved_views WHERE user_id = $1",
            "DELETE FROM activity_reactions WHERE user_id = $1",
        ] {
            sqlx::query(sql)
                .bind(user)
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)
    }
}

impl AccountDeletionHook for ItemsService {
    fn name(&self) -> &'static str {
        "items"
    }

    fn before_delete(&self, user: Uuid) -> HookFuture<'_> {
        Box::pin(self.prepare_account_deletion(user))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use chrono::TimeZone;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 27, 9, minute, 0).unwrap()
    }

    #[test]
    fn hs2_the_oldest_member_inherits() {
        let (item, ada, bob, can) = (
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
        );
        let members = vec![
            (RelationTuple::new(item, Relation::Owner, ada), at(0)),
            (RelationTuple::new(item, Relation::Watcher, can), at(5)),
            (RelationTuple::new(item, Relation::Assignee, bob), at(9)),
            (RelationTuple::new(item, Relation::Watcher, bob), at(2)),
        ];
        assert_eq!(heir(&members, ada), Some(bob), "Bob's first role is oldest");
        assert_eq!(heir(&members[..1], ada), None, "nobody else");
        // The leaving person is never their own heir, even through another role.
        let only_ada = vec![
            (RelationTuple::new(item, Relation::Owner, ada), at(0)),
            (RelationTuple::new(item, Relation::Assignee, ada), at(1)),
        ];
        assert_eq!(heir(&only_ada, ada), None);
    }

    #[test]
    fn hs2_a_tie_goes_to_the_smaller_id() {
        let item = Uuid::now_v7();
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let members = vec![
            (RelationTuple::new(item, Relation::Viewer, b), at(3)),
            (RelationTuple::new(item, Relation::Editor, a), at(3)),
        ];
        assert_eq!(heir(&members, Uuid::now_v7()), Some(a));
    }
}
