//! Tracker and entry types (ARCHITECTURE §5.5, PRD T1, T2).

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

/// Which stretch of time a target applies to (T1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Day,
    Week,
    Month,
    /// All entries, with no time limit.
    Total,
    /// A target range; both dates are included.
    Range {
        start: NaiveDate,
        end: NaiveDate,
    },
}

impl Period {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Total => "total",
            Self::Range { .. } => "range",
        }
    }

    pub(crate) const fn range_start(self) -> Option<NaiveDate> {
        match self {
            Self::Range { start, .. } => Some(start),
            _ => None,
        }
    }

    pub(crate) const fn range_end(self) -> Option<NaiveDate> {
        match self {
            Self::Range { end, .. } => Some(end),
            _ => None,
        }
    }

    pub(crate) fn parse(
        value: &str,
        start: Option<NaiveDate>,
        end: Option<NaiveDate>,
    ) -> Option<Self> {
        match (value, start, end) {
            ("day", None, None) => Some(Self::Day),
            ("week", None, None) => Some(Self::Week),
            ("month", None, None) => Some(Self::Month),
            ("total", None, None) => Some(Self::Total),
            ("range", Some(start), Some(end)) => Some(Self::Range { start, end }),
            _ => None,
        }
    }
}

macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// Every value.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub(crate) const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub(crate) fn parse(value: &str) -> Option<Self> {
                match value {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

text_enum! {
    /// How a period's entries combine into one value (T1).
    Aggregation {
        Sum => "sum",
        Count => "count",
        /// The most recent entry.
        Last => "last",
        Max => "max",
    }
}

text_enum! {
    /// Which way is good (T1).
    Direction {
        /// Should rise to the target (pages of a book).
        Increase => "increase",
        /// Should stay under the target (budget, calories).
        Limit => "limit",
    }
}

text_enum! {
    /// What the tracker belongs to (ARCHITECTURE §5.5).
    OwnerKind {
        Standalone => "standalone",
        Routine => "routine",
        Module => "module",
    }
}

/// A tracker definition (T1). Its id is the id of its `tracker` item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracker {
    pub item_id: Uuid,
    pub unit: String,
    pub target_value: Decimal,
    pub period: Period,
    pub aggregation: Aggregation,
    pub direction: Direction,
    pub owner_kind: OwnerKind,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A tracker to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTracker {
    pub item_id: Uuid,
    pub unit: String,
    pub target_value: Decimal,
    pub period: Period,
    pub aggregation: Aggregation,
    pub direction: Direction,
    pub owner_kind: OwnerKind,
}

/// A value entered on a tracker (T2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerEntry {
    pub id: Uuid,
    pub tracker_item_id: Uuid,
    pub user_id: Uuid,
    pub value: Decimal,
    pub occurred_at: DateTime<Utc>,
    /// The user's calendar day when it occurred.
    pub local_date: NaiveDate,
    /// The entry's note is this line of the process page, its only copy (T3).
    pub note_block_id: Option<Uuid>,
    /// Interim home of the note, until process pages exist (E2.5).
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// An entry to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    pub tracker_item_id: Uuid,
    pub user_id: Uuid,
    pub value: Decimal,
    pub occurred_at: DateTime<Utc>,
    pub local_date: NaiveDate,
    pub note_block_id: Option<Uuid>,
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(2026, 11, d)
    }

    #[test]
    fn text_values_round_trip() {
        for value in Aggregation::ALL {
            assert_eq!(Aggregation::parse(value.as_str()), Some(*value));
        }
        for value in Direction::ALL {
            assert_eq!(Direction::parse(value.as_str()), Some(*value));
        }
        for value in OwnerKind::ALL {
            assert_eq!(OwnerKind::parse(value.as_str()), Some(*value));
        }
        assert_eq!(Aggregation::parse("avg"), None);
    }

    #[test]
    fn period_needs_dates_exactly_for_a_range() {
        for period in [Period::Day, Period::Week, Period::Month, Period::Total] {
            assert_eq!(Period::parse(period.as_str(), None, None), Some(period));
            assert_eq!(Period::parse(period.as_str(), day(1), day(2)), None);
        }
        let (Some(start), Some(end)) = (day(1), day(2)) else {
            panic!("valid dates");
        };
        let range = Period::Range { start, end };
        assert_eq!(
            Period::parse(range.as_str(), range.range_start(), range.range_end()),
            Some(range)
        );
        assert_eq!(Period::parse("range", day(1), None), None);
        assert_eq!(Period::parse("year", None, None), None);
    }
}
