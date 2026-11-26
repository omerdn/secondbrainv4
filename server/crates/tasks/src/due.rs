//! Unfinished tasks due by a day (PRD H2, T2.7.1): what the Today screen shows as overdue and
//! upcoming. Answers from this crate's tables only; items come from `ItemsService` (ADR-0007).

use access::Perms;
use chrono::NaiveDate;
use items::Item;
use kernel::AppError;
use serde::Serialize;
use sqlx::FromRow;
use ts_rs::TS;
use uuid::Uuid;

use crate::service::TasksService;

/// An unfinished task with a due date (H2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct DueTask {
    pub item: Item,
    #[ts(type = "string")]
    pub due_date: NaiveDate,
    #[ts(type = "number")]
    pub done: i64,
    #[ts(type = "number")]
    pub total: i64,
}

#[derive(FromRow)]
struct DueRow {
    item_id: Uuid,
    due_date: NaiveDate,
    done: i64,
    total: i64,
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl TasksService {
    /// Active tasks `actor` can view that are due in `from..=until` (no lower end when `from` is
    /// none) and still have an open subtask, earliest due first, at most `limit` (H2).
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn due_by(
        &self,
        actor: Uuid,
        from: Option<NaiveDate>,
        until: NaiveDate,
        limit: i64,
    ) -> Result<Vec<DueTask>, AppError> {
        let visible = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        // Archived and trashed tasks are dropped after the item lookup, so over-fetch a little.
        let rows: Vec<DueRow> = sqlx::query_as(
            "SELECT t.item_id, t.due_date,
                    count(s.id) FILTER (WHERE s.done_at IS NOT NULL) AS done,
                    count(s.id) AS total
             FROM tasks t JOIN subtasks s ON s.task_id = t.item_id
             WHERE t.item_id = ANY($1) AND t.due_date IS NOT NULL AND t.due_date <= $2
               AND ($4::date IS NULL OR t.due_date >= $4)
             GROUP BY t.item_id, t.due_date
             HAVING count(s.id) FILTER (WHERE s.done_at IS NULL) > 0
             ORDER BY t.due_date, t.item_id
             LIMIT $3",
        )
        .bind(&visible)
        .bind(until)
        .bind(limit.saturating_mul(2))
        .bind(from)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let mut items: std::collections::HashMap<Uuid, Item> = self
            .items
            .visible_items(actor, &ids)
            .await?
            .into_iter()
            .filter(|item| item.archived_at.is_none() && item.deleted_at.is_none())
            .map(|item| (item.id, item))
            .collect();
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                items.remove(&row.item_id).map(|item| DueTask {
                    item,
                    due_date: row.due_date,
                    done: row.done,
                    total: row.total,
                })
            })
            .take(usize::try_from(limit).unwrap_or(0))
            .collect())
    }
}
