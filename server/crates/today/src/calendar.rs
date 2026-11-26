//! The calendar (PRD C1, T2.7.3): task dates, routine occurrences and goal ranges in a range of
//! calendar days, from the modules that own them (ADR-0007). Only what the caller can view.

use chrono::NaiveDate;
use goals::GoalSpan;
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use routines::RoutineOccurrences;
use serde::{Deserialize, Serialize};
use tasks::DatedTask;
use ts_rs::TS;
use uuid::Uuid;

use crate::service::TodayService;

/// Longest range one request covers: a month view with the weeks around it.
pub const MAX_CALENDAR_DAYS: i64 = 62;
/// Most entries of one kind in a range.
pub const CALENDAR_LIMIT: usize = 500;

/// `GET /calendar` query: calendar days, both ends included.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct CalendarQuery {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

/// What a range of calendar days holds (C1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Calendar {
    #[ts(type = "string")]
    pub from: NaiveDate,
    #[ts(type = "string")]
    pub to: NaiveDate,
    /// Tasks whose start or due date, or the days between, touch the range.
    pub tasks: Vec<DatedTask>,
    /// Routines with their occurrences in the range, as dates of their own time zone.
    pub routines: Vec<RoutineOccurrences>,
    /// Goals whose range overlaps it.
    pub goals: Vec<GoalSpan>,
}

/// # Errors
/// [`AppError::Validation`] for `to` before `from` or a range over [`MAX_CALENDAR_DAYS`].
pub fn check_range(from: NaiveDate, to: NaiveDate) -> Result<(), AppError> {
    let days = (to - from).num_days();
    if (0..MAX_CALENDAR_DAYS).contains(&days) {
        Ok(())
    } else {
        Err(ValidationErrors::from(FieldError::new(
            "to",
            FieldErrorCode::OutOfRange,
            format!("must be on or after `from`, at most {MAX_CALENDAR_DAYS} days"),
        ))
        .into())
    }
}

impl TodayService {
    /// The calendar of `from..=to` for `actor` (C1).
    ///
    /// # Errors
    /// [`AppError::Validation`] for a bad range, or [`AppError::Internal`].
    pub async fn calendar(
        &self,
        actor: Uuid,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Calendar, AppError> {
        check_range(from, to)?;
        let limit = i64::try_from(CALENDAR_LIMIT).unwrap_or(i64::MAX);
        Ok(Calendar {
            from,
            to,
            tasks: self.tasks.dated(actor, from, to, limit).await?,
            routines: self
                .routines
                .occurrences(actor, from, to, CALENDAR_LIMIT)
                .await?,
            goals: self
                .goals
                .overlapping(actor, from, to, CALENDAR_LIMIT)
                .await?,
        })
    }
}
