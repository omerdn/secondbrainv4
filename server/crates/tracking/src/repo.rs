//! Storage of trackers and entries (T1.3.1). Takes any executor, so callers can run it inside the
//! transaction of their change. Authorization belongs to the service on top (T1.3.4).

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use crate::model::{
    Aggregation, Direction, NewEntry, NewTracker, OwnerKind, Period, Tracker, TrackerEntry,
};

const TRACKER_COLUMNS: &str = "item_id, unit, target_value, period, range_start, range_end, \
                               aggregation, direction, owner_kind, created_at, updated_at";
const ENTRY_COLUMNS: &str = "id, tracker_item_id, user_id, value, occurred_at, local_date, \
                             note_block_id, note, created_at, deleted_at";

#[derive(FromRow)]
struct TrackerRow {
    item_id: Uuid,
    unit: String,
    target_value: Decimal,
    period: String,
    range_start: Option<NaiveDate>,
    range_end: Option<NaiveDate>,
    aggregation: String,
    direction: String,
    owner_kind: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// A stored value this binary cannot read; excluded by the table's CHECK constraints.
fn corrupt(column: &str) -> sqlx::Error {
    sqlx::Error::Decode(format!("unknown tracker {column}").into())
}

impl TryFrom<TrackerRow> for Tracker {
    type Error = sqlx::Error;

    fn try_from(row: TrackerRow) -> sqlx::Result<Self> {
        Ok(Self {
            item_id: row.item_id,
            unit: row.unit,
            target_value: row.target_value,
            period: Period::parse(&row.period, row.range_start, row.range_end)
                .ok_or_else(|| corrupt("period"))?,
            aggregation: Aggregation::parse(&row.aggregation)
                .ok_or_else(|| corrupt("aggregation"))?,
            direction: Direction::parse(&row.direction).ok_or_else(|| corrupt("direction"))?,
            owner_kind: OwnerKind::parse(&row.owner_kind).ok_or_else(|| corrupt("owner_kind"))?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(FromRow)]
struct EntryRow {
    id: Uuid,
    tracker_item_id: Uuid,
    user_id: Uuid,
    value: Decimal,
    occurred_at: DateTime<Utc>,
    local_date: NaiveDate,
    note_block_id: Option<Uuid>,
    note: Option<String>,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl From<EntryRow> for TrackerEntry {
    fn from(row: EntryRow) -> Self {
        Self {
            id: row.id,
            tracker_item_id: row.tracker_item_id,
            user_id: row.user_id,
            value: row.value,
            occurred_at: row.occurred_at,
            local_date: row.local_date,
            note_block_id: row.note_block_id,
            note: row.note,
            created_at: row.created_at,
            deleted_at: row.deleted_at,
        }
    }
}

/// Stores a tracker for an existing `tracker` item.
///
/// # Errors
/// Database errors, including a constraint violation for an invalid definition.
pub async fn insert_tracker(
    conn: impl PgExecutor<'_>,
    new: &NewTracker,
    now: DateTime<Utc>,
) -> sqlx::Result<Tracker> {
    sqlx::query_as::<_, TrackerRow>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO trackers (item_id, unit, target_value, period, range_start, range_end,
                               aggregation, direction, owner_kind, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)
         RETURNING {TRACKER_COLUMNS}"
    )))
    .bind(new.item_id)
    .bind(&new.unit)
    .bind(new.target_value)
    .bind(new.period.as_str())
    .bind(new.period.range_start())
    .bind(new.period.range_end())
    .bind(new.aggregation.as_str())
    .bind(new.direction.as_str())
    .bind(new.owner_kind.as_str())
    .bind(now)
    .fetch_one(conn)
    .await?
    .try_into()
}

/// The tracker of an item, if it has one.
///
/// # Errors
/// Database errors.
pub async fn get_tracker(
    conn: impl PgExecutor<'_>,
    item_id: Uuid,
) -> sqlx::Result<Option<Tracker>> {
    sqlx::query_as::<_, TrackerRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {TRACKER_COLUMNS} FROM trackers WHERE item_id = $1"
    )))
    .bind(item_id)
    .fetch_optional(conn)
    .await?
    .map(Tracker::try_from)
    .transpose()
}

/// Changes to a tracker; absent fields stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackerChanges {
    pub unit: Option<String>,
    pub target_value: Option<Decimal>,
    pub period: Option<Period>,
    pub aggregation: Option<Aggregation>,
    pub direction: Option<Direction>,
}

