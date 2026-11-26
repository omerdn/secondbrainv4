//! The work session timer as a pure state machine (PRD W1–W3, ARCHITECTURE §11, T2.6.1).
//!
//! The server is the only authority on the timer. [`apply`] takes the current timer, a command
//! and the time (from the injected `Clock`) and returns the next timer plus what happens to the
//! segment log: a segment is one stretch of work in one mode on one item (W3). Pausing keeps the
//! segment; stopping ends it; switching the item (only while paused or stopped) ends it and, if
//! paused, starts the next one. Repeating a command whose effect already holds changes nothing,
//! so retries are safe.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// What a segment measures (W2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "SessionMode")]
pub enum Mode {
    /// Counts down from the planned time.
    Pomodoro,
    /// Counts up.
    Stopwatch,
    /// A break, counting down if a time is planned.
    Break,
}

impl Mode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pomodoro => "pomodoro",
            Self::Stopwatch => "stopwatch",
            Self::Break => "break",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pomodoro" => Some(Self::Pomodoro),
            "stopwatch" => Some(Self::Stopwatch),
            "break" => Some(Self::Break),
            _ => None,
        }
    }
}

/// Whether the timer runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "TimerState")]
pub enum State {
    Running,
    Paused,
    Stopped,
}

impl State {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

/// The timer of a session. While stopped, `mode` and `item_id` are what the next start uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timer {
    pub state: State,
    pub mode: Option<Mode>,
    pub item_id: Option<Uuid>,
    /// Set exactly while running.
    pub started_at: Option<DateTime<Utc>>,
    /// Active time of the current segment before `started_at`.
    pub accumulated_ms: i64,
    /// Planned length of a countdown; none for a stopwatch.
    pub planned_ms: Option<i64>,
}

impl Timer {
    /// A timer that has not started.
    pub const STOPPED: Self = Self {
        state: State::Stopped,
        mode: None,
        item_id: None,
        started_at: None,
        accumulated_ms: 0,
        planned_ms: None,
    };

    /// Active time of the current segment at `now`.
    #[must_use]
    pub fn active_ms(&self, now: DateTime<Utc>) -> i64 {
        let running = self
            .started_at
            .map_or(0, |started| (now - started).num_milliseconds().max(0));
        self.accumulated_ms.saturating_add(running)
    }
}

/// A command sent to a session's timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "command", rename_all = "snake_case")]
#[ts(export, rename = "TimerCommand")]
pub enum Command {
    /// Starts a new segment (from stopped).
    Start {
        mode: Mode,
        /// The item the time goes to (W3).
        #[serde(default)]
        #[ts(optional)]
        item_id: Option<Uuid>,
        /// Countdown length; pomodoro defaults to the session's work length.
        #[serde(default)]
        #[ts(optional, type = "number")]
        planned_ms: Option<i64>,
    },
    Pause,
    Resume,
    /// Ends the current segment.
    Stop,
    /// Another item (or none): a new segment while paused, the next start's item while stopped.
    SwitchItem {
        #[serde(default)]
        #[ts(optional)]
        item_id: Option<Uuid>,
    },
}

/// What happens to the segment log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogChange {
    /// Nothing.
    None,
    /// A segment starts.
    Open { mode: Mode, item_id: Option<Uuid> },
    /// The current segment ends with this much active time.
    Close { active_ms: i64 },
    /// The current segment ends and the next one starts (W3).
    CloseAndOpen {
        active_ms: i64,
        mode: Mode,
        item_id: Option<Uuid>,
    },
}

/// A command that does not fit the timer's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TimerError {
    #[error("the timer is already running another segment")]
    AlreadyRunning,
    #[error("the timer is not running")]
    NotStarted,
    #[error("pause the timer before switching the item")]
    SwitchWhileRunning,
    #[error("the planned time must be positive")]
    InvalidPlan,
}

