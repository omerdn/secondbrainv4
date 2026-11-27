//! SQL for `friend_requests`, `friendships` and `blocks`; nothing else touches them (ADR-0007).

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

pub(crate) async fn sent_since(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    since: DateTime<Utc>,
) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM friend_requests WHERE from_user = $1 AND created_at > $2",
    )
    .bind(user)
    .bind(since)
    .fetch_one(conn)
    .await
}

/// Whether the sender has an open request to the address, or one declined since `since`.
pub(crate) async fn recently_asked(
    conn: impl PgExecutor<'_>,
    from: Uuid,
    email: &str,
    since: DateTime<Utc>,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM friend_requests
                        WHERE from_user = $1 AND to_email = $2
                          AND (status = 'pending' OR (status = 'declined' AND responded_at >= $3)))",
    )
    .bind(from)
    .bind(email)
    .bind(since)
    .fetch_one(conn)
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_request(
    conn: impl PgExecutor<'_>,
    id: Uuid,
    from: Uuid,
    email: &str,
    to: Option<Uuid>,
    silenced: bool,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "INSERT INTO friend_requests (id, from_user, to_email, to_user, silenced, status, created_at)
         VALUES ($1, $2, $3, $4, $5, 'pending', $6)
         ON CONFLICT (from_user, to_email) WHERE status = 'pending' DO NOTHING",
    )
    .bind(id)
    .bind(from)
    .bind(email)
    .bind(to)
    .bind(silenced)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn outgoing(
    conn: impl PgExecutor<'_>,
    user: Uuid,
) -> sqlx::Result<Vec<(Uuid, String, DateTime<Utc>)>> {
    sqlx::query_as(
        "SELECT id, to_email, created_at FROM friend_requests
         WHERE from_user = $1 AND status = 'pending' ORDER BY created_at DESC, id DESC",
    )
    .bind(user)
    .fetch_all(conn)
    .await
}

pub(crate) async fn incoming(
    conn: impl PgExecutor<'_>,
    user: Uuid,
) -> sqlx::Result<Vec<(Uuid, Uuid, DateTime<Utc>)>> {
    sqlx::query_as(
        "SELECT id, from_user, created_at FROM friend_requests
         WHERE to_user = $1 AND status = 'pending' AND NOT silenced
         ORDER BY created_at DESC, id DESC",
    )
    .bind(user)
    .fetch_all(conn)
    .await
}

/// Locks a pending, delivered request addressed to `user`; its sender.
pub(crate) async fn lock_incoming(
    conn: &mut PgConnection,
    user: Uuid,
    id: Uuid,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT from_user FROM friend_requests
         WHERE id = $1 AND to_user = $2 AND status = 'pending' AND NOT silenced FOR UPDATE",
    )
    .bind(id)
    .bind(user)
    .fetch_optional(conn)
    .await
}

pub(crate) async fn respond(
    conn: impl PgExecutor<'_>,
    id: Uuid,
    status: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE friend_requests SET status = $2, responded_at = $3 WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(now)
        .execute(conn)
        .await?;
    Ok(())
}

/// Cancels the sender's own pending request; false when there is none.
pub(crate) async fn cancel_own(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    id: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE friend_requests SET status = 'cancelled', responded_at = $3
         WHERE id = $1 AND from_user = $2 AND status = 'pending'",
    )
    .bind(id)
    .bind(user)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Cancels every pending request between the two, either way.
pub(crate) async fn cancel_between(
    conn: impl PgExecutor<'_>,
    a: Uuid,
    b: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE friend_requests SET status = 'cancelled', responded_at = $3
         WHERE status = 'pending'
           AND ((from_user = $1 AND to_user = $2) OR (from_user = $2 AND to_user = $1))",
    )
    .bind(a)
    .bind(b)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn insert_friendship(
    conn: impl PgExecutor<'_>,
    (a, b): (Uuid, Uuid),
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO friendships (user_a, user_b, created_at) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(a)
    .bind(b)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn delete_friendship(
    conn: impl PgExecutor<'_>,
    (a, b): (Uuid, Uuid),
) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM friendships WHERE user_a = $1 AND user_b = $2")
        .bind(a)
        .bind(b)
        .execute(conn)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn are_friends(
    conn: impl PgExecutor<'_>,
    (a, b): (Uuid, Uuid),
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM friendships WHERE user_a = $1 AND user_b = $2)",
    )
    .bind(a)
    .bind(b)
    .fetch_one(conn)
    .await
}

pub(crate) async fn friends(
    conn: impl PgExecutor<'_>,
    user: Uuid,
) -> sqlx::Result<Vec<(Uuid, DateTime<Utc>)>> {
    sqlx::query_as(
        "SELECT CASE WHEN user_a = $1 THEN user_b ELSE user_a END, created_at
         FROM friendships WHERE user_a = $1 OR user_b = $1 ORDER BY created_at DESC",
    )
    .bind(user)
    .fetch_all(conn)
    .await
}

/// Records a nudge for the sender's `day`; `false` when they already nudged that person that day
/// (AK3).
pub(crate) async fn insert_nudge(
    conn: impl PgExecutor<'_>,
    (from, to): (Uuid, Uuid),
    day: chrono::NaiveDate,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "INSERT INTO nudges (from_user, to_user, local_date, created_at)
         VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
    )
    .bind(from)
    .bind(to)
    .bind(day)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Whether `blocker` blocked `blocked`.
pub(crate) async fn has_blocked(
    conn: impl PgExecutor<'_>,
    blocker: Uuid,
    blocked: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM blocks WHERE blocker = $1 AND blocked = $2)")
        .bind(blocker)
        .bind(blocked)
        .fetch_one(conn)
        .await
}

/// Whether either blocked the other.
pub(crate) async fn blocked_between(
    conn: impl PgExecutor<'_>,
    a: Uuid,
    b: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM blocks
                        WHERE (blocker = $1 AND blocked = $2) OR (blocker = $2 AND blocked = $1))",
    )
    .bind(a)
    .bind(b)
    .fetch_one(conn)
    .await
}

pub(crate) async fn insert_block(
    conn: impl PgExecutor<'_>,
    blocker: Uuid,
    blocked: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO blocks (blocker, blocked, created_at) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(blocker)
    .bind(blocked)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn delete_block(
    conn: impl PgExecutor<'_>,
    blocker: Uuid,
    blocked: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM blocks WHERE blocker = $1 AND blocked = $2")
        .bind(blocker)
        .bind(blocked)
        .execute(conn)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn blocks(
    conn: impl PgExecutor<'_>,
    user: Uuid,
) -> sqlx::Result<Vec<(Uuid, DateTime<Utc>)>> {
    sqlx::query_as(
        "SELECT blocked, created_at FROM blocks WHERE blocker = $1 ORDER BY created_at DESC",
    )
    .bind(user)
    .fetch_all(conn)
    .await
}
