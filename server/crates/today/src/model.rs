//! Today screen DTOs (PRD H1–H6).

use chrono::{DateTime, NaiveDate, Utc};
use goals::ActiveGoal;
use items::Item;
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use notebooks::process::StatusLine;
use routines::RoutineToday;
use serde::{Deserialize, Serialize};
use sessions::TimerView;
use tasks::DueTask;
use ts_rs::TS;
use uuid::Uuid;

/// Days without a touch after which an active item is forgotten, unless the user set it (H4).
pub const DEFAULT_FORGOTTEN_DAYS: i32 = 7;

/// A forgotten threshold in days, 1 to 365.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForgottenDays(i32);

impl ForgottenDays {
    /// # Errors
    /// [`AppError::Validation`] outside 1 to 365.
    pub fn new(days: i32) -> Result<Self, AppError> {
        if (1..=365).contains(&days) {
            Ok(Self(days))
        } else {
            Err(ValidationErrors::from(FieldError::new(
                "forgotten_days",
                FieldErrorCode::OutOfRange,
                "must be between 1 and 365",
            ))
            .into())
        }
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

/// The user's Today settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct HomePreferences {
    pub forgotten_days: i32,
}

/// `PUT /today/preferences` body.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct UpdateHomePreferences {
    pub forgotten_days: i32,
}

/// `GET /today` query.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct TodayQuery {
    /// The caller's local day; the server's UTC day when absent. Within a day of it.
    pub today: Option<NaiveDate>,
}

/// An item to pick up again, with the latest line of its process page (H5, S3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ResumeItem {
    pub item: Item,
    pub line: Option<StatusLine>,
}

/// The open work session at a glance (H6); clients count the time from `server_time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SessionGlance {
    pub id: Uuid,
    pub timer: TimerView,
    #[ts(type = "string")]
    pub server_time: DateTime<Utc>,
}

/// What the Today screen shows (H1–H6).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Today {
    #[ts(type = "string")]
    pub today: NaiveDate,
    /// Routines scheduled today, each in its own time zone (H1).
    pub routines: Vec<RoutineToday>,
    /// Unfinished tasks due before today, oldest due first (H2).
    pub overdue: Vec<DueTask>,
    /// Unfinished tasks due today or in the next days (H2).
    pub upcoming: Vec<DueTask>,
    /// Goals running today, ending soonest first (H3).
    pub goals: Vec<ActiveGoal>,
    /// Active items untouched for `forgotten_days`, longest untouched first (H4).
    pub forgotten: Vec<Item>,
    pub forgotten_days: i32,
    /// Most recently touched items with their status lines (H5).
    pub resume: Vec<ResumeItem>,
    /// The open work session, if any (H6).
    pub session: Option<SessionGlance>,
}
