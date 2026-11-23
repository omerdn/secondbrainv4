//! Task storage (ARCHITECTURE §5.4). Only this crate touches these tables (ADR-0007).

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::model::SubtaskView;

/// A `tasks` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRow)]
pub struct TaskRow {
    pub item_id: Uuid,
    pub start_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
}

#[derive(FromRow)]
struct SubtaskRow {
    id: Uuid,
    task_id: Uuid,
    title: String,
    position: i32,
    done_at: Option<DateTime<Utc>>,
    done_by: Option<Uuid>,
    assignee_ids: Vec<Uuid>,
}

impl From<SubtaskRow> for SubtaskView {
    fn from(row: SubtaskRow) -> Self {
        Self {
            id: row.id,
            task_id: row.task_id,
            title: row.title,
            position: row.position,
            done_at: row.done_at,
            done_by: row.done_by,
            assignee_ids: row.assignee_ids,
        }
    }
}

const SUBTASK_SELECT: &str = "SELECT s.id, s.task_id, s.title, s.position, s.done_at, s.done_by,
        COALESCE((SELECT array_agg(a.user_id ORDER BY a.user_id)
                  FROM subtask_assignees a WHERE a.subtask_id = s.id),
                 '{}'::uuid[]) AS assignee_ids
 FROM subtasks s";

