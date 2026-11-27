//! Routine storage (ARCHITECTURE §5.6). Only this crate touches these tables (ADR-0007).

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

use crate::model::PlanVersionView;

/// A `routines` row.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct RoutineRow {
    pub item_id: Uuid,
    pub timezone: String,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub reminder_times: Vec<NaiveTime>,
    /// `pool` or `individual` (R5).
    pub completion_mode: String,
    /// `every_occurrence` or `weekly`; `None` keeps fixed assignees (R6).
    pub rotation: Option<String>,
}

#[derive(FromRow)]
struct VersionRow {
    id: Uuid,
    rrule: String,
    daily_target: i32,
    valid_from: NaiveDate,
    valid_to: Option<NaiveDate>,
}

impl From<VersionRow> for PlanVersionView {
    fn from(row: VersionRow) -> Self {
        Self {
            id: row.id,
            rrule: row.rrule,
            daily_target: row.daily_target,
            valid_from: row.valid_from,
            valid_to: row.valid_to,
        }
    }
}

const ROUTINE_COLUMNS: &str =
    "item_id, timezone, start_date, end_date, reminder_times, completion_mode, rotation";

/// # Errors
/// Database errors.
pub async fn insert_routine(
    conn: &mut PgConnection,
    row: &RoutineRow,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO routines (item_id, timezone, start_date, end_date, reminder_times,
                               completion_mode, rotation, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $7, $8, $6, $6)",
    )
    .bind(row.item_id)
    .bind(&row.timezone)
    .bind(row.start_date)
    .bind(row.end_date)
    .bind(&row.reminder_times)
    .bind(now)
    .bind(&row.completion_mode)
    .bind(&row.rotation)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn get_routine(conn: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<Option<RoutineRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ROUTINE_COLUMNS} FROM routines WHERE item_id = $1"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// Locks the routine for the rest of the transaction, so plan changes run one at a time.
///
/// # Errors
/// Database errors.
pub async fn lock_routine(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<RoutineRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ROUTINE_COLUMNS} FROM routines WHERE item_id = $1 FOR UPDATE"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// # Errors
/// Database errors.
pub async fn update_routine(
    conn: &mut PgConnection,
    row: &RoutineRow,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE routines SET timezone = $2, end_date = $3, reminder_times = $4, updated_at = $5,
                             completion_mode = $6, rotation = $7
         WHERE item_id = $1",
    )
    .bind(row.item_id)
    .bind(&row.timezone)
    .bind(row.end_date)
    .bind(&row.reminder_times)
    .bind(now)
    .bind(&row.completion_mode)
    .bind(&row.rotation)
    .execute(conn)
    .await?;
    Ok(())
}

/// The plan history, oldest first.
///
/// # Errors
/// Database errors.
pub async fn list_versions(
    conn: impl PgExecutor<'_>,
    routine: Uuid,
) -> sqlx::Result<Vec<PlanVersionView>> {
    let rows = sqlx::query_as::<_, VersionRow>(
        "SELECT id, rrule, daily_target, valid_from, valid_to
         FROM routine_schedules WHERE routine_id = $1 ORDER BY valid_from",
    )
    .bind(routine)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(PlanVersionView::from).collect())
}

/// # Errors
/// Database errors.
pub async fn insert_version(
    conn: &mut PgConnection,
    routine: Uuid,
    (rrule, daily_target): (&str, i32),
    (valid_from, valid_to): (NaiveDate, Option<NaiveDate>),
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO routine_schedules (id, routine_id, rrule, daily_target, valid_from, valid_to)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(kernel::Id::<()>::new_at(now).as_uuid())
    .bind(routine)
    .bind(rrule)
    .bind(daily_target)
    .bind(valid_from)
    .bind(valid_to)
    .execute(conn)
    .await?;
    Ok(())
}

/// # Errors
/// Database errors.
pub async fn set_version_end(
    conn: &mut PgConnection,
    id: Uuid,
    valid_to: Option<NaiveDate>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE routine_schedules SET valid_to = $2 WHERE id = $1")
        .bind(id)
        .bind(valid_to)
        .execute(conn)
        .await?;
    Ok(())
}

/// Replaces the rules of a version that has not started yet or starts today.
///
/// # Errors
/// Database errors.
pub async fn replace_version(
    conn: &mut PgConnection,
    id: Uuid,
    rrule: &str,
    daily_target: i32,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE routine_schedules SET rrule = $2, daily_target = $3 WHERE id = $1")
        .bind(id)
        .bind(rrule)
        .bind(daily_target)
        .execute(conn)
        .await?;
    Ok(())
}

