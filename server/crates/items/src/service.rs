//! `ItemsService`: create, read, update, archive, trash, restore and purge (PRD N1–N4, V6,
//! T1.2.1). Every authorization goes through `access::PermissionService`.

use chrono::{DateTime, Duration, Utc};
use db::PgPool;
use kernel::{AppError, SharedClock};
use sqlx::FromRow;
use uuid::Uuid;

use access::{AccessEngine, Perms, Relation, RelationTuple};

use crate::context::ItemsAccessContext;
use crate::model::{Item, ItemChanges, ListState, NewItem, Owner};

/// How long an item stays in the trash before it is purged (N4).
pub const TRASH_RETENTION: Duration = Duration::days(30);

/// Most items one list returns; filters and paging come with T1.2.7.
pub const LIST_LIMIT: i64 = 500;

/// Items purged per batch of the daily job.
const PURGE_BATCH: i64 = 200;

const COLUMNS: &str = "id, kind, title, description, owner_user_id, owner_team_id, created_by, \
                       created_at, updated_at, last_touched_at, archived_at, deleted_at";

#[derive(FromRow)]
struct ItemRow {
    id: Uuid,
    kind: String,
    title: String,
    description: Option<String>,
    owner_user_id: Option<Uuid>,
    owner_team_id: Option<Uuid>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    last_touched_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
    deleted_at: Option<DateTime<Utc>>,
}