/// # Errors
/// Database errors.
pub async fn insert_task(
    conn: &mut PgConnection,
    item_id: Uuid,
    (start_date, due_date): (Option<NaiveDate>, Option<NaiveDate>),
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO tasks (item_id, start_date, due_date, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $4)",
    )
    .bind(item_id)
    .bind(start_date)
    .bind(due_date)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn get_task(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<TaskRow>> {
    sqlx::query_as("SELECT item_id, start_date, due_date FROM tasks WHERE item_id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Locks the task for the rest of the transaction, so subtask changes of one task run one at a
/// time and the G2 invariant holds under concurrency.
///
/// # Errors
/// Database errors.
pub async fn lock_task(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<TaskRow>> {
    sqlx::query_as("SELECT item_id, start_date, due_date FROM tasks WHERE item_id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// The task row inside a transaction that already holds its lock.
///
/// # Errors
/// [`kernel::AppError::NotFound`] if the task is gone, or [`kernel::AppError::Internal`].
pub async fn get_locked(conn: &mut PgConnection, id: Uuid) -> Result<TaskRow, kernel::AppError> {
    lock_task(conn, id)
        .await
        .map_err(|err| kernel::AppError::Internal(Box::new(err)))?
        .ok_or(kernel::AppError::NotFound)
}

/// # Errors
/// Database errors.
pub async fn update_dates(
    conn: &mut PgConnection,
    id: Uuid,
    (start_date, due_date): (Option<NaiveDate>, Option<NaiveDate>),
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE tasks SET start_date = $2, due_date = $3, updated_at = $4 WHERE item_id = $1",
    )
    .bind(id)
    .bind(start_date)
    .bind(due_date)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// Appends a subtask after the task's last one. Call with the task locked.
///
/// # Errors
/// Database errors.
pub async fn insert_subtask(
    conn: &mut PgConnection,
    task_id: Uuid,
    title: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<Uuid> {
    let id = kernel::Id::<()>::new_at(now).as_uuid();
    sqlx::query(
        "INSERT INTO subtasks (id, task_id, title, position, created_at, updated_at)
         VALUES ($1, $2, $3,
                 (SELECT COALESCE(MAX(position) + 1, 0) FROM subtasks WHERE task_id = $2),
                 $4, $4)",
    )
    .bind(id)
    .bind(task_id)
    .bind(title)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(id)
}

/// The task's subtasks in order.
///
/// # Errors
/// Database errors.
pub async fn list_subtasks(pool: &PgPool, task_id: Uuid) -> sqlx::Result<Vec<SubtaskView>> {
    let rows = sqlx::query_as::<_, SubtaskRow>(sqlx::AssertSqlSafe(format!(
        "{SUBTASK_SELECT} WHERE s.task_id = $1 ORDER BY s.position"
    )))
    .bind(task_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(SubtaskView::from).collect())
}

/// One subtask of the task; a subtask of another task is not found.
///
/// # Errors
/// Database errors.
pub async fn get_subtask(
    conn: &mut PgConnection,
    task_id: Uuid,
    id: Uuid,
) -> sqlx::Result<Option<SubtaskView>> {
    let row = sqlx::query_as::<_, SubtaskRow>(sqlx::AssertSqlSafe(format!(
        "{SUBTASK_SELECT} WHERE s.task_id = $1 AND s.id = $2"
    )))
    .bind(task_id)
    .bind(id)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(SubtaskView::from))
}

/// # Errors
/// Database errors.
pub async fn count_subtasks(conn: &mut PgConnection, task_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM subtasks WHERE task_id = $1")
        .bind(task_id)
        .fetch_one(conn)
        .await
}

/// How many subtasks are done, and how many there are.
///
/// # Errors
/// Database errors.
pub async fn subtask_counts(pool: &PgPool, task_id: Uuid) -> sqlx::Result<(u32, u32)> {
    let (done, total): (i32, i32) = sqlx::query_as(
        "SELECT (count(*) FILTER (WHERE done_at IS NOT NULL))::int4, count(*)::int4
         FROM subtasks WHERE task_id = $1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await?;
    Ok((done.unsigned_abs(), total.unsigned_abs()))
}

/// Whether every subtask of the task is done.
///
/// # Errors
/// Database errors.
pub async fn all_done(conn: &mut PgConnection, task_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM subtasks WHERE task_id = $1 AND done_at IS NULL)",
    )
    .bind(task_id)
    .fetch_one(conn)
    .await
}

/// # Errors
/// Database errors.
pub async fn rename_subtask(
    conn: &mut PgConnection,
    task_id: Uuid,
    id: Uuid,
    title: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE subtasks SET title = $3, updated_at = $4 WHERE task_id = $1 AND id = $2",
    )
    .bind(task_id)
    .bind(id)
    .bind(title)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// # Errors
/// Database errors.
pub async fn delete_subtask(
    conn: &mut PgConnection,
    task_id: Uuid,
    id: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM subtasks WHERE task_id = $1 AND id = $2")
        .bind(task_id)
        .bind(id)
        .execute(conn)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Marks a subtask done; one that is already done keeps its original time and person. Returns
/// whether this call completed it.
///
/// # Errors
/// Database errors.
pub async fn mark_done(
    conn: &mut PgConnection,
    task_id: Uuid,
    id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE subtasks SET done_at = $4, done_by = $3, updated_at = $4
         WHERE task_id = $1 AND id = $2 AND done_at IS NULL",
    )
    .bind(task_id)
    .bind(id)
    .bind(actor)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Returns whether this call reopened the subtask.
///
/// # Errors
/// Database errors.
pub async fn mark_undone(
    conn: &mut PgConnection,
    task_id: Uuid,
    id: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE subtasks SET done_at = NULL, done_by = NULL, updated_at = $3
         WHERE task_id = $1 AND id = $2 AND done_at IS NOT NULL",
    )
    .bind(task_id)
    .bind(id)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Returns whether the person was newly assigned.
///
/// # Errors
/// Database errors.
pub async fn add_assignee(
    conn: &mut PgConnection,
    subtask: Uuid,
    user: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "INSERT INTO subtask_assignees (subtask_id, user_id, created_at) VALUES ($1, $2, $3)
         ON CONFLICT (subtask_id, user_id) DO NOTHING",
    )
    .bind(subtask)
    .bind(user)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// # Errors
/// Database errors.
pub async fn remove_assignee(
    conn: &mut PgConnection,
    subtask: Uuid,
    user: Uuid,
) -> sqlx::Result<bool> {
    let result =
        sqlx::query("DELETE FROM subtask_assignees WHERE subtask_id = $1 AND user_id = $2")
            .bind(subtask)
            .bind(user)
            .execute(conn)
            .await?;
    Ok(result.rows_affected() == 1)
}
