//! `GoalsService`: goals and the items linked to them (PRD D1-D3, T2.3.1). Every authorization
//! goes through `access::PermissionService`; items and links come from `ItemsService`
//! (ADR-0007), so a link follows the item rules (B1, V4) and a hidden item is never shown (V9).

use std::collections::HashMap;
use std::sync::Arc;

use access::{PermissionService, Perms};
use items::{CreateItemRequest, ItemsAccessContext, ItemsService, LinkDirection, TouchAction};
use kernel::{AppError, FieldErrorCode, SharedClock};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{
    CreateGoalRequest, Goal, GoalLink, LINKABLE_KINDS, LinkGoalRequest, UpdateGoalRequest,
    check_dates, check_weight, contributes, invalid,
};
use crate::progress::Providers;
use crate::repo::{self, GoalRow};
use tracking::ProgressProvider;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct GoalsService {
    pool: PgPool,
    pub(crate) clock: SharedClock,
    permissions: PermissionService<ItemsAccessContext>,
    items: ItemsService,
    pub(crate) providers: Arc<Providers>,
}

impl GoalsService {
    #[must_use]
    pub fn new(
        pool: PgPool,
        clock: SharedClock,
        permissions: PermissionService<ItemsAccessContext>,
        items: ItemsService,
    ) -> Self {
        Self {
            pool,
            clock,
            permissions,
            items,
            providers: Arc::new(HashMap::new()),
        }
    }

    /// Registers the module that measures items of `kind` (ADR-0006). Items of a kind without a
    /// provider count as not started.
    #[must_use]
    pub fn with_progress(mut self, kind: &str, provider: Arc<dyn ProgressProvider>) -> Self {
        Arc::make_mut(&mut self.providers).insert(kind.to_owned(), provider);
        self
    }

    /// Creates a personal goal and its item in one transaction. Both dates are required (D1).
    ///
    /// # Errors
    /// [`AppError::Validation`] or [`AppError::Internal`].
    pub async fn create(&self, actor: Uuid, request: CreateGoalRequest) -> Result<Goal, AppError> {
        let (start_date, end_date) = check_dates(request.start_date, request.end_date)?;
        let new_item = CreateItemRequest {
            kind: "goal".into(),
            title: request.title,
            description: request.description,
        }
        .validate()?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let item = self.items.create_in(&mut tx, actor, new_item).await?;
        let row = GoalRow {
            item_id: item.id,
            start_date,
            end_date,
        };
        repo::insert_goal(&mut tx, row, self.clock.now())
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.goal(actor, item.id).await
    }

    /// A goal with the linked items the caller can see (VIEW). Links to items they cannot see
    /// are left out entirely, count included (V9).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not visible or not a goal, or [`AppError::Internal`].
    pub async fn goal(&self, actor: Uuid, id: Uuid) -> Result<Goal, AppError> {
        let item = self.items.get(actor, id).await?;
        let row = repo::get_goal(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let weights: HashMap<Uuid, Decimal> = repo::weights(&self.pool, id)
            .await
            .map_err(internal)?
            .into_iter()
            .map(|(item, weight)| (item, weight.normalize()))
            .collect();
        let links = self
            .items
            .links(actor, id)
            .await?
            .into_iter()
            .filter(|link| {
                link.direction == LinkDirection::Incoming
                    && LINKABLE_KINDS.contains(&link.item.kind.as_str())
            })
            .map(|link| GoalLink {
                weight: weights.get(&link.item.id).copied().unwrap_or(Decimal::ONE),
                contributes: contributes(&link.item.kind),
                item: link.item,
            })
            .collect();
        Ok(Goal {
            item,
            start_date: row.start_date,
            end_date: row.end_date,
            links,
        })
    }

    /// Moves the goal's dates (EDIT). They cannot be cleared (D1).
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`], [`AppError::Validation`],
    /// [`AppError::Conflict`] for a goal in the trash, or [`AppError::Internal`].
    pub async fn update(
        &self,
        actor: Uuid,
        id: Uuid,
        request: UpdateGoalRequest,
    ) -> Result<Goal, AppError> {
        self.require_live(actor, id).await?;
        if request.start_date.is_none() && request.end_date.is_none() {
            return Err(invalid(
                "start_date",
                FieldErrorCode::Required,
                "nothing to change",
            ));
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let mut row = repo::lock_goal(&mut tx, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let (start_date, end_date) = check_dates(
            request.start_date.or(Some(row.start_date)),
            request.end_date.or(Some(row.end_date)),
        )?;
        row.start_date = start_date;
        row.end_date = end_date;
        repo::update_goal(&mut tx, row, self.clock.now())
            .await
            .map_err(internal)?;
        self.items
            .touch(&mut tx, id, TouchAction::Edited)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.goal(actor, id).await
    }

    /// Links `item` to the goal, or changes the weight of an existing link (D2, D3). Needs EDIT
    /// on the goal, and the item rules for a link: LINK on the item (V4). Repeating it is
    /// harmless.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a goal or item the caller cannot see (V9),
    /// [`AppError::Forbidden`], [`AppError::Validation`] for a kind that cannot be linked or a
    /// bad weight, [`AppError::Conflict`] for an item in the trash, or [`AppError::Internal`].
    pub async fn link(
        &self,
        actor: Uuid,
        goal: Uuid,
        item_id: Uuid,
        request: LinkGoalRequest,
    ) -> Result<Goal, AppError> {
        self.require_live(actor, goal).await?;
        let item = self.items.get(actor, item_id).await?;
        if !LINKABLE_KINDS.contains(&item.kind.as_str()) {
            return Err(invalid(
                "item_id",
                FieldErrorCode::Invalid,
                "only tasks, routines, trackers and pages can be linked to a goal",
            ));
        }
        let weight = match request.weight {
            Some(_) if !contributes(&item.kind) => {
                return Err(invalid(
                    "weight",
                    FieldErrorCode::Invalid,
                    "a page does not count towards the goal",
                ));
            }
            Some(weight) => Some(check_weight(weight)?),
            None => None,
        };
        self.items.link(actor, item_id, goal).await?;
        if let Some(weight) = weight {
            let mut tx = self.pool.begin().await.map_err(internal)?;
            repo::set_weight(&mut tx, (goal, item_id), weight, self.clock.now())
                .await
                .map_err(internal)?;
            self.items
                .touch(&mut tx, goal, TouchAction::Edited)
                .await
                .map_err(internal)?;
            tx.commit().await.map_err(internal)?;
        }
        self.goal(actor, goal).await
    }

    /// Removes the link from `item` to the goal (EDIT on the goal, LINK on the item). Removing
    /// an absent link changes nothing.
    ///
    /// # Errors
    /// As [`Self::link`].
    pub async fn unlink(&self, actor: Uuid, goal: Uuid, item_id: Uuid) -> Result<Goal, AppError> {
        self.require_live(actor, goal).await?;
        self.items.unlink(actor, item_id, goal).await?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        repo::delete_weight(&mut tx, (goal, item_id))
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.goal(actor, goal).await
    }

    /// EDIT on a goal that is not in the trash. An item that is not a goal is not found.
    async fn require_live(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        self.permissions
            .require(&self.pool, actor, id, Perms::EDIT)
            .await?;
        let item = self.items.get(actor, id).await?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        repo::get_goal(&self.pool, id)
            .await
            .map_err(internal)?
            .map(|_| ())
            .ok_or(AppError::NotFound)
    }
}
