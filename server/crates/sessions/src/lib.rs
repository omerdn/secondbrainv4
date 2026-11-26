//! `sessions`: Work Sessions: timers and segments (PRD W1–W5, ADR-0015, ARCHITECTURE §11).
//!
//! - [`timer`]: the server-authoritative timer as a pure state machine (T2.6.1).
//! - [`WorkSessions`]: open, command and close a personal session; `session.timer` events.
//! - Work Session never completes anything (W5, ADR-0015): segments are statistics only.

pub mod http;
mod model;
mod repo;
mod service;
pub mod timer;

pub use model::{OpenRequest, PomodoroConfig, Segment, TimerView, WorkSession};
pub use service::{TIMER_EVENT, WorkSessions, default_pomodoro};