/// The next timer and log change for `command` at `now` (see the module docs).
///
/// # Errors
/// [`TimerError`] for a command that does not fit; nothing changes then.
pub fn apply(
    timer: &Timer,
    command: Command,
    now: DateTime<Utc>,
    default_pomodoro_ms: i64,
) -> Result<(Timer, LogChange), TimerError> {
    let unchanged = Ok((*timer, LogChange::None));
    match (timer.state, command) {
        (
            State::Stopped,
            Command::Start {
                mode,
                item_id,
                planned_ms,
            },
        ) => {
            let planned_ms = match (mode, planned_ms) {
                (_, Some(ms)) if ms <= 0 => return Err(TimerError::InvalidPlan),
                (Mode::Stopwatch, _) => None,
                (Mode::Pomodoro, None) => Some(default_pomodoro_ms),
                (_, planned) => planned,
            };
            let next = Timer {
                state: State::Running,
                mode: Some(mode),
                item_id,
                started_at: Some(now),
                accumulated_ms: 0,
                planned_ms,
            };
            Ok((next, LogChange::Open { mode, item_id }))
        }
        // The same start again (a retry) changes nothing; another one is refused.
        (State::Running | State::Paused, Command::Start { mode, item_id, .. }) => {
            if timer.mode == Some(mode) && timer.item_id == item_id {
                unchanged
            } else {
                Err(TimerError::AlreadyRunning)
            }
        }
        (State::Running, Command::Pause) => Ok((
            Timer {
                state: State::Paused,
                started_at: None,
                accumulated_ms: timer.active_ms(now),
                ..*timer
            },
            LogChange::None,
        )),
        (State::Paused, Command::Resume) => Ok((
            Timer {
                state: State::Running,
                started_at: Some(now),
                ..*timer
            },
            LogChange::None,
        )),
        (State::Paused, Command::Pause) | (State::Running, Command::Resume) => unchanged,
        (State::Stopped, Command::Pause | Command::Resume) => Err(TimerError::NotStarted),
        (State::Running | State::Paused, Command::Stop) => Ok((
            Timer {
                state: State::Stopped,
                started_at: None,
                accumulated_ms: 0,
                ..*timer
            },
            LogChange::Close {
                active_ms: timer.active_ms(now),
            },
        )),
        (State::Stopped, Command::Stop) => unchanged,
        (State::Running, Command::SwitchItem { item_id }) => {
            if timer.item_id == item_id {
                unchanged
            } else {
                Err(TimerError::SwitchWhileRunning)
            }
        }
        (State::Paused, Command::SwitchItem { item_id }) => {
            if timer.item_id == item_id {
                return unchanged;
            }
            let mode = timer.mode.unwrap_or(Mode::Stopwatch);
            Ok((
                Timer {
                    item_id,
                    accumulated_ms: 0,
                    ..*timer
                },
                LogChange::CloseAndOpen {
                    active_ms: timer.accumulated_ms,
                    mode,
                    item_id,
                },
            ))
        }
        (State::Stopped, Command::SwitchItem { item_id }) => {
            Ok((Timer { item_id, ..*timer }, LogChange::None))
        }
    }
}

