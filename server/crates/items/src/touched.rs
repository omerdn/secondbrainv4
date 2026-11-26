//! Items by when they were last touched (PRD H4, H5, T2.7.1): the forgotten ones and the ones
//! to pick up again. Only active items the caller can view; `last_touched_at` counts edits,
//! completions and the like, never reads (ARCHITECTURE §4).

use access::Perms;
use chrono::{DateTime, Utc};
use kernel::AppError;
use uuid::Uuid;

use crate::model::Item;
use crate::service::{COLUMNS, ItemRow, ItemsService};

/// Which end of the touch history to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touched {
    /// Untouched since before this time, longest untouched first (H4).
    Before(DateTime<Utc>),
    /// Most recently touched first (H5).
    Latest,
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl ItemsService {
    /// Active items of `kinds` that `actor` can view, by `touched`, at most `limit`.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn by_touch(
        &self,
        actor: Uuid,
        kinds: &[&str],
        touched: Touched,
        limit: i64,
    ) -> Result<Vec<Item>, AppError> {
        let visible = self
            .access
            .permissions()
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        let (before, order) = match touched {
            Touched::Before(at) => (Some(at), "last_touched_at ASC, id ASC"),
            Touched::Latest => (None, "last_touched_at DESC, id DESC"),
        };
        let kinds: Vec<String> = kinds.iter().map(|kind| (*kind).to_owned()).collect();
        let rows = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM items
             WHERE id = ANY($1) AND deleted_at IS NULL AND archived_at IS NULL
               AND kind = ANY($2)
               AND ($3::timestamptz IS NULL OR last_touched_at < $3)
             ORDER BY {order}
             LIMIT $4"
        )))
        .bind(&visible)
        .bind(&kinds)
        .bind(before)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(Item::from).collect())
    }
}
