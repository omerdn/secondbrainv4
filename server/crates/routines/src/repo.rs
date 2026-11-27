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

const ROUTINE_COLUMNS: &str = "item_id, timezone, start_date, end_date, reminder_times";

/// # Errors
/// Database errors.
pub async fn insert_routine(
    conn: &mut PgConnection,
    row: &RoutineRow,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO routines (item_id, timezone, start_date, end_date, reminder_times,
                               created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $6)",
    )
    .bind(row.item_id)
    .bind(&row.timezone)
    .bind(row.start_date)
    .bind(row.end_date)
    .bind(&row.reminder_times)
    .bind(now)
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
        "UPDATE routines SET timezone = $2, end_date = $3, reminder_times = $4, updated_at = $5
         WHERE item_id = $1",
    )
    .bind(row.item_id)
    .bind(&row.timezone)
    .bind(row.end_date)
    .bind(&row.reminder_times)
    .bind(now)
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
    routine: Uuid,
    day: NaiveDate,
    time: NaiveTime,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "INSERT INTO routine_reminders_sent (routine_id, local_date, reminder_time, sent_at)
         VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
    )
    .bind(routine)
    .bind(day)
    .bind(time)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}
