//! `PermissionService`: the only place other modules ask "may this user do that?" (ARCHITECTURE
//! §6.6, PRD V7, V11, T1.1.5). Reads the derived `item_access` cache; `preview` runs
//! `compute_access` as a what-if.

use std::collections::BTreeSet;
use std::sync::Arc;

use kernel::AppError;
use serde::Serialize;
use sqlx::{PgConnection, PgExecutor};
use ts_rs::TS;
use uuid::Uuid;

use crate::affected::Change;
use crate::compute::{Reason, compute_access};
use crate::model::{Perms, RelationTuple};
use crate::recompute::{AccessContext, Loader};
use crate::repo;

/// One permission, as the API names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AccessPermission")]
pub enum Permission {
    View,
    Edit,
    Share,
    Link,
    Manage,
    Delete,
}

impl Permission {
    const ALL: [(Self, Perms); 6] = [
        (Self::View, Perms::VIEW),
        (Self::Edit, Perms::EDIT),
        (Self::Share, Perms::SHARE),
        (Self::Link, Perms::LINK),
        (Self::Manage, Perms::MANAGE),
        (Self::Delete, Perms::DELETE),
    ];

    /// The permissions in `perms`, in a fixed order.
    #[must_use]
    pub fn list(perms: Perms) -> Vec<Self> {
        Self::ALL
            .iter()
            .filter(|(_, p)| perms.contains(*p))
            .map(|(name, _)| *name)
            .collect()
    }
}

/// "Why am I seeing this?" (V11): what the user may do on an item and every path that grants it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AccessExplanation")]
pub struct Explanation {
    pub permissions: Vec<Permission>,
    /// Owner, member roles, team role, notebook or link (with the linked item).
    pub reasons: Vec<Reason>,
}

/// Users who would gain access to one item if an action went ahead (V7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AccessPreviewItem")]
pub struct PreviewItem {
    pub item_id: Uuid,
    /// Users who cannot view the item now and could afterwards, sorted.
    pub new_viewers: Vec<Uuid>,
}

/// Read side of the access engine. Get one from [`crate::AccessEngine::permissions`].
pub struct PermissionService<C> {
    ctx: Arc<C>,
}

impl<C> Clone for PermissionService<C> {
    fn clone(&self) -> Self {
        Self {
            ctx: Arc::clone(&self.ctx),
        }
    }
}

impl<C: AccessContext> PermissionService<C> {
    pub(crate) fn new(ctx: Arc<C>) -> Self {
        Self { ctx }
    }

    /// Whether `user` has every permission in `perms` on `item`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn check(
        &self,
        conn: impl PgExecutor<'_>,
        user: Uuid,
        item: Uuid,
        perms: Perms,
    ) -> sqlx::Result<bool> {
        Ok(repo::item_access(conn, item, user)
            .await?
            .is_some_and(|a| a.perms.contains(perms)))
    }

    /// Like [`Self::check`], as the error an endpoint returns: [`AppError::NotFound`] if the user
    /// cannot even view the item (its existence is not revealed, V9), [`AppError::Forbidden`] if
    /// they can view it but lack `perms`.
    ///
    /// # Errors
    /// As described, or [`AppError::Internal`] on database errors.
    pub async fn require(
        &self,
        conn: impl PgExecutor<'_>,
        user: Uuid,
        item: Uuid,
        perms: Perms,
    ) -> Result<(), AppError> {
        let access = repo::item_access(conn, item, user)
            .await
            .map_err(|err| AppError::Internal(Box::new(err)))?;
        match access {
            Some(a) if a.perms.contains(perms) => Ok(()),
            Some(a) if a.perms.contains(Perms::VIEW) => Err(AppError::Forbidden),
            _ => Err(AppError::NotFound),
        }
    }

    /// Ids of the items on which `user` has at least `required`, sorted. Listing queries in other
    /// modules filter by this set (or by the same rule in a JOIN through a future API).
    ///
    /// # Errors
    /// Database errors.
    pub async fn list_accessible(
        &self,
        conn: impl PgExecutor<'_>,
        user: Uuid,
        required: Perms,
    ) -> sqlx::Result<Vec<Uuid>> {
        repo::accessible_item_ids(conn, user, required).await
    }

    /// Why `user` can access `item`, or `None` if they cannot (V11).
    ///
    /// # Errors
    /// Database errors, or stored reasons this binary cannot read.
    pub async fn explain(
        &self,
        conn: impl PgExecutor<'_>,
        user: Uuid,
        item: Uuid,
    ) -> sqlx::Result<Option<Explanation>> {
        let Some(access) = repo::item_access(conn, item, user).await? else {
            return Ok(None);
        };
        let reasons: Vec<Reason> = serde_json::from_value(access.reasons)
            .map_err(|err| sqlx::Error::Decode(Box::new(err)))?;
        Ok(Some(Explanation {
            permissions: Permission::list(access.perms),
            reasons,
        }))
    }

    /// Who would gain access if `tuple` were added (linking a page, adding a member): every item
    /// whose viewers would grow, with the new viewers (V7). Nothing is written. Authorization of
    /// the action itself (V4, V5) is the caller's job, via [`Self::require`].
    ///
    /// # Errors
    /// Database errors.
    pub async fn preview(
        &self,
        conn: &mut PgConnection,
        tuple: RelationTuple,
    ) -> sqlx::Result<Vec<PreviewItem>> {
        let change = Change::Relation(tuple);
        let mut loader = Loader::default();
        let affected = loader.affected(conn, &change).await?;
        loader.load_compute_inputs(conn, &affected).await?;
        // The tuple's object is affected, so its relations are loaded now: an action that
        // already happened gives nobody new access.
        if loader.graph.contains(tuple) {
            return Ok(Vec::new());
        }
        // Load what the new relation brings in (e.g. the linked item's members), then compare
        // the graph with and without it.
        loader.graph.add(tuple);
        loader.load_compute_inputs(conn, &affected).await?;
        loader.load_context(conn, &*self.ctx).await?;
        let after = &loader.graph;
        let mut before = after.clone();
        before.remove(tuple);
        let mut preview = Vec::new();
        for &item in &affected {
            let viewers = |graph| -> BTreeSet<Uuid> {
                compute_access(graph, item)
                    .into_iter()
                    .filter(|(_, a)| a.perms.contains(Perms::VIEW))
                    .map(|(user, _)| user)
                    .collect()
            };
            let new_viewers: Vec<Uuid> = viewers(after)
                .difference(&viewers(&before))
                .copied()
                .collect();
            if !new_viewers.is_empty() {
                preview.push(PreviewItem {
                    item_id: item,
                    new_viewers,
                });
            }
        }
        Ok(preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_are_listed_in_order() {
        assert_eq!(
            Permission::list(Perms::DELETE | Perms::VIEW),
            [Permission::View, Permission::Delete]
        );
        assert!(Permission::list(Perms::NONE).is_empty());
        assert_eq!(Permission::list(Perms::ALL).len(), 6);
    }
}
