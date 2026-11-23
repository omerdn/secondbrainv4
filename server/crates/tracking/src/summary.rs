//! A tracker's current period: value, remaining amount and overrun (PRD K2, T2.4.2). The
//! calculation is a pure function of the definition, the entries and the local day.

use std::str::FromStr;

use access::Perms;
use chrono::NaiveDate;
use chrono_tz::Tz;
use identity::model::UserId;
use kernel::AppError;
use rust_decimal::Decimal;
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::aggregate::{aggregate, window_for};
use crate::model::{Direction, Tracker, TrackerEntry};
use crate::repo;
use crate::service::TrackerEntries;

/// `GET /trackers/{id}/summary`. Amounts are decimal strings without trailing zeros.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TrackerSummary {
    /// The caller's local day the period was chosen for.
    #[ts(type = "string")]
    pub today: NaiveDate,
    /// First day of the period; `null` for a total.
    #[ts(type = "string | null")]
    pub period_start: Option<NaiveDate>,
    /// Last day of the period; `null` for a total.
    #[ts(type = "string | null")]
    pub period_end: Option<NaiveDate>,
    /// The period's value; `null` when a "last" or "highest" tracker has no entry yet.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[ts(type = "string | null")]
    pub value: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub target_value: Decimal,
    /// How much is left up to the target; never negative.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub remaining: Decimal,
    /// How far the value went past the target; never negative.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub exceeded_by: Decimal,
    /// A limit tracker that went past its limit (K2).
    pub over_limit: bool,
}

