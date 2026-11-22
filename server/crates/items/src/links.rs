//! Item links (PRD B1-B3, V3, V4, V8, V9, T1.2.3).
//!
//! Only a link from a page carries access: it is mirrored to `access` as `linked_to`, in the same
//! transaction, so viewers of the target can open the page and lose it when the link goes. Links
//! to goals from tasks, routines and trackers grant nothing. Text mentions (B3) are not links.

use access::{Perms, Relation, RelationTuple};
use chrono::{DateTime, Utc};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::Serialize;
use sqlx::FromRow;
use ts_rs::TS;
use uuid::Uuid;

use crate::model::Item;
use crate::service::{COLUMNS, ItemRow, ItemsService, LIST_LIMIT};

const PAGE: &str = "page";

/// Which end of the link the listed item is, seen from the item being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LinkDirection {
    /// The item being read links to `item`.
    Outgoing,
    /// `item` links to the item being read.
    Incoming,
}

/// A link as the API returns it. Links whose other end the caller cannot see are never
/// returned (V9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ItemLink {
    pub id: Uuid,
    pub direction: LinkDirection,
    /// The other end of the link.
    pub item: Item,
    pub created_at: DateTime<Utc>,
}

/// Whether B1 allows a link from `source` to `target`: pages to tasks, routines, goals and
/// trackers; tasks, routines and trackers to goals.
#[must_use]
pub fn link_allowed(source: &str, target: &str) -> bool {
    match source {
        PAGE => matches!(target, "task" | "routine" | "goal" | "tracker"),
        "task" | "routine" | "tracker" => target == "goal",
        _ => false,
    }
}

#[derive(FromRow)]
struct LinkRow {
    link_id: Uuid,
    link_created_at: DateTime<Utc>,
    outgoing: bool,
    #[sqlx(flatten)]
    item: ItemRow,
}

impl From<LinkRow> for ItemLink {
    fn from(row: LinkRow) -> Self {
        Self {
            id: row.link_id,
            direction: if row.outgoing {
                LinkDirection::Outgoing
            } else {
                LinkDirection::Incoming
            },
            item: row.item.into(),
            created_at: row.link_created_at,
        }
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl ItemsService {
    /// Links `source` to `target`. Needs LINK on the source and VIEW on the target (V4). Linking
    /// twice returns the existing link.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an item the actor cannot see (V9), [`AppError::Forbidden`]
    /// without LINK, [`AppError::Validation`] for a pair B1 does not allow,
    /// [`AppError::Conflict`] if either item is in the trash, or [`AppError::Internal`].
    pub async fn link(
        &self,
        actor: Uuid,
        source_id: Uuid,
        target_id: Uuid,
    ) -> Result<ItemLink, AppError> {
        self.require(actor, source_id, Perms::LINK).await?;
        self.require(actor, target_id, Perms::VIEW).await?;
        let source = self.load(source_id).await?.ok_or(AppError::NotFound)?;
        let target = self.load(target_id).await?.ok_or(AppError::NotFound)?;
        if !link_allowed(&source.kind, &target.kind) {
            let mut errors = ValidationErrors::new();
            errors.push(FieldError::new(
                "target_item_id",
                FieldErrorCode::Invalid,
                "these item kinds cannot be linked",
            ));
            errors.into_result()?;
        }
        if source.deleted_at.is_some() || target.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        let now = self.clock.now();
        let new_id = kernel::Id::<()>::new_at(now).as_uuid();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let (id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO item_links (id, source_item_id, target_item_id, created_by, created_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (source_item_id, target_item_id)
             DO UPDATE SET source_item_id = EXCLUDED.source_item_id
             RETURNING id, created_at",
        )
        .bind(new_id)
        .bind(source_id)
        .bind(target_id)
        .bind(actor)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        if source.kind == PAGE {
            // V8: recomputed in this transaction; a repeated grant is a no-op.
            self.access
                .grant(
                    &mut tx,
                    RelationTuple::new(source_id, Relation::LinkedTo, target_id),
                )
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(ItemLink {
            id,
            direction: LinkDirection::Outgoing,
            item: target,
            created_at,
        })
    }

    /// Removes the link from `source` to `target`; access that came through it drops at once
    /// (V8). Needs the same permissions as linking, so a hidden link stays hidden (V9). Removing
    /// an absent link is a no-op.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`] or [`AppError::Internal`].
    pub async fn unlink(
        &self,
        actor: Uuid,
        source_id: Uuid,
        target_id: Uuid,
    ) -> Result<(), AppError> {
        self.require(actor, source_id, Perms::LINK).await?;
        self.require(actor, target_id, Perms::VIEW).await?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let deleted =
            sqlx::query("DELETE FROM item_links WHERE source_item_id = $1 AND target_item_id = $2")
                .bind(source_id)
                .bind(target_id)
                .execute(&mut *tx)
                .await
                .map_err(internal)?
                .rows_affected();
        if deleted == 1 {
            // Only page links have a relation; for the others this is a no-op.
            self.access
                .revoke(
                    &mut tx,
                    RelationTuple::new(source_id, Relation::LinkedTo, target_id),
                )
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(())
    }

    /// The links of an item `actor` can view, in both directions. A link whose other end they
    /// cannot view, or which is in the trash, is left out entirely (V9).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if the item is not visible, or [`AppError::Internal`].
    pub async fn links(&self, actor: Uuid, id: Uuid) -> Result<Vec<ItemLink>, AppError> {
        self.require(actor, id, Perms::VIEW).await?;
        let visible = self
            .access
            .permissions()
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        let rows = sqlx::query_as::<_, LinkRow>(sqlx::AssertSqlSafe(format!(
            "SELECT l.link_id, l.link_created_at, l.outgoing, {COLUMNS}
             FROM (SELECT id AS link_id, created_at AS link_created_at,
                          source_item_id = $1 AS outgoing,
                          CASE WHEN source_item_id = $1 THEN target_item_id
                               ELSE source_item_id END AS other_id
                   FROM item_links
                   WHERE source_item_id = $1 OR target_item_id = $1) l
             JOIN items ON items.id = l.other_id
             WHERE items.id = ANY($2) AND items.deleted_at IS NULL
             ORDER BY l.link_created_at, l.link_id
             LIMIT $3"
        )))
        .bind(id)
        .bind(&visible)
        .bind(LIST_LIMIT)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(ItemLink::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b1_link_kinds() {
        for target in ["task", "routine", "goal", "tracker"] {
            assert!(link_allowed("page", target), "page -> {target}");
        }
        for source in ["task", "routine", "tracker"] {
            assert!(link_allowed(source, "goal"), "{source} -> goal");
            assert!(!link_allowed(source, "task"), "{source} -> task");
            assert!(!link_allowed(source, "page"), "{source} -> page");
        }
        for (source, target) in [
            ("page", "page"),
            ("page", "notebook"),
            ("goal", "goal"),
            ("goal", "task"),
            ("notebook", "goal"),
            ("module:budget", "goal"),
        ] {
            assert!(!link_allowed(source, target), "{source} -> {target}");
        }
    }
}
