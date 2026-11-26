//! SQL for the work session tables; nothing else touches them (ADR-0007).

use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

use crate::model::{PomodoroConfig, Segment};
use crate::timer::{Mode, State, Timer};

type SegmentRow = (
    Uuid,
    String,
    Option<Uuid>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    i64,
);

#[derive(Debug, FromRow)]
pub(crate) struct SessionRow {
    pub id: Uuid,
    pub created_by: Uuid,
    pub status: String,
    pub timer_state: String,
    pub timer_mode: Option<String>,
    pub timer_item_id: Option<Uuid>,
    pub timer_started_at: Option<DateTime<Utc>>,
    pub timer_accumulated_ms: i64,
    pub timer_planned_ms: Option<i64>,
    pub pomodoro_config: Json<PomodoroConfig>,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl SessionRow {
    pub(crate) fn timer(&self) -> sqlx::Result<Timer> {
        let corrupt = |c: &str| sqlx::Error::Decode(format!("unknown session {c}").into());
        Ok(Timer {
            state: State::parse(&self.timer_state).ok_or_else(|| corrupt("timer_state"))?,
            mode: self
                .timer_mode
                .as_deref()
                .map(|m| Mode::parse(m).ok_or_else(|| corrupt("timer_mode")))
                .transpose()?,
            item_id: self.timer_item_id,
            started_at: self.timer_started_at,
            accumulated_ms: self.timer_accumulated_ms,
            planned_ms: self.timer_planned_ms,
        })
    }
}

const COLUMNS: &str = "id, created_by, status, timer_state, timer_mode, timer_item_id, \
                       timer_started_at, timer_accumulated_ms, timer_planned_ms, pomodoro_config, \
                       created_at, closed_at";

/// The caller's open personal session.
pub(crate) async fn open_of(
    conn: impl PgExecutor<'_>,
    user: Uuid,
) -> sqlx::Result<Option<SessionRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM work_sessions
         WHERE created_by = $1 AND status = 'open' AND NOT is_shared"
    )))
    .bind(user)
    .fetch_optional(conn)
    .await
}

/// A session the user takes part in; locked with `for_update`.
pub(crate) async fn get(
    conn: impl PgExecutor<'_>,
    id: Uuid,
    user: Uuid,
    for_update: bool,
) -> sqlx::Result<Option<SessionRow>> {
    let lock = if for_update { " FOR UPDATE" } else { "" };
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM work_sessions s
         WHERE id = $1 AND (created_by = $2 OR EXISTS (
             SELECT 1 FROM work_session_participants p
             WHERE p.session_id = s.id AND p.user_id = $2 AND p.left_at IS NULL)){lock}"
    )))
    .bind(id)
    .bind(user)
    .fetch_optional(conn)
    .await
}

