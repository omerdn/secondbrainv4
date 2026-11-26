//! Work session statistics (PRD W4, W5, T2.6.2): a pure function over segments.
//!
//! Time is only ever counted here; nothing is completed (W5, ADR-0015). Work is pomodoro and
//! stopwatch time, breaks are counted apart. A segment counts for the local day it started on,
//! in the user's time zone.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::timer::Mode;

/// One segment as the statistics see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counted {
    pub mode: Mode,
    pub item_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    /// Active time, up to now for the segment still going.
    pub active_ms: i64,
}

/// One local day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SessionDayStats")]
pub struct DayStats {
    #[ts(type = "string")]
    pub date: NaiveDate,
    #[ts(type = "number")]
    pub work_ms: i64,
    #[ts(type = "number")]
    pub break_ms: i64,
}

/// Work time on one item; `item_id` none for time on no item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SessionItemStats")]
pub struct ItemStats {
    pub item_id: Option<Uuid>,
    #[ts(type = "number")]
    pub work_ms: i64,
}

/// Statistics of a range of local days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SessionStats {
    #[ts(type = "string")]
    pub from: NaiveDate,
    #[ts(type = "string")]
    pub to: NaiveDate,
    /// Every day of the range, oldest first, zero when nothing happened.
    pub days: Vec<DayStats>,
    /// Items by work time, most first.
    pub items: Vec<ItemStats>,
    #[ts(type = "number")]
    pub work_ms: i64,
    #[ts(type = "number")]
    pub break_ms: i64,
}

/// The statistics of `segments` for the local days `from..=to` in `timezone`.
#[must_use]
pub fn summarize(
    segments: &[Counted],
    from: NaiveDate,
    to: NaiveDate,
    timezone: Tz,
) -> SessionStats {
    let mut days: BTreeMap<NaiveDate, DayStats> = from
        .iter_days()
        .take_while(|day| *day <= to)
        .map(|date| {
            (
                date,
                DayStats {
                    date,
                    work_ms: 0,
                    break_ms: 0,
                },
            )
        })
        .collect();
    let mut items: BTreeMap<Option<Uuid>, i64> = BTreeMap::new();
    for segment in segments {
        let date = segment.started_at.with_timezone(&timezone).date_naive();
        let Some(day) = days.get_mut(&date) else {
            continue;
        };
        let ms = segment.active_ms.max(0);
        if segment.mode == Mode::Break {
            day.break_ms = day.break_ms.saturating_add(ms);
        } else {
            day.work_ms = day.work_ms.saturating_add(ms);
            let total = items.entry(segment.item_id).or_default();
            *total = total.saturating_add(ms);
        }
    }
    let days: Vec<DayStats> = days.into_values().collect();
    let mut items: Vec<ItemStats> = items
        .into_iter()
        .filter(|(_, ms)| *ms > 0)
        .map(|(item_id, work_ms)| ItemStats { item_id, work_ms })
        .collect();
    items.sort_by(|a, b| b.work_ms.cmp(&a.work_ms).then(a.item_id.cmp(&b.item_id)));
    SessionStats {
        from,
        to,
        work_ms: days.iter().map(|d| d.work_ms).sum(),
        break_ms: days.iter().map(|d| d.break_ms).sum(),
        days,
        items,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;

    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn seg(mode: Mode, item: Option<Uuid>, at: DateTime<Utc>, minutes: i64) -> Counted {
        Counted {
            mode,
            item_id: item,
            started_at: at,
            active_ms: minutes * 60_000,
        }
    }

    #[test]
    fn days_follow_the_users_time_zone_and_breaks_are_apart() {
        let istanbul: Tz = "Europe/Istanbul".parse().unwrap();
        let gitar = Some(Uuid::now_v7());
        // 22:30 UTC on the 25th is 01:30 on the 26th in Istanbul (UTC+3).
        let late = Utc.with_ymd_and_hms(2026, 11, 25, 22, 30, 0).unwrap();
        let noon = Utc.with_ymd_and_hms(2026, 11, 26, 9, 0, 0).unwrap();
        let segments = [
            seg(Mode::Stopwatch, gitar, late, 45),
            seg(Mode::Break, None, noon, 5),
            seg(Mode::Pomodoro, None, noon, 25),
        ];
        let stats = summarize(&segments, day(25), day(26), istanbul);
        assert_eq!(
            stats.days,
            vec![
                DayStats {
                    date: day(25),
                    work_ms: 0,
                    break_ms: 0
                },
                DayStats {
                    date: day(26),
                    work_ms: 70 * 60_000,
                    break_ms: 5 * 60_000
                },
            ]
        );
        assert_eq!(
            stats.items,
            vec![
                ItemStats {
                    item_id: gitar,
                    work_ms: 45 * 60_000
                },
                ItemStats {
                    item_id: None,
                    work_ms: 25 * 60_000
                },
            ]
        );
        // In UTC the late segment belongs to the 25th.
        let utc = summarize(&segments, day(25), day(26), chrono_tz::UTC);
        assert_eq!(utc.days[0].work_ms, 45 * 60_000);
        // Outside the range: not counted.
        assert_eq!(summarize(&segments, day(27), day(27), istanbul).work_ms, 0);
    }

    proptest! {
        /// Totals agree: days, items and the overall sums.
        #[test]
        fn totals_agree(raw in prop::collection::vec((0u8..3, 0u8..3, 0i64..72, 0i64..200), 0..40)) {
            let ids = [None, Some(Uuid::nil()), Some(Uuid::max())];
            let base = Utc.with_ymd_and_hms(2026, 11, 24, 0, 0, 0).unwrap();
            let segments: Vec<Counted> = raw.iter().map(|(m, i, h, min)| Counted {
                mode: [Mode::Pomodoro, Mode::Stopwatch, Mode::Break][usize::from(*m)],
                item_id: ids[usize::from(*i)],
                started_at: base + chrono::Duration::hours(*h),
                active_ms: min * 60_000,
            }).collect();
            let stats = summarize(&segments, day(24), day(26), chrono_tz::UTC);
            prop_assert_eq!(stats.days.len(), 3);
            prop_assert_eq!(stats.work_ms, stats.items.iter().map(|i| i.work_ms).sum::<i64>());
            let all_work: i64 = segments.iter().filter(|s| s.mode != Mode::Break).map(|s| s.active_ms).sum();
            let all_break: i64 = segments.iter().filter(|s| s.mode == Mode::Break).map(|s| s.active_ms).sum();
            prop_assert_eq!((stats.work_ms, stats.break_ms), (all_work, all_break));
            prop_assert!(stats.items.windows(2).all(|w| w[0].work_ms >= w[1].work_ms));
        }
    }
}
