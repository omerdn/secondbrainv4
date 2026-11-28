//! `today`: the Today screen (PRD H1–H6, T2.7.1) and the calendar (PRD C1, T2.7.3).
//!
//! [`TodayService`] gathers what the screen shows from the modules that own it, through their
//! service interfaces (ADR-0007): routines scheduled today (`routines`), overdue and upcoming
//! tasks (`tasks`), running goals (`goals`), forgotten and recently touched items (`items`),
//! their status lines (`notebooks`) and the open work session (`sessions`). Every list holds only
//! what the caller can view. The only table here is `home_preferences` (the forgotten threshold).

pub mod calendar;
mod dashboard;
pub mod http;
mod model;
mod repo;
mod service;

pub use calendar::{CALENDAR_LIMIT, Calendar, CalendarQuery, MAX_CALENDAR_DAYS};
pub use dashboard::{ADHERENCE_DAYS, TeamDashboard};
pub use model::{
    DEFAULT_FORGOTTEN_DAYS, ForgottenDays, HomePreferences, ResumeItem, SessionGlance, Today,
    TodayQuery, UpdateHomePreferences,
};
pub use service::{FORGOTTEN_KINDS, LIST_LIMIT, RESUME_KINDS, TodayService, UPCOMING_DAYS};
