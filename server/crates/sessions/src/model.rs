//! Work session DTOs (PRD W1–W4, ARCHITECTURE §11).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::timer::{Mode, State, Timer};

/// Pomodoro lengths of a session (W2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PomodoroConfig {
    #[ts(type = "number")]
    pub work_ms: i64,
    #[ts(type = "number")]
    pub short_break_ms: i64,
    #[ts(type = "number")]
    pub long_break_ms: i64,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self {
            work_ms: 25 * 60_000,
            short_break_ms: 5 * 60_000,
            long_break_ms: 15 * 60_000,
        }
    }
}

/// Longest pomodoro phase accepted.
pub(crate) const MAX_PHASE_MS: i64 = 4 * 60 * 60_000;

impl PomodoroConfig {
    /// Whether every length is between a minute and four hours.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        [self.work_ms, self.short_break_ms, self.long_break_ms]
            .iter()
            .all(|ms| (60_000..=MAX_PHASE_MS).contains(ms))
    }
}

/// `POST /sessions` body: the pomodoro lengths, defaults otherwise.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export, rename = "OpenSessionRequest")]
pub struct OpenRequest {
    #[serde(default)]
    #[ts(optional)]
    pub pomodoro: Option<PomodoroConfig>,
}

/// The timer as clients see it; they compute the remaining time from `server_time`
/// (ARCHITECTURE §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SessionTimer")]
pub struct TimerView {
    pub state: State,
    pub mode: Option<Mode>,
    pub item_id: Option<Uuid>,
    #[ts(type = "string | null")]
    pub started_at: Option<DateTime<Utc>>,
    #[ts(type = "number")]
    pub accumulated_ms: i64,
    #[ts(type = "number | null")]
    pub planned_ms: Option<i64>,
}

impl From<Timer> for TimerView {
    fn from(t: Timer) -> Self {
        Self {
            state: t.state,
            mode: t.mode,
            item_id: t.item_id,
            started_at: t.started_at,
            accumulated_ms: t.accumulated_ms,
            planned_ms: t.planned_ms,
        }
    }
}

/// One stretch of work on one item (W3, W4). Statistics only (W5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SessionSegment")]
pub struct Segment {
    pub id: Uuid,
    pub mode: Mode,
    pub item_id: Option<Uuid>,
    #[ts(type = "string")]
    pub started_at: DateTime<Utc>,
    #[ts(type = "string | null")]
    pub ended_at: Option<DateTime<Utc>>,
    /// Active time; for the open segment, as of its last pause or stop.
    #[ts(type = "number")]
    pub active_ms: i64,
}

/// A work session with its timer and the caller's segments, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct WorkSession {
    pub id: Uuid,
    pub open: bool,
    pub timer: TimerView,
    pub pomodoro: PomodoroConfig,
    pub segments: Vec<Segment>,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "string | null")]
    pub closed_at: Option<DateTime<Utc>>,
    /// The server's clock when this was read, for the client's offset.
    #[ts(type = "string")]
    pub server_time: DateTime<Utc>,
}

/// A session in the history (W5): its totals as of its last pause or stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SessionSummary {
    pub id: Uuid,
    pub open: bool,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "string | null")]
    pub closed_at: Option<DateTime<Utc>>,
    #[ts(type = "number")]
    pub work_ms: i64,
    #[ts(type = "number")]
    pub break_ms: i64,
    #[ts(type = "number")]
    pub segments: i64,
}

/// `GET /sessions/stats` query: local days, both ends included; the last 7 days by default.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct StatsQuery {
    pub from: Option<chrono::NaiveDate>,
    pub to: Option<chrono::NaiveDate>,
}

/// `GET /sessions` query.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct HistoryQuery {
    /// Only sessions opened before this time (the next page).
    pub before: Option<DateTime<Utc>>,
}