/// Applies `changes`; `None` if the item has no tracker.
///
/// # Errors
/// Database errors.
pub async fn update_tracker(
    conn: impl PgExecutor<'_>,
    item_id: Uuid,
    changes: &TrackerChanges,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<Tracker>> {
    sqlx::query_as::<_, TrackerRow>(sqlx::AssertSqlSafe(format!(
        "UPDATE trackers SET unit = COALESCE($2, unit),
                             target_value = COALESCE($3, target_value),
                             period = COALESCE($4, period),
                             range_start = CASE WHEN $5 THEN $6::date ELSE range_start END,
                             range_end = CASE WHEN $5 THEN $7::date ELSE range_end END,
                             aggregation = COALESCE($8, aggregation),
                             direction = COALESCE($9, direction),
                             updated_at = $10
         WHERE item_id = $1
         RETURNING {TRACKER_COLUMNS}"
    )))
    .bind(item_id)
    .bind(&changes.unit)
    .bind(changes.target_value)
    .bind(changes.period.map(Period::as_str))
    .bind(changes.period.is_some())
    .bind(changes.period.and_then(Period::range_start))
    .bind(changes.period.and_then(Period::range_end))
    .bind(changes.aggregation.map(Aggregation::as_str))
    .bind(changes.direction.map(Direction::as_str))
    .bind(now)
    .fetch_optional(conn)
    .await?
    .map(Tracker::try_from)
    .transpose()
}

/// Stores an entry.
///
/// # Errors
/// Database errors, including a foreign key violation for an unknown tracker.
pub async fn insert_entry(
    conn: impl PgExecutor<'_>,
    new: &NewEntry,
    now: DateTime<Utc>,
) -> sqlx::Result<TrackerEntry> {
    let row = sqlx::query_as::<_, EntryRow>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO tracker_entries (id, tracker_item_id, user_id, value, occurred_at, local_date,
                                      note_block_id, note, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING {ENTRY_COLUMNS}"
    )))
    .bind(kernel::Id::<()>::new_at(now).as_uuid())
    .bind(new.tracker_item_id)
    .bind(new.user_id)
    .bind(new.value)
    .bind(new.occurred_at)
    .bind(new.local_date)
    .bind(new.note_block_id)
    .bind(&new.note)
    .bind(now)
    .fetch_one(conn)
    .await?;
    Ok(row.into())
}

/// A live entry of a tracker.
///
/// # Errors
/// Database errors.
pub async fn get_entry(
    conn: impl PgExecutor<'_>,
    tracker_item_id: Uuid,
    entry_id: Uuid,
) -> sqlx::Result<Option<TrackerEntry>> {
    let row = sqlx::query_as::<_, EntryRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {ENTRY_COLUMNS} FROM tracker_entries
         WHERE id = $1 AND tracker_item_id = $2 AND deleted_at IS NULL"
    )))
    .bind(entry_id)
    .bind(tracker_item_id)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(TrackerEntry::from))
}

/// One page of a tracker's live entries, newest first, within `days` (local, both included) and
/// after the `(occurred_at, id)` of the previous page.
///
/// # Errors
/// Database errors.
pub async fn page_entries(
    conn: impl PgExecutor<'_>,
    tracker_item_id: Uuid,
    (from, to): (Option<NaiveDate>, Option<NaiveDate>),
    after: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> sqlx::Result<Vec<TrackerEntry>> {
    let rows = sqlx::query_as::<_, EntryRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {ENTRY_COLUMNS} FROM tracker_entries
         WHERE tracker_item_id = $1 AND deleted_at IS NULL
           AND ($2::date IS NULL OR local_date >= $2)
           AND ($3::date IS NULL OR local_date <= $3)
           AND ($4::timestamptz IS NULL OR (occurred_at, id) < ($4, $5))
         ORDER BY occurred_at DESC, id DESC
         LIMIT $6"
    )))
    .bind(tracker_item_id)
    .bind(from)
    .bind(to)
    .bind(after.map(|(at, _)| at))
    .bind(after.map(|(_, id)| id))
    .bind(limit)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(TrackerEntry::from).collect())
}

/// A tracker's live entries, oldest first, optionally within `from..=to` local days.
///
/// # Errors
/// Database errors.
pub async fn list_entries(
    conn: impl PgExecutor<'_>,
    tracker_item_id: Uuid,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> sqlx::Result<Vec<TrackerEntry>> {
    let rows = sqlx::query_as::<_, EntryRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {ENTRY_COLUMNS} FROM tracker_entries
         WHERE tracker_item_id = $1 AND deleted_at IS NULL
           AND ($2::date IS NULL OR local_date >= $2)
           AND ($3::date IS NULL OR local_date <= $3)
         ORDER BY local_date, occurred_at, id"
    )))
    .bind(tracker_item_id)
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(TrackerEntry::from).collect())
}

/// Marks an entry as deleted. Returns `false` if it is absent or already deleted, so repeating it
/// is harmless and keeps the first deletion time.
///
/// # Errors
/// Database errors.
pub async fn soft_delete_entry(
    conn: impl PgExecutor<'_>,
    tracker_item_id: Uuid,
    entry_id: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let updated = sqlx::query(
        "UPDATE tracker_entries SET deleted_at = $3
         WHERE id = $1 AND tracker_item_id = $2 AND deleted_at IS NULL",
    )
    .bind(entry_id)
    .bind(tracker_item_id)
    .bind(now)
    .execute(conn)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

/// Links an entry's note to its line on the process page (T3); a linked entry stays as it is.
///
/// # Errors
/// Database errors.
pub async fn set_note_block(
    conn: impl PgExecutor<'_>,
    entry_id: Uuid,
    block: Uuid,
) -> sqlx::Result<bool> {
    let done = sqlx::query(
        "UPDATE tracker_entries SET note_block_id = $2 WHERE id = $1 AND note_block_id IS NULL",
    )
    .bind(entry_id)
    .bind(block)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Live entries whose note is not on a process page yet, oldest first, at most `limit`.
///
/// # Errors
/// Database errors.
pub async fn unlinked_notes(
    conn: impl PgExecutor<'_>,
    limit: i64,
) -> sqlx::Result<Vec<TrackerEntry>> {
    let rows = sqlx::query_as::<_, EntryRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {ENTRY_COLUMNS} FROM tracker_entries
         WHERE note IS NOT NULL AND note_block_id IS NULL AND deleted_at IS NULL
         ORDER BY created_at, id LIMIT $1"
    )))
    .bind(limit)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(TrackerEntry::from).collect())
}
