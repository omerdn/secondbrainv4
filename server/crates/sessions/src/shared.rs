//! Shared sessions as pure functions (PRD W6, ADR-0015, T3.6.1): one timer for everyone, a log
//! of one's own.
//!
//! The session's timer is shared: any participant starts, pauses, resumes or stops it, and the
//! timer itself never carries an item. Each participant has their own item and their own
//! segments. A segment remembers the timer's active time when it opened (`base_ms`), so someone
//! who joins a running timer counts from then on, and closing it counts `active - base_ms`.
//!
//! [`apply`] turns a command into the next timer and the segment changes for every participant;
//! [`join`] and [`leave`] do the same for someone arriving or going.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::timer::{self, Command, LogChange, Mode, State, Timer, TimerError};

/// A participant as the shared timer sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Participant {
    pub user: Uuid,
    /// What their time goes to (W3); each picks their own (W6).
    pub item: Option<Uuid>,
    /// The timer's active time when their open segment started; `None` without one.
    pub base_ms: Option<i64>,
}

/// A change to one participant's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentOp {
    Open {
        user: Uuid,
        mode: Mode,
        item: Option<Uuid>,
        base_ms: i64,
    },
    Close {
        user: Uuid,
        active_ms: i64,
    },
}

/// The outcome of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub timer: Timer,
    pub ops: Vec<SegmentOp>,
    /// The actor's new item, when it changed.
    pub item: Option<Option<Uuid>>,
}

fn close_all(participants: &[Participant], active: i64) -> Vec<SegmentOp> {
    participants
        .iter()
        .filter_map(|p| {
            p.base_ms.map(|base| SegmentOp::Close {
                user: p.user,
                active_ms: (active - base).max(0),
            })
        })
        .collect()
}

/// The next shared timer, the log changes and the actor's item for `command` by `actor`.
///
/// # Errors
/// As [`timer::apply`]: nothing changes then.
pub fn apply(
    current: &Timer,
    command: Command,
    actor: Uuid,
    participants: &[Participant],
    now: DateTime<Utc>,
    default_pomodoro_ms: i64,
) -> Result<Outcome, TimerError> {
    let mine = participants.iter().find(|p| p.user == actor);
    let unchanged = Outcome {
        timer: *current,
        ops: Vec::new(),
        item: None,
    };
    match command {
        Command::Start {
            mode,
            item_id,
            planned_ms,
        } => {
            // The shared timer never holds an item; each participant keeps their own.
            let shared = Command::Start {
                mode,
                item_id: None,
                planned_ms,
            };
            let (next, log) = timer::apply(current, shared, now, default_pomodoro_ms)?;
            let LogChange::Open { .. } = log else {
                return Ok(unchanged);
            };
            let ops = participants
                .iter()
                .map(|p| SegmentOp::Open {
                    user: p.user,
                    mode,
                    item: if p.user == actor { item_id } else { p.item },
                    base_ms: 0,
                })
                .collect();
            let item = (mine.map(|p| p.item) != Some(item_id)).then_some(item_id);
            Ok(Outcome {
                timer: next,
                ops,
                item,
            })
        }
        Command::Pause | Command::Resume => {
            let (next, _) = timer::apply(current, command, now, default_pomodoro_ms)?;
            Ok(Outcome {
                timer: next,
                ops: Vec::new(),
                item: None,
            })
        }
        Command::Stop => {
            let (next, log) = timer::apply(current, command, now, default_pomodoro_ms)?;
            let ops = match log {
                LogChange::Close { active_ms } => close_all(participants, active_ms),
                _ => Vec::new(),
            };
            Ok(Outcome {
                timer: next,
                ops,
                item: None,
            })
        }
        // Only the actor's own item and log change (W3, W6).
        Command::SwitchItem { item_id } => {
            let own = mine.and_then(|p| p.item);
            if own == item_id {
                return Ok(unchanged);
            }
            match current.state {
                State::Running => Err(TimerError::SwitchWhileRunning),
                State::Stopped => Ok(Outcome {
                    item: Some(item_id),
                    ..unchanged
                }),
                State::Paused => {
                    let mode = current.mode.unwrap_or(Mode::Stopwatch);
                    let at = current.accumulated_ms;
                    let mut ops = Vec::new();
                    if let Some(base) = mine.and_then(|p| p.base_ms) {
                        ops.push(SegmentOp::Close {
                            user: actor,
                            active_ms: (at - base).max(0),
                        });
                    }
                    ops.push(SegmentOp::Open {
                        user: actor,
                        mode,
                        item: item_id,
                        base_ms: at,
                    });
                    Ok(Outcome {
                        timer: *current,
                        ops,
                        item: Some(item_id),
                    })
                }
            }
        }
    }
}