/// Opens a personal session; an open one already there wins (W1, concurrent opens).
pub(crate) async fn insert(
    conn: &mut PgConnection,
    id: Uuid,
    user: Uuid,
    pomodoro: PomodoroConfig,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let done = sqlx::query(
        "INSERT INTO work_sessions (id, created_by, status, timer_state, pomodoro_config, created_at)
         VALUES ($1, $2, 'open', 'stopped', $3, $4)
         ON CONFLICT (created_by) WHERE status = 'open' AND NOT is_shared DO NOTHING",
    )
    .bind(id)
    .bind(user)
    .bind(Json(pomodoro))
    .bind(now)
    .execute(&mut *conn)
    .await?;
    if done.rows_affected() == 1 {
        sqlx::query(
            "INSERT INTO work_session_participants (session_id, user_id, joined_at) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(user)
        .bind(now)
        .execute(conn)
        .await?;
    }
    Ok(done.rows_affected() == 1)
}

pub(crate) async fn save_timer(
    conn: &mut PgConnection,
    id: Uuid,
    timer: &Timer,
    closed_at: Option<DateTime<Utc>>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE work_sessions SET timer_state = $2, timer_mode = $3, timer_item_id = $4,
             timer_started_at = $5, timer_accumulated_ms = $6, timer_planned_ms = $7,
             status = CASE WHEN $8::timestamptz IS NULL THEN status ELSE 'closed' END,
             closed_at = COALESCE($8, closed_at)
         WHERE id = $1",
    )
    .bind(id)
    .bind(timer.state.as_str())
    .bind(timer.mode.map(Mode::as_str))
    .bind(timer.item_id)
    .bind(timer.started_at)
    .bind(timer.accumulated_ms)
    .bind(timer.planned_ms)
    .bind(closed_at)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn open_segment(
    conn: &mut PgConnection,
    (session, user): (Uuid, Uuid),
    mode: Mode,
    item: Option<Uuid>,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO work_session_segments (id, session_id, user_id, mode, item_id, started_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(kernel::Id::<()>::new_at(now).as_uuid())
    .bind(session)
    .bind(user)
    .bind(mode.as_str())
    .bind(item)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn close_segment(
    conn: &mut PgConnection,
    (session, user): (Uuid, Uuid),
    active_ms: i64,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE work_session_segments SET ended_at = $3, active_ms = $4
         WHERE session_id = $1 AND user_id = $2 AND ended_at IS NULL",
    )
    .bind(session)
    .bind(user)
    .bind(now)
    .bind(active_ms)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn segments(
    conn: impl PgExecutor<'_>,
    session: Uuid,
    user: Uuid,
) -> sqlx::Result<Vec<Segment>> {
    let rows: Vec<SegmentRow> = sqlx::query_as(
        "SELECT id, mode, item_id, started_at, ended_at, active_ms FROM work_session_segments
             WHERE session_id = $1 AND user_id = $2 ORDER BY started_at, id",
    )
    .bind(session)
    .bind(user)
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|(id, mode, item_id, started_at, ended_at, active_ms)| {
            Ok(Segment {
                id,
                mode: Mode::parse(&mode)
                    .ok_or_else(|| sqlx::Error::Decode("unknown segment mode".into()))?,
                item_id,
                started_at,
                ended_at,
                active_ms,
            })
        })
        .collect()
}

/// The user's segments that started in `[from, to)`, with the live active time of the one
/// still going (from its session's timer).
pub(crate) async fn counted(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    (from, to): (DateTime<Utc>, DateTime<Utc>),
    now: DateTime<Utc>,
) -> sqlx::Result<Vec<crate::stats::Counted>> {
    let rows: Vec<(String, Option<Uuid>, DateTime<Utc>, i64)> = sqlx::query_as(
        "SELECT g.mode, g.item_id, g.started_at,
                CASE WHEN g.ended_at IS NOT NULL THEN g.active_ms
                     ELSE s.timer_accumulated_ms + COALESCE(
                         (EXTRACT(EPOCH FROM ($4 - s.timer_started_at)) * 1000)::bigint, 0)
                END
         FROM work_session_segments g JOIN work_sessions s ON s.id = g.session_id
         WHERE g.user_id = $1 AND g.started_at >= $2 AND g.started_at < $3
         ORDER BY g.started_at, g.id",
    )
    .bind(user)
    .bind(from)
    .bind(to)
    .bind(now)
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|(mode, item_id, started_at, active_ms)| {
            Ok(crate::stats::Counted {
                mode: Mode::parse(&mode)
                    .ok_or_else(|| sqlx::Error::Decode("unknown segment mode".into()))?,
                item_id,
                started_at,
                active_ms: active_ms.max(0),
            })
        })
        .collect()
}

/// A past or current session in the history list.
#[derive(Debug, FromRow)]
pub(crate) struct HistoryRow {
    pub id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub work_ms: i64,
    pub break_ms: i64,
    pub segments: i64,
}

/// The user's sessions newest first, created before `before` if given, at most `limit`.
pub(crate) async fn history(
    conn: impl PgExecutor<'_>,
    user: Uuid,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> sqlx::Result<Vec<HistoryRow>> {
    sqlx::query_as(
        "SELECT s.id, s.status, s.created_at, s.closed_at,
                COALESCE(SUM(g.active_ms) FILTER (WHERE g.mode <> 'break'), 0)::bigint AS work_ms,
                COALESCE(SUM(g.active_ms) FILTER (WHERE g.mode = 'break'), 0)::bigint AS break_ms,
                COUNT(g.id) AS segments
         FROM work_sessions s
         JOIN work_session_participants p ON p.session_id = s.id AND p.user_id = $1
         LEFT JOIN work_session_segments g ON g.session_id = s.id AND g.user_id = $1
         WHERE ($2::timestamptz IS NULL OR s.created_at < $2)
         GROUP BY s.id ORDER BY s.created_at DESC, s.id DESC LIMIT $3",
    )
    .bind(user)
    .bind(before)
    .bind(limit)
    .fetch_all(conn)
    .await
}
