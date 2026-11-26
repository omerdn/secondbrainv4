//! SQL for the `goals` and `goal_links` tables; nothing else in the crate touches them, and they
//! touch no other crate's tables (ADR-0007).

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRow)]
pub struct GoalRow {
    pub item_id: Uuid,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

/// # Errors
/// Database errors.
pub async fn insert_goal(
    conn: &mut PgConnection,
    row: GoalRow,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO goals (item_id, start_date, end_date, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $4)",
    )
    .bind(row.item_id)
    .bind(row.start_date)
    .bind(row.end_date)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn get_goal(conn: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<Option<GoalRow>> {
    sqlx::query_as("SELECT item_id, start_date, end_date FROM goals WHERE item_id = $1")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Locks the goal for the rest of the transaction, so date changes run one at a time.
///
/// # Errors
/// Database errors.
pub async fn lock_goal(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<GoalRow>> {
    sqlx::query_as("SELECT item_id, start_date, end_date FROM goals WHERE item_id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// # Errors
/// Database errors.
pub async fn update_goal(
    conn: &mut PgConnection,
    row: GoalRow,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE goals SET start_date = $2, end_date = $3, updated_at = $4 WHERE item_id = $1",
    )
    .bind(row.item_id)
    .bind(row.start_date)
    .bind(row.end_date)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// The stored weights of a goal's links; links without a row weigh 1.
///
/// # Errors
/// Database errors.
pub async fn weights(conn: impl PgExecutor<'_>, goal: Uuid) -> sqlx::Result<Vec<(Uuid, Decimal)>> {
    sqlx::query_as("SELECT item_id, weight FROM goal_links WHERE goal_id = $1")
        .bind(goal)
        .fetch_all(conn)
        .await
}

/// # Errors
/// Database errors.
pub async fn set_weight(
    conn: &mut PgConnection,
    (goal, item): (Uuid, Uuid),
    weight: Decimal,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO goal_links (goal_id, item_id, weight, updated_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (goal_id, item_id) DO UPDATE SET weight = EXCLUDED.weight,
                                                      updated_at = EXCLUDED.updated_at",
    )
    .bind(goal)
    .bind(item)
    .bind(weight)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn delete_weight(
    conn: &mut PgConnection,
    (goal, item): (Uuid, Uuid),
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM goal_links WHERE goal_id = $1 AND item_id = $2")
        .bind(goal)
        .bind(item)
        .execute(conn)
        .await?;
    Ok(())
}

/// The goals among `ids` whose range holds `today`, ending soonest first.
///
/// # Errors
/// Database errors.
pub async fn running_goals(
    conn: impl PgExecutor<'_>,
    ids: &[Uuid],
    today: NaiveDate,
) -> sqlx::Result<Vec<GoalRow>> {
    sqlx::query_as(
        "SELECT item_id, start_date, end_date FROM goals
         WHERE item_id = ANY($1) AND start_date <= $2 AND end_date >= $2
         ORDER BY end_date, item_id",
    )
    .bind(ids)
    .bind(today)
    .fetch_all(conn)
    .await
}

/// The goals among `ids` whose range overlaps `from..=to`, starting first.
///
/// # Errors
/// Database errors.
pub async fn overlapping_goals(
    conn: impl PgExecutor<'_>,
    ids: &[Uuid],
    from: NaiveDate,
    to: NaiveDate,
) -> sqlx::Result<Vec<GoalRow>> {
    sqlx::query_as(
        "SELECT item_id, start_date, end_date FROM goals
         WHERE item_id = ANY($1) AND start_date <= $3 AND end_date >= $2
         ORDER BY start_date, item_id",
    )
    .bind(ids)
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await
}