/// Someone joins: while the timer runs or is paused, their segment starts now and counts from
/// the timer's current active time.
#[must_use]
pub fn join(current: &Timer, user: Uuid, item: Option<Uuid>, now: DateTime<Utc>) -> Vec<SegmentOp> {
    match (current.state, current.mode) {
        (State::Running | State::Paused, Some(mode)) => vec![SegmentOp::Open {
            user,
            mode,
            item,
            base_ms: current.active_ms(now),
        }],
        _ => Vec::new(),
    }
}

/// Someone leaves: their open segment ends with what they were there for.
#[must_use]
pub fn leave(current: &Timer, participant: &Participant, now: DateTime<Utc>) -> Vec<SegmentOp> {
    match (current.state, participant.base_ms) {
        (State::Running | State::Paused, Some(base)) => vec![SegmentOp::Close {
            user: participant.user,
            active_ms: (current.active_ms(now) - base).max(0),
        }],
        _ => Vec::new(),
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
        Utc.with_ymd_and_hms(2026, 11, 27, 9, 0, 0).unwrap()
    }

    fn at(minutes: i64) -> DateTime<Utc> {
        t0() + Duration::minutes(minutes)
    }

    fn person(n: u128, item: Option<u128>) -> Participant {
        Participant {
            user: Uuid::from_u128(n),
            item: item.map(Uuid::from_u128),
            base_ms: None,
        }
    }

    fn start(item: Option<u128>) -> Command {
        Command::Start {
            mode: Mode::Stopwatch,
            item_id: item.map(Uuid::from_u128),
            planned_ms: None,
        }
    }

    #[test]
    fn w6_one_start_opens_everyones_segment_with_their_own_item() {
        let (ali, ayse) = (person(1, Some(10)), person(2, Some(20)));
        let out = apply(
            &Timer::STOPPED,
            start(Some(11)),
            ali.user,
            &[ali, ayse],
            t0(),
            POMODORO,
        )
        .unwrap();
        assert_eq!(out.timer.state, State::Running);
        assert_eq!(out.timer.item_id, None, "the shared timer holds no item");
        assert_eq!(out.item, Some(Some(Uuid::from_u128(11))));
        assert_eq!(
            out.ops,
            [
                SegmentOp::Open {
                    user: ali.user,
                    mode: Mode::Stopwatch,
                    item: Some(Uuid::from_u128(11)),
                    base_ms: 0
                },
                SegmentOp::Open {
                    user: ayse.user,
                    mode: Mode::Stopwatch,
                    item: Some(Uuid::from_u128(20)),
                    base_ms: 0
                },
            ]
        );
    }

    #[test]
    fn w6_anyone_pauses_and_stops_for_everyone() {
        let (ali, ayse) = (person(1, None), person(2, None));
        let running = apply(
            &Timer::STOPPED,
            start(None),
            ali.user,
            &[ali, ayse],
            t0(),
            POMODORO,
        )
        .unwrap()
        .timer;
        let paused = apply(&running, Command::Pause, ayse.user, &[], at(10), POMODORO).unwrap();
        assert_eq!(paused.timer.state, State::Paused);
        assert!(paused.ops.is_empty());
        let open = |p: Participant| Participant {
            base_ms: Some(0),
            ..p
        };
        let stopped = apply(
            &paused.timer,
            Command::Stop,
            ayse.user,
            &[open(ali), open(ayse)],
            at(20),
            POMODORO,
        )
        .unwrap();
        assert_eq!(
            stopped.ops,
            [
                SegmentOp::Close {
                    user: ali.user,
                    active_ms: 10 * 60_000
                },
                SegmentOp::Close {
                    user: ayse.user,
                    active_ms: 10 * 60_000
                },
            ]
        );
    }

    #[test]
    fn w6_switching_the_item_changes_only_ones_own_log() {
        let (ali, ayse) = (person(1, Some(10)), person(2, Some(20)));
        let running = apply(
            &Timer::STOPPED,
            start(Some(10)),
            ali.user,
            &[ali, ayse],
            t0(),
            POMODORO,
        )
        .unwrap()
        .timer;
        let with_base = |p: Participant| Participant {
            base_ms: Some(0),
            ..p
        };
        assert_eq!(
            apply(
                &running,
                Command::SwitchItem {
                    item_id: Some(Uuid::from_u128(30))
                },
                ayse.user,
                &[with_base(ali), with_base(ayse)],
                at(5),
                POMODORO
            ),
            Err(TimerError::SwitchWhileRunning)
        );
        let paused = apply(&running, Command::Pause, ali.user, &[], at(5), POMODORO)
            .unwrap()
            .timer;
        let out = apply(
            &paused,
            Command::SwitchItem {
                item_id: Some(Uuid::from_u128(30)),
            },
            ayse.user,
            &[with_base(ali), with_base(ayse)],
            at(6),
            POMODORO,
        )
        .unwrap();
        assert_eq!(out.timer, paused, "the shared timer is untouched");
        assert_eq!(
            out.ops,
            [
                SegmentOp::Close {
                    user: ayse.user,
                    active_ms: 5 * 60_000
                },
                SegmentOp::Open {
                    user: ayse.user,
                    mode: Mode::Stopwatch,
                    item: Some(Uuid::from_u128(30)),
                    base_ms: 5 * 60_000
                },
            ]
        );
        // While stopped, only the next start's item changes.
        let stopped = apply(
            &Timer::STOPPED,
            Command::SwitchItem { item_id: None },
            ali.user,
            &[ali],
            t0(),
            POMODORO,
        )
        .unwrap();
        assert_eq!((stopped.ops.len(), stopped.item), (0, Some(None)));
    }

    #[test]
    fn w6_a_late_joiner_counts_from_when_they_joined() {
        let ali = person(1, None);
        let running = apply(
            &Timer::STOPPED,
            start(None),
            ali.user,
            &[ali],
            t0(),
            POMODORO,
        )
        .unwrap()
        .timer;
        let ayse = Uuid::from_u128(2);
        let joined = join(&running, ayse, None, at(15));
        assert_eq!(
            joined,
            [SegmentOp::Open {
                user: ayse,
                mode: Mode::Stopwatch,
                item: None,
                base_ms: 15 * 60_000
            }]
        );
        assert!(join(&Timer::STOPPED, ayse, None, at(15)).is_empty());
        let her = Participant {
            user: ayse,
            item: None,
            base_ms: Some(15 * 60_000),
        };
        assert_eq!(
            leave(&running, &her, at(40)),
            [SegmentOp::Close {
                user: ayse,
                active_ms: 25 * 60_000
            }]
        );
        assert!(leave(&Timer::STOPPED, &her, at(40)).is_empty());
    }

    proptest! {
        /// Whoever joins and pauses when, nobody's segment ever counts more than the shared
        /// timer's active time, and someone there from the start counts all of it.
        #[test]
        fn nobody_counts_more_than_the_shared_timer(join_at in 0i64..60, pause_at in 0i64..60, stop_at in 60i64..120) {
            let ali = person(1, None);
            let running = apply(&Timer::STOPPED, start(None), ali.user, &[ali], t0(), POMODORO).unwrap().timer;
            let ayse = Uuid::from_u128(2);
            let (mut timer, mut ayse_base) = (running, None);
            for minute in 0..stop_at {
                if minute == join_at
                    && let Some(SegmentOp::Open { base_ms, .. }) =
                        join(&timer, ayse, None, at(minute)).first()
                {
                    ayse_base = Some(*base_ms);
                }
                if minute == pause_at {
                    timer = apply(&timer, Command::Pause, ali.user, &[], at(minute), POMODORO).unwrap().timer;
                }
            }
            let total = timer.active_ms(at(stop_at));
            let everyone = [
                Participant { base_ms: Some(0), ..ali },
                Participant { user: ayse, item: None, base_ms: ayse_base },
            ];
            let stopped = apply(&timer, Command::Stop, ayse, &everyone, at(stop_at), POMODORO).unwrap();
            for op in &stopped.ops {
                if let SegmentOp::Close { user, active_ms } = op {
                    prop_assert!(*active_ms <= total);
                    if *user == ali.user {
                        prop_assert_eq!(*active_ms, total);
                    }
                }
            }
        }
    }
}