impl From<ItemRow> for Item {
    fn from(row: ItemRow) -> Self {
        let owner = match (row.owner_user_id, row.owner_team_id) {
            (Some(user_id), _) => Owner::User { user_id },
            (None, Some(team_id)) => Owner::Team { team_id },
            // Excluded by items_owner_check.
            (None, None) => Owner::User {
                user_id: row.created_by,
            },
        };
        Self {
            id: row.id,
            kind: row.kind,
            title: row.title,
            description: row.description,
            owner,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_touched_at: row.last_touched_at,
            archived_at: row.archived_at,
            deleted_at: row.deleted_at,
        }
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct ItemsService {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
    access: AccessEngine<ItemsAccessContext>,
}

impl ItemsService {
    pub fn new(pool: PgPool, clock: SharedClock, access: AccessEngine<ItemsAccessContext>) -> Self {
        Self {
            pool,
            clock,
            access,
        }
    }

    /// Creates a personal item owned by `actor`; the owner relation is written in the same
    /// transaction (N1).
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn create(&self, actor: Uuid, new: NewItem) -> Result<Item, AppError> {
        let now = self.clock.now();
        let id = kernel::Id::<()>::new_at(now).as_uuid();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "INSERT INTO items (id, kind, title, description, owner_user_id, created_by,
                                created_at, updated_at, last_touched_at)
             VALUES ($1, $2, $3, $4, $5, $5, $6, $6, $6)
             RETURNING {COLUMNS}"
        )))
        .bind(id)
        .bind(&new.kind)
        .bind(&new.title)
        .bind(&new.description)
        .bind(actor)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        self.access
            .grant(&mut tx, RelationTuple::new(id, Relation::Owner, actor))
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(row.into())
    }

    /// The item, if `actor` may view it; otherwise [`AppError::NotFound`] (V9).
    ///
    /// # Errors
    /// [`AppError::NotFound`], or [`AppError::Internal`] on database errors.
    pub async fn get(&self, actor: Uuid, id: Uuid) -> Result<Item, AppError> {
        self.require(actor, id, Perms::VIEW).await?;
        self.load(id).await?.ok_or(AppError::NotFound)
    }

    /// Items `actor` can view in `state`, newest first ([`ListState::Trash`]: those they may
    /// delete, i.e. restore).
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn list(&self, actor: Uuid, state: ListState) -> Result<Vec<Item>, AppError> {
        self.list_where(actor, state, None).await
    }

    /// [`Self::list`], optionally limited to items carrying `tag`. The caller checks that the
    /// tag is visible to `actor`.
    pub(crate) async fn list_where(
        &self,
        actor: Uuid,
        state: ListState,
        tag: Option<Uuid>,
    ) -> Result<Vec<Item>, AppError> {
        let (required, filter) = match state {
            ListState::Active => (Perms::VIEW, "deleted_at IS NULL AND archived_at IS NULL"),
            ListState::Archived => (
                Perms::VIEW,
                "deleted_at IS NULL AND archived_at IS NOT NULL",
            ),
            ListState::Trash => (Perms::DELETE, "deleted_at IS NOT NULL"),
        };
        let ids = self
            .access
            .permissions()
            .list_accessible(&self.pool, actor, required)
            .await
            .map_err(internal)?;
        let rows = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM items WHERE id = ANY($1) AND {filter}
               AND ($3::uuid IS NULL
                    OR id IN (SELECT item_id FROM item_tags WHERE tag_id = $3))
             ORDER BY created_at DESC, id DESC LIMIT $2"
        )))
        .bind(&ids)
        .bind(LIST_LIMIT)
        .bind(tag)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(Item::from).collect())
    }

    /// Changes title and/or description (EDIT). Items in the trash must be restored first.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`], [`AppError::Conflict`] for an item in
    /// the trash, or [`AppError::Internal`].
    pub async fn update(
        &self,
        actor: Uuid,
        id: Uuid,
        changes: ItemChanges,
    ) -> Result<Item, AppError> {
        self.require(actor, id, Perms::EDIT).await?;
        let (set_description, description) = match changes.description {
            Some(d) => (true, d),
            None => (false, None),
        };
        let row = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "UPDATE items SET title = COALESCE($2, title),
                              description = CASE WHEN $3 THEN $4 ELSE description END,
                              updated_at = $5
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {COLUMNS}"
        )))
        .bind(id)
        .bind(&changes.title)
        .bind(set_description)
        .bind(&description)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        self.or_conflict(id, row).await
    }

    /// Archives (`true`) or unarchives (`false`) an item (MANAGE). Archiving is a reversible
    /// state of its own, separate from the trash (N4).
    ///
    /// # Errors
    /// As [`Self::update`].
    pub async fn set_archived(
        &self,
        actor: Uuid,
        id: Uuid,
        archived: bool,
    ) -> Result<Item, AppError> {
        self.require(actor, id, Perms::MANAGE).await?;
        let now = self.clock.now();
        let row = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "UPDATE items SET archived_at = CASE WHEN $2 THEN COALESCE(archived_at, $3) END,
                              updated_at = $3
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {COLUMNS}"
        )))
        .bind(id)
        .bind(archived)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        self.or_conflict(id, row).await
    }

    /// Moves an item to the trash (`true`) or restores it (`false`). Only the owner may (DELETE,
    /// V6). Trashing twice keeps the first time, so the 30 days do not restart.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`] or [`AppError::Internal`].
    pub async fn set_trashed(
        &self,
        actor: Uuid,
        id: Uuid,
        trashed: bool,
    ) -> Result<Item, AppError> {
        self.require(actor, id, Perms::DELETE).await?;
        let now = self.clock.now();
        let row = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "UPDATE items SET deleted_at = CASE WHEN $2 THEN COALESCE(deleted_at, $3) END,
                              updated_at = $3
             WHERE id = $1
             RETURNING {COLUMNS}"
        )))
        .bind(id)
        .bind(trashed)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        row.map(Item::from).ok_or(AppError::NotFound)
    }

    /// Permanently deletes items whose trash time ran out before `now` (N4), with their access
    /// relations. Returns how many were purged.
    ///
    /// # Errors
    /// Database errors.
    pub async fn purge_expired(&self, now: DateTime<Utc>) -> sqlx::Result<usize> {
        let cutoff = now - TRASH_RETENTION;
        let mut purged = 0;
        loop {
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT id FROM items WHERE deleted_at < $1 ORDER BY deleted_at LIMIT $2",
            )
            .bind(cutoff)
            .bind(PURGE_BATCH)
            .fetch_all(&self.pool)
            .await?;
            if ids.is_empty() {
                return Ok(purged);
            }
            for id in ids {
                let mut tx = self.pool.begin().await?;
                // Re-checked inside the transaction: a restore in the meantime wins.
                let deleted = sqlx::query("DELETE FROM items WHERE id = $1 AND deleted_at < $2")
                    .bind(id)
                    .bind(cutoff)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                if deleted == 1 {
                    self.access.remove_item(&mut tx, id).await?;
                    purged += 1;
                }
                tx.commit().await?;
            }
        }
    }

    pub(crate) async fn require(
        &self,
        actor: Uuid,
        id: Uuid,
        perms: Perms,
    ) -> Result<(), AppError> {
        self.access
            .permissions()
            .require(&self.pool, actor, id, perms)
            .await
    }

    pub(crate) async fn load(&self, id: Uuid) -> Result<Option<Item>, AppError> {
        let row = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM items WHERE id = $1"
        )))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        Ok(row.map(Item::from))
    }

    /// `None` from a guarded UPDATE: the item is gone or in the trash.
    async fn or_conflict(&self, id: Uuid, row: Option<ItemRow>) -> Result<Item, AppError> {
        match row {
            Some(row) => Ok(row.into()),
            None if self.load(id).await?.is_some() => Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            )),
            None => Err(AppError::NotFound),
        }
    }
}
