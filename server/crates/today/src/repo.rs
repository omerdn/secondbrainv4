//! SQL for `home_preferences`; nothing else in the crate touches it (ADR-0007).

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

pub(crate) async fn forgotten_days(
    conn: impl PgExecutor<'_>,
    user: Uuid,
) -> sqlx::Result<Option<i32>> {
    sqlx::query_scalar("SELECT forgotten_days FROM home_preferences WHERE user_id = $1")
        .bind(user)
        .fetch_optional(conn)
        .await
}

pub(crate) async fn set_forgotten_days(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    days: i32,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO home_preferences (user_id, forgotten_days, updated_at) VALUES ($1, $2, $3)
         ON CONFLICT (user_id) DO UPDATE SET forgotten_days = $2, updated_at = $3",
    )
    .bind(user)
    .bind(days)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}