/// The summary of the period containing `today`. Entries outside it are ignored.
#[must_use]
pub fn summarize(tracker: &Tracker, entries: &[TrackerEntry], today: NaiveDate) -> TrackerSummary {
    let window = window_for(tracker.period, today);
    let value = aggregate(
        tracker.aggregation,
        entries.iter().filter(|e| window.contains(e.local_date)),
    );
    let used = value.unwrap_or(Decimal::ZERO);
    let remaining = tracker.target_value.saturating_sub(used).max(Decimal::ZERO);
    let exceeded_by = used.saturating_sub(tracker.target_value).max(Decimal::ZERO);
    TrackerSummary {
        today,
        period_start: window.start,
        period_end: window.end,
        value: value.as_ref().map(Decimal::normalize),
        target_value: tracker.target_value.normalize(),
        remaining: remaining.normalize(),
        exceeded_by: exceeded_by.normalize(),
        over_limit: tracker.direction == Direction::Limit && exceeded_by > Decimal::ZERO,
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl TrackerEntries {
    /// The tracker's current period in the caller's time zone (VIEW).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not visible or the item has no tracker, or
    /// [`AppError::Internal`].
    pub async fn summary(&self, actor: Uuid, id: Uuid) -> Result<TrackerSummary, AppError> {
        self.permissions
            .require(&self.pool, actor, id, Perms::VIEW)
            .await?;
        let tracker = repo::get_tracker(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let user = self.identity.current_user(UserId::from_uuid(actor)).await?;
        let timezone = Tz::from_str(&user.timezone).map_err(internal)?;
        let today = recurrence::local_date(self.clock.now(), timezone);
        let window = window_for(tracker.period, today);
        let entries = repo::list_entries(&self.pool, id, window.start, window.end)
            .await
            .map_err(internal)?;
        Ok(summarize(&tracker, &entries, today))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::{Duration, NaiveTime};
    use proptest::prelude::*;

    use super::*;
    use crate::model::{Aggregation, OwnerKind, Period};

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn tracker(period: Period, aggregation: Aggregation, direction: Direction) -> Tracker {
        let at = day(1).and_time(NaiveTime::MIN).and_utc();
        Tracker {
            item_id: Uuid::nil(),
            unit: "TL".into(),
            target_value: Decimal::from(100),
            period,
            aggregation,
            direction,
            owner_kind: OwnerKind::Standalone,
            created_at: at,
            updated_at: at,
        }
    }

    fn entry(value: Decimal, on: NaiveDate, minute: i64) -> TrackerEntry {
        let at = on.and_time(NaiveTime::MIN).and_utc() + Duration::minutes(minute);
        TrackerEntry {
            id: Uuid::now_v7(),
            tracker_item_id: Uuid::nil(),
            user_id: Uuid::nil(),
            value,
            occurred_at: at,
            local_date: on,
            note_block_id: None,
            note: None,
            created_at: at,
            deleted_at: None,
        }
    }

    #[test]
    fn k2_a_limit_shows_use_and_what_is_left() {
        let limit = tracker(Period::Week, Aggregation::Sum, Direction::Limit);
        // Monday 23 to Sunday 29 November; the 22nd belongs to the week before.
        let entries = [
            entry(Decimal::from(500), day(22), 0),
            entry(Decimal::from(30), day(23), 0),
            entry(Decimal::new(125, 1), day(25), 0),
        ];
        let summary = summarize(&limit, &entries, day(25));
        assert_eq!(summary.period_start, Some(day(23)));
        assert_eq!(summary.period_end, Some(day(29)));
        assert_eq!(summary.value, Some(Decimal::new(425, 1)));
        assert_eq!(summary.remaining, Decimal::new(575, 1));
        assert_eq!(summary.exceeded_by, Decimal::ZERO);
        assert!(!summary.over_limit);
    }

    #[test]
    fn k2_going_past_the_limit_is_reported() {
        let limit = tracker(Period::Day, Aggregation::Sum, Direction::Limit);
        let summary = summarize(&limit, &[entry(Decimal::from(130), day(5), 0)], day(5));
        assert_eq!(summary.remaining, Decimal::ZERO);
        assert_eq!(summary.exceeded_by, Decimal::from(30));
        assert!(summary.over_limit);
    }

    #[test]
    fn exactly_at_the_limit_is_not_over() {
        let limit = tracker(Period::Day, Aggregation::Sum, Direction::Limit);
        let summary = summarize(&limit, &[entry(Decimal::from(100), day(5), 0)], day(5));
        assert_eq!(summary.remaining, Decimal::ZERO);
        assert!(!summary.over_limit);
    }

    #[test]
    fn a_rising_tracker_past_its_target_is_not_over_a_limit() {
        let rising = tracker(Period::Total, Aggregation::Sum, Direction::Increase);
        let summary = summarize(&rising, &[entry(Decimal::from(150), day(5), 0)], day(9));
        assert_eq!((summary.period_start, summary.period_end), (None, None));
        assert_eq!(summary.exceeded_by, Decimal::from(50));
        assert!(!summary.over_limit);
    }

    #[test]
    fn no_entry_has_no_last_value_but_a_full_remainder() {
        let last = tracker(Period::Day, Aggregation::Last, Direction::Limit);
        let summary = summarize(&last, &[], day(5));
        assert_eq!(summary.value, None);
        assert_eq!(summary.remaining, Decimal::from(100));
    }

    #[test]
    fn deleted_entries_do_not_count() {
        let limit = tracker(Period::Day, Aggregation::Sum, Direction::Limit);
        let mut gone = entry(Decimal::from(90), day(5), 0);
        gone.deleted_at = Some(gone.occurred_at);
        let summary = summarize(&limit, &[gone], day(5));
        assert_eq!(summary.value, Some(Decimal::ZERO));
    }

    fn aggregation() -> impl Strategy<Value = Aggregation> {
        prop::sample::select(Aggregation::ALL)
    }

    fn direction() -> impl Strategy<Value = Direction> {
        prop::sample::select(Direction::ALL)
    }

    proptest! {
        #[test]
        fn k2_remaining_and_overrun_never_conflict(
            values in prop::collection::vec((-1_000_000_i64..1_000_000, 1_u32..=28), 0..30),
            aggregation in aggregation(),
            direction in direction(),
            today in 1_u32..=28,
        ) {
            let definition = tracker(Period::Month, aggregation, direction);
            let entries: Vec<TrackerEntry> = values
                .iter()
                .enumerate()
                .map(|(i, (v, d))| entry(Decimal::new(*v, 2), day(*d), i64::try_from(i).unwrap()))
                .collect();
            let summary = summarize(&definition, &entries, day(today));
            prop_assert!(summary.remaining >= Decimal::ZERO);
            prop_assert!(summary.exceeded_by >= Decimal::ZERO);
            prop_assert!(summary.remaining.is_zero() || summary.exceeded_by.is_zero());
            let used = summary.value.unwrap_or(Decimal::ZERO);
            prop_assert_eq!(
                used + summary.remaining - summary.exceeded_by,
                summary.target_value
            );
            prop_assert_eq!(
                summary.over_limit,
                direction == Direction::Limit && used > summary.target_value
            );
        }
    }
}
