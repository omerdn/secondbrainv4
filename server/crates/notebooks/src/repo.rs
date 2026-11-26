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

/// Stores one Yjs update of a page (Y3). Append-only.
///
/// # Errors
/// Database errors.
pub async fn append_update(
    conn: impl PgExecutor<'_>,
    page: Uuid,
    update: &[u8],
    author: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO page_updates (page_id, update, created_by, created_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(page)
    .bind(update)
    .bind(author)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// A page's stored updates, in the order they were written.
///
/// # Errors
/// Database errors.
pub async fn page_updates(conn: impl PgExecutor<'_>, page: Uuid) -> sqlx::Result<Vec<Vec<u8>>> {
    sqlx::query_scalar("SELECT update FROM page_updates WHERE page_id = $1 ORDER BY id")
        .bind(page)
        .fetch_all(conn)
        .await
}

/// A page's latest snapshot: version and state.
///
/// # Errors
/// Database errors.
pub async fn latest_snapshot(
    conn: impl PgExecutor<'_>,
    page: Uuid,
) -> sqlx::Result<Option<(i32, Vec<u8>)>> {
    sqlx::query_as(
        "SELECT version, state FROM page_snapshots WHERE page_id = $1
         ORDER BY version DESC LIMIT 1",
    )
    .bind(page)
    .fetch_optional(conn)
    .await
}

/// A page's stored updates with their ids, in the order they were written.
///
/// # Errors
/// Database errors.
pub async fn page_updates_with_ids(
    conn: impl PgExecutor<'_>,
    page: Uuid,
) -> sqlx::Result<Vec<(i64, Vec<u8>)>> {
    sqlx::query_as("SELECT id, update FROM page_updates WHERE page_id = $1 ORDER BY id")
        .bind(page)
        .fetch_all(conn)
        .await
}

/// Serializes compactions of one page until the transaction ends.
///
/// # Errors
/// Database errors.
pub async fn lock_page_compaction(conn: &mut PgConnection, page: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('notebooks:compact:' || $1::text, 0))",
    )
    .bind(page)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn insert_snapshot(
    conn: &mut PgConnection,
    (page, version): (Uuid, i32),
    state: &[u8],
    plain_text: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO page_snapshots (page_id, version, state, plain_text, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(page)
    .bind(version)
    .bind(state)
    .bind(plain_text)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// Deletes exactly the updates `ids` of `page`, once a snapshot holds them.
///
/// # Errors
/// Database errors.
pub async fn delete_updates(conn: &mut PgConnection, page: Uuid, ids: &[i64]) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM page_updates WHERE page_id = $1 AND id = ANY($2)")
        .bind(page)
        .bind(ids)
        .execute(conn)
        .await?;
    Ok(())
}

/// Pages that have updates waiting to be compacted, at most `limit`.
///
/// # Errors
/// Database errors.
pub async fn pages_with_updates(conn: impl PgExecutor<'_>, limit: i64) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar("SELECT DISTINCT page_id FROM page_updates LIMIT $1")
        .bind(limit)
        .fetch_all(conn)
        .await
}

/// The latest snapshot's plain text of each page among `pages` (search).
///
/// # Errors
/// Database errors.
pub async fn plain_texts(
    conn: impl PgExecutor<'_>,
    pages: &[Uuid],
) -> sqlx::Result<Vec<(Uuid, String)>> {
    sqlx::query_as(
        "SELECT DISTINCT ON (page_id) page_id, plain_text FROM page_snapshots
         WHERE page_id = ANY($1) ORDER BY page_id, version DESC",
    )
    .bind(pages)
    .fetch_all(conn)
    .await
}
