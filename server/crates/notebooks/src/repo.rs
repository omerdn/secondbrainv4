//! SQL for the `notebooks` and `pages` tables; nothing else in the crate touches them, and they
//! touch no other crate's tables (ADR-0007).

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRow)]
pub struct NotebookRow {
    pub item_id: Uuid,
    pub default_for: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRow)]
pub struct PageRow {
    pub item_id: Uuid,
    pub notebook_id: Uuid,
    pub process_of_item_id: Option<Uuid>,
}

/// Serializes default-notebook creation for one user until the transaction ends.
///
/// # Errors
/// Database errors.
pub async fn lock_user(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('notebooks:default:' || $1::text, 0))",
    )
    .bind(user)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn default_of(conn: impl PgExecutor<'_>, user: Uuid) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT item_id FROM notebooks WHERE default_for = $1")
        .bind(user)
        .fetch_optional(conn)
        .await
}

/// # Errors
/// Database errors.
pub async fn insert_notebook(
    conn: &mut PgConnection,
    row: NotebookRow,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO notebooks (item_id, default_for, created_at, updated_at) VALUES ($1, $2, $3, $3)",
    )
    .bind(row.item_id)
    .bind(row.default_for)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn get_notebook(
    conn: impl PgExecutor<'_>,
    id: Uuid,
) -> sqlx::Result<Option<NotebookRow>> {
    sqlx::query_as("SELECT item_id, default_for FROM notebooks WHERE item_id = $1")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Notebooks among `ids`, oldest first.
///
/// # Errors
/// Database errors.
pub async fn notebooks_among(
    conn: impl PgExecutor<'_>,
    ids: &[Uuid],
) -> sqlx::Result<Vec<NotebookRow>> {
    sqlx::query_as(
        "SELECT item_id, default_for FROM notebooks WHERE item_id = ANY($1)
         ORDER BY default_for IS NULL, created_at, item_id",
    )
    .bind(ids)
    .fetch_all(conn)
    .await
}

/// # Errors
/// Database errors.
pub async fn insert_page(
    conn: &mut PgConnection,
    row: PageRow,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO pages (item_id, notebook_id, process_of_item_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $4)",
    )
    .bind(row.item_id)
    .bind(row.notebook_id)
    .bind(row.process_of_item_id)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn get_page(conn: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<Option<PageRow>> {
    sqlx::query_as("SELECT item_id, notebook_id, process_of_item_id FROM pages WHERE item_id = $1")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// A notebook's pages, oldest first.
///
/// # Errors
/// Database errors.
pub async fn pages_of(conn: impl PgExecutor<'_>, notebook: Uuid) -> sqlx::Result<Vec<PageRow>> {
    sqlx::query_as(
        "SELECT item_id, notebook_id, process_of_item_id FROM pages WHERE notebook_id = $1
         ORDER BY created_at, item_id",
    )
    .bind(notebook)
    .fetch_all(conn)
    .await
}
