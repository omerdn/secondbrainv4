//! `routines`: Routines, completion modes, rotation and streaks.
//!
//! See ARCHITECTURE §5.6 and PRD R1–R13. A routine is an `items` row plus a detail row here;
//! `items` is only reached through [`items::ItemsService`] (ADR-0007). Occurrences are computed by
//! `recurrence` and never stored (ADR-0008).

mod completions;
pub mod http;
mod model;
mod progress;
pub mod repo;
mod service;
mod streak;

pub use completions::{
    BACKDATE_DAYS, COUNT_MAX, CompleteRoutineRequest, DAYS_MAX, RoutineDay, RoutineDaysQuery,
    UndoCompletionQuery, may_mark_day, routine_day,
};
pub use model::{
    ChangePlanRequest, CreateRoutineRequest, DAILY_TARGET_MAX, PlanVersionView, REMINDER_LIMIT,
    RoutineView, UpdateRoutineRequest, parse_reminders, parse_rules, parse_timezone, target_on,
};
pub use progress::{RoutineProgress, overlap, routine_progress};
pub use service::RoutinesService;
pub use streak::{RoutineStreak, StreakDay, routine_streak, streak};
