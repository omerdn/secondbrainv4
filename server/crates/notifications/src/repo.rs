//! SQL for `notifications` and `notification_prefs`; nothing else touches them (ADR-0007).

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub(crate) struct NotificationRow {
    pub id: Uuid,
    pub payload: serde_json::Value,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Inserts unless the event already made one for the user; the row when it did.
pub(crate) async fn insert(
    conn: impl PgExecutor<'_>,
    (id, user, kind): (Uuid, Uuid, &str),
    payload: &serde_json::Value,
    source_event: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<NotificationRow>> {
    sqlx::query_as(
        "INSERT INTO notifications (id, user_id, type, payload, source_event_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (source_event_id, user_id) DO NOTHING
         RETURNING id, payload, read_at, created_at",
    )
    .bind(id)
    .bind(user)
    .bind(kind)
    .bind(payload)
    .bind(source_event)
    .bind(now)
    .fetch_optional(conn)
    .await
}

pub(crate) async fn page(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> sqlx::Result<Vec<NotificationRow>> {
    sqlx::query_as(
        "SELECT id, payload, read_at, created_at FROM notifications
         WHERE user_id = $1 AND ($2::timestamptz IS NULL OR created_at < $2)
         ORDER BY created_at DESC, id DESC
         LIMIT $3",
    )
    .bind(user)
    .bind(before)
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub(crate) async fn unread(conn: impl PgExecutor<'_>, user: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL")
        .bind(user)
        .fetch_one(conn)
        .await
}

/// Marks the user's notification read; the row, none when it is not theirs.
pub(crate) async fn mark_read(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    id: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<NotificationRow>> {
    sqlx::query_as(
        "UPDATE notifications SET read_at = COALESCE(read_at, $3)
         WHERE id = $1 AND user_id = $2
         RETURNING id, payload, read_at, created_at",
    )
    .bind(id)
    .bind(user)
    .bind(now)
    .fetch_optional(conn)
    .await
}

pub(crate) async fn mark_all_read(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<u64> {
    Ok(
        sqlx::query("UPDATE notifications SET read_at = $2 WHERE user_id = $1 AND read_at IS NULL")
            .bind(user)
            .bind(now)
            .execute(conn)
            .await?
            .rows_affected(),
    )
}

pub(crate) async fn preferences(
    conn: impl PgExecutor<'_>,
    user: Uuid,
) -> sqlx::Result<Vec<(String, Vec<String>)>> {
    sqlx::query_as("SELECT type, channels FROM notification_prefs WHERE user_id = $1")
        .bind(user)
        .fetch_all(conn)
        .await
}

pub(crate) async fn preference(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    kind: &str,
) -> sqlx::Result<Option<Vec<String>>> {
    sqlx::query_scalar("SELECT channels FROM notification_prefs WHERE user_id = $1 AND type = $2")
        .bind(user)
        .bind(kind)
        .fetch_optional(conn)
        .await
}

pub(crate) async fn set_preference(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    kind: &str,
    channels: &[String],
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO notification_prefs (user_id, type, channels, updated_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, type) DO UPDATE SET channels = $3, updated_at = $4",
    )
    .bind(user)
    .bind(kind)
    .bind(channels)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}
