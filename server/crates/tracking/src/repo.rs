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
                             note_block_id, created_at, deleted_at";

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
                                      note_block_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING {ENTRY_COLUMNS}"
    )))
    .bind(kernel::Id::<()>::new_at(now).as_uuid())
    .bind(new.tracker_item_id)
    .bind(new.user_id)
    .bind(new.value)
    .bind(new.occurred_at)
    .bind(new.local_date)
    .bind(new.note_block_id)
    .bind(now)
    .fetch_one(conn)
    .await?;
    Ok(row.into())
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