/// Stops the timer when the session closes (W1): the open segment ends.
#[must_use]
pub fn close(timer: &Timer, now: DateTime<Utc>) -> (Timer, LogChange) {
    match apply(timer, Command::Stop, now, 1) {
        Ok(next) => next,
        Err(_) => (*timer, LogChange::None),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::{Duration, TimeZone};
    use proptest::prelude::*;

    use super::*;

    const POMODORO: i64 = 25 * 60 * 1000;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 26, 9, 0, 0).unwrap()
    }

    fn at(minutes: i64) -> DateTime<Utc> {
        t0() + Duration::minutes(minutes)
    }

    fn start(mode: Mode, item: Option<Uuid>) -> Command {
        Command::Start {
            mode,
            item_id: item,
            planned_ms: None,
        }
    }

    #[test]
    fn a_segment_runs_pauses_resumes_and_stops_with_its_active_time() {
        let item = Some(Uuid::now_v7());
        let (t, log) = apply(
            &Timer::STOPPED,
            start(Mode::Pomodoro, item),
            at(0),
            POMODORO,
        )
        .unwrap();
        assert_eq!(
            log,
            LogChange::Open {
                mode: Mode::Pomodoro,
                item_id: item
            }
        );
        assert_eq!(t.planned_ms, Some(POMODORO));
        let (t, _) = apply(&t, Command::Pause, at(10), POMODORO).unwrap();
        assert_eq!(
            (t.state, t.accumulated_ms, t.started_at),
            (State::Paused, 600_000, None)
        );
        // Paused time does not count.
        let (t, _) = apply(&t, Command::Resume, at(30), POMODORO).unwrap();
        assert_eq!(t.active_ms(at(35)), 900_000);
        let (t, log) = apply(&t, Command::Stop, at(35), POMODORO).unwrap();
        assert_eq!(log, LogChange::Close { active_ms: 900_000 });
        assert_eq!(t.state, State::Stopped);
        assert_eq!(t.accumulated_ms, 0);
    }

    #[test]
    fn w3_switching_the_item_starts_a_new_segment_only_when_paused_or_stopped() {
        let (a, b) = (Some(Uuid::now_v7()), Some(Uuid::now_v7()));
        let (t, _) = apply(&Timer::STOPPED, start(Mode::Stopwatch, a), at(0), POMODORO).unwrap();
        assert_eq!(t.planned_ms, None);
        assert_eq!(
            apply(&t, Command::SwitchItem { item_id: b }, at(5), POMODORO),
            Err(TimerError::SwitchWhileRunning)
        );
        let (t, _) = apply(&t, Command::Pause, at(45), POMODORO).unwrap();
        let (t, log) = apply(&t, Command::SwitchItem { item_id: b }, at(46), POMODORO).unwrap();
        assert_eq!(
            log,
            LogChange::CloseAndOpen {
                active_ms: 45 * 60_000,
                mode: Mode::Stopwatch,
                item_id: b
            }
        );
        assert_eq!(
            (t.state, t.item_id, t.accumulated_ms),
            (State::Paused, b, 0)
        );
        // Stopped: the item is kept for the next start, no segment.
        let (t, _) = apply(&t, Command::Stop, at(47), POMODORO).unwrap();
        let (t, log) = apply(&t, Command::SwitchItem { item_id: a }, at(48), POMODORO).unwrap();
        assert_eq!((log, t.item_id), (LogChange::None, a));
    }

    #[test]
    fn repeated_commands_change_nothing() {
        let item = Some(Uuid::now_v7());
        let (running, _) =
            apply(&Timer::STOPPED, start(Mode::Break, item), at(0), POMODORO).unwrap();
        assert_eq!(running.planned_ms, None);
        for command in [
            start(Mode::Break, item),
            Command::Resume,
            Command::SwitchItem { item_id: item },
        ] {
            assert_eq!(
                apply(&running, command, at(1), POMODORO),
                Ok((running, LogChange::None))
            );
        }
        let (paused, _) = apply(&running, Command::Pause, at(2), POMODORO).unwrap();
        assert_eq!(
            apply(&paused, Command::Pause, at(3), POMODORO),
            Ok((paused, LogChange::None))
        );
        let (stopped, _) = apply(&paused, Command::Stop, at(4), POMODORO).unwrap();
        assert_eq!(
            apply(&stopped, Command::Stop, at(5), POMODORO),
            Ok((stopped, LogChange::None))
        );
        assert_eq!(
            apply(&stopped, Command::Pause, at(5), POMODORO),
            Err(TimerError::NotStarted)
        );
        assert_eq!(
            apply(&running, start(Mode::Stopwatch, None), at(1), POMODORO),
            Err(TimerError::AlreadyRunning)
        );
        let bad = Command::Start {
            mode: Mode::Pomodoro,
            item_id: None,
            planned_ms: Some(0),
        };
        assert_eq!(
            apply(&Timer::STOPPED, bad, at(0), POMODORO),
            Err(TimerError::InvalidPlan)
        );
    }

    #[test]
    fn w1_closing_stops_the_timer_and_ends_the_segment() {
        let (t, _) = apply(
            &Timer::STOPPED,
            start(Mode::Stopwatch, None),
            at(0),
            POMODORO,
        )
        .unwrap();
        assert_eq!(
            close(&t, at(20)).1,
            LogChange::Close {
                active_ms: 20 * 60_000
            }
        );
        assert_eq!(
            close(&Timer::STOPPED, at(20)),
            (Timer::STOPPED, LogChange::None)
        );
    }

    fn command() -> impl Strategy<Value = Command> {
        let item = prop_oneof![Just(None), Just(Some(Uuid::nil())), Just(Some(Uuid::max()))];
        let mode = prop_oneof![
            Just(Mode::Pomodoro),
            Just(Mode::Stopwatch),
            Just(Mode::Break)
        ];
        prop_oneof![
            (mode, item.clone(), prop::option::of(-5i64..100_000)).prop_map(
                |(mode, item_id, planned_ms)| Command::Start {
                    mode,
                    item_id,
                    planned_ms
                }
            ),
            Just(Command::Pause),
            Just(Command::Resume),
            Just(Command::Stop),
            item.prop_map(|item_id| Command::SwitchItem { item_id }),
        ]
    }

    proptest! {
        /// Whatever happens, the timer stays consistent and the log adds up: the time logged is
        /// exactly the running time, and at most one segment is open.
        #[test]
        fn the_log_adds_up(steps in prop::collection::vec((command(), 0i64..600), 1..60)) {
            let mut timer = Timer::STOPPED;
            let mut now = t0();
            let mut open = false;
            let mut logged = 0i64;
            let mut running_ms = 0i64;
            for (cmd, wait_s) in steps {
                let later = now + Duration::seconds(wait_s);
                if timer.state == State::Running {
                    running_ms += (later - now).num_milliseconds();
                }
                now = later;
                let before = timer;
                match apply(&timer, cmd, now, POMODORO) {
                    Ok((next, log)) => {
                        match log {
                            LogChange::None => {}
                            LogChange::Open { .. } => { prop_assert!(!open); open = true; }
                            LogChange::Close { active_ms } => { prop_assert!(open); open = false; logged += active_ms; }
                            LogChange::CloseAndOpen { active_ms, .. } => { prop_assert!(open); logged += active_ms; }
                        }
                        timer = next;
                    }
                    Err(_) => prop_assert_eq!(timer, before),
                }
                prop_assert_eq!(timer.state == State::Running, timer.started_at.is_some());
                prop_assert_eq!(open, timer.state != State::Stopped);
                prop_assert!(timer.accumulated_ms >= 0);
                prop_assert!(timer.planned_ms.is_none_or(|ms| ms > 0));
                // Logged plus the current segment is all the running time.
                prop_assert_eq!(logged + timer.active_ms(now), running_ms);
            }
        }
    }
}
