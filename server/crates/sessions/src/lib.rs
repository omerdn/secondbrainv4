//! `sessions`: Work Sessions: timers and segments (PRD W1–W5, ADR-0015, ARCHITECTURE §11).
//!
//! - [`timer`]: the server-authoritative timer as a pure state machine (T2.6.1).
//! - [`WorkSessions`]: open, command and close a personal session; `session.timer` events;
//!   history and statistics (T2.6.2).
//! - Shared sessions (W6, T3.6.1): [`shared`] keeps one timer for everyone and each
//!   participant's own item and log; friends join by invitation.
//! - [`stats`]: work and break time per local day and per item, as a pure function.
//! - Work Session never completes anything (W5, ADR-0015): segments are statistics only.

pub mod http;
mod model;
mod repo;
mod service;
pub mod shared;
pub mod stats;
pub mod timer;

pub use model::{
    HistoryQuery, InviteRequest, JoinRequest, OpenRequest, PomodoroConfig, Segment,
    SessionInvitation, SessionSummary, StatsQuery, TimerView, WorkSession,
};
pub use service::{INVITATION_EVENT, MAX_STATS_DAYS, TIMER_EVENT, WorkSessions, default_pomodoro};