/// The routines among `ids`.
///
/// # Errors
/// Database errors.
pub async fn list_routines(
    conn: impl PgExecutor<'_>,
    ids: &[Uuid],
) -> sqlx::Result<Vec<RoutineRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ROUTINE_COLUMNS} FROM routines WHERE item_id = ANY($1)"
    )))
    .bind(ids)
    .fetch_all(conn)
    .await
}

/// Routines with at least one reminder time.
///
/// # Errors
/// Database errors.
pub async fn routines_with_reminders(conn: impl PgExecutor<'_>) -> sqlx::Result<Vec<RoutineRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ROUTINE_COLUMNS} FROM routines WHERE cardinality(reminder_times) > 0"
    )))
    .fetch_all(conn)
    .await
}

/// Records a sent reminder; false when it was already sent.
///
/// # Errors
/// Database errors.
pub async fn record_reminder(
    conn: &mut PgConnection,
    (routine, user): (Uuid, Uuid),
    day: NaiveDate,
    time: NaiveTime,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "INSERT INTO routine_reminders_sent_to (routine_id, local_date, reminder_time, user_id, sent_at)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
    )
    .bind(routine)
    .bind(day)
    .bind(time)
    .bind(user)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// The order people take turns in (R6), first turn first. It may name former members.
///
/// # Errors
/// Database errors.
pub async fn rotation_order(conn: impl PgExecutor<'_>, routine: Uuid) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT user_id FROM routine_rotation WHERE routine_id = $1 ORDER BY position",
    )
    .bind(routine)
    .fetch_all(conn)
    .await
}

/// Replaces the turn order.
///
/// # Errors
/// Database errors.
pub async fn set_rotation_order(
    conn: &mut PgConnection,
    routine: Uuid,
    order: &[Uuid],
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM routine_rotation WHERE routine_id = $1")
        .bind(routine)
        .execute(&mut *conn)
        .await?;
    let positions: Vec<i32> = (0..i32::try_from(order.len()).unwrap_or(i32::MAX)).collect();
    sqlx::query(
        "INSERT INTO routine_rotation (routine_id, user_id, position)
         SELECT $1, person, position FROM unnest($2::uuid[], $3::int[]) AS t(person, position)",
    )
    .bind(routine)
    .bind(order)
    .bind(&positions)
    .execute(conn)
    .await?;
    Ok(())
}

/// The swaps that were made under `turn_order`: the date and who has it. Swaps made under
/// another order no longer count.
///
/// # Errors
/// Database errors.
pub async fn rotation_overrides(
    conn: impl PgExecutor<'_>,
    routine: Uuid,
    turn_order: &[Uuid],
) -> sqlx::Result<Vec<(NaiveDate, Uuid)>> {
    sqlx::query_as(
        "SELECT occurrence_date, user_id FROM routine_rotation_overrides
         WHERE routine_id = $1 AND turn_order = $2 ORDER BY occurrence_date",
    )
    .bind(routine)
    .bind(turn_order)
    .fetch_all(conn)
    .await
}

/// Stores what a swap left on `dates`, made under `turn_order`: the entries of `overrides` on
/// those dates replace the stored ones, and a date without an entry goes back to the rotation.
///
/// # Errors
/// Database errors.
pub async fn set_rotation_overrides(
    conn: &mut PgConnection,
    routine: Uuid,
    dates: &[NaiveDate],
    overrides: &[(NaiveDate, Uuid)],
    turn_order: &[Uuid],
) -> sqlx::Result<()> {
    sqlx::query(
        "DELETE FROM routine_rotation_overrides WHERE routine_id = $1 AND occurrence_date = ANY($2)",
    )
    .bind(routine)
    .bind(dates)
    .execute(&mut *conn)
    .await?;
    let (days, people): (Vec<NaiveDate>, Vec<Uuid>) = overrides.iter().copied().unzip();
    sqlx::query(
        "INSERT INTO routine_rotation_overrides (routine_id, occurrence_date, user_id, turn_order)
         SELECT $1, day, person, $4 FROM unnest($2::date[], $3::uuid[]) AS t(day, person)",
    )
    .bind(routine)
    .bind(&days)
    .bind(&people)
    .bind(turn_order)
    .execute(conn)
    .await?;
    Ok(())
}

/// Drops every swap, when the routine goes back to fixed assignees.
///
/// # Errors
/// Database errors.
pub async fn clear_rotation_overrides(conn: &mut PgConnection, routine: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM routine_rotation_overrides WHERE routine_id = $1")
        .bind(routine)
        .execute(conn)
        .await?;
    Ok(())
}
