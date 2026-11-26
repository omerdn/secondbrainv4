//! Goal DTOs and validation (PRD D1-D3).

use chrono::NaiveDate;
use items::Item;
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Item kinds whose progress feeds the goal (D2). A linked page is shown but never counts.
pub const CONTRIBUTING_KINDS: [&str; 3] = ["task", "routine", "tracker"];
/// Item kinds that can be linked to a goal (D2).
pub const LINKABLE_KINDS: [&str; 4] = ["task", "routine", "tracker", "page"];
/// Highest weight of one link (D3).
pub const WEIGHT_MAX: Decimal = Decimal::from_parts(100, 0, 0, false, 0);
/// Decimal places a weight may have; the column is `numeric(6, 2)`.
const WEIGHT_SCALE: u32 = 2;

/// `POST /goals` body. Both dates are required (D1); they are optional here only so a missing
/// one is reported as a field error rather than a malformed body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateGoalRequest {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default)]
    #[ts(type = "string")]
    pub start_date: Option<NaiveDate>,
    #[serde(default)]
    #[ts(type = "string")]
    pub end_date: Option<NaiveDate>,
}

/// `PATCH /goals/{id}` body: only the dates present change; they can never be cleared (D1). The
/// title and description belong to the item (`PATCH /items/{id}`).
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateGoalRequest {
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub start_date: Option<NaiveDate>,
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub end_date: Option<NaiveDate>,
}

/// `PUT /goals/{id}/links/{item_id}` body.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct LinkGoalRequest {
    /// Decimal string, more than 0 and at most 100, two decimals at most; 1 when absent (D3).
    /// Pages take no weight (D2).
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[ts(optional, type = "string")]
    pub weight: Option<Decimal>,
}

/// An item linked to a goal as the API returns it. Items the caller cannot see are never
/// listed (V9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct GoalLink {
    pub item: Item,
    /// Decimal string (D3).
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub weight: Decimal,
    /// Whether the item's progress counts towards the goal: false for pages (D2).
    pub contributes: bool,
}

/// A goal as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Goal {
    pub item: Item,
    #[ts(type = "string")]
    pub start_date: NaiveDate,
    #[ts(type = "string")]
    pub end_date: NaiveDate,
    /// Linked items the caller can see, oldest link first.
    pub links: Vec<GoalLink>,
}

pub(crate) fn invalid(field: &str, code: FieldErrorCode, message: &str) -> AppError {
    let mut errors = ValidationErrors::new();
    errors.push(FieldError::new(field, code, message));
    match errors.into_result() {
        Err(err) => err,
        Ok(()) => AppError::NotFound,
    }
}

/// Both dates, start first (D1).
///
/// # Errors
/// [`AppError::Validation`] for a missing date or an end before the start.
pub fn check_dates(
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
) -> Result<(NaiveDate, NaiveDate), AppError> {
    let mut errors = ValidationErrors::new();
    if start.is_none() {
        errors.push(FieldError::new(
            "start_date",
            FieldErrorCode::Required,
            "a goal needs a start date",
        ));
    }
    if end.is_none() {
        errors.push(FieldError::new(
            "end_date",
            FieldErrorCode::Required,
            "a goal needs an end date",
        ));
    }
    errors.into_result()?;
    match (start, end) {
        (Some(start), Some(end)) if start <= end => Ok((start, end)),
        (Some(_), Some(_)) => Err(invalid(
            "end_date",
            FieldErrorCode::Invalid,
            "must not be before the start date",
        )),
        _ => Err(AppError::NotFound),
    }
}

/// A weight in range with at most two decimals (D3).
///
/// # Errors
/// [`AppError::Validation`] otherwise.
pub fn check_weight(weight: Decimal) -> Result<Decimal, AppError> {
    if weight <= Decimal::ZERO || weight > WEIGHT_MAX {
        return Err(invalid(
            "weight",
            FieldErrorCode::OutOfRange,
            "must be more than 0 and at most 100",
        ));
    }
    if weight.normalize().scale() > WEIGHT_SCALE {
        return Err(invalid(
            "weight",
            FieldErrorCode::Invalid,
            "at most two decimals",
        ));
    }
    Ok(weight.normalize())
}

/// Whether an item of `kind` feeds the goal's progress (D2).
#[must_use]
pub fn contributes(kind: &str) -> bool {
    CONTRIBUTING_KINDS.contains(&kind)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn d1_both_dates_are_required() {
        assert!(matches!(
            check_dates(None, Some(day(2))),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            check_dates(Some(day(1)), None),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            check_dates(None, None),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            check_dates(Some(day(2)), Some(day(1))),
            Err(AppError::Validation(_))
        ));
        assert_eq!(
            check_dates(Some(day(1)), Some(day(1))).unwrap(),
            (day(1), day(1))
        );
    }

    #[test]
    fn d2_pages_are_linked_but_never_count() {
        for kind in ["task", "routine", "tracker"] {
            assert!(contributes(kind));
        }
        assert!(!contributes("page"));
        assert!(LINKABLE_KINDS.contains(&"page"));
        assert!(!LINKABLE_KINDS.contains(&"goal"));
    }

    #[test]
    fn d3_weights_are_bounded() {
        assert_eq!(
            check_weight(Decimal::new(250, 2)).unwrap(),
            Decimal::new(25, 1)
        );
        assert!(check_weight(Decimal::ZERO).is_err());
        assert!(check_weight(Decimal::new(-1, 0)).is_err());
        assert!(check_weight(Decimal::new(10_001, 2)).is_err());
        assert!(check_weight(Decimal::new(1_005, 3)).is_err());
        assert!(check_weight(Decimal::new(1_000, 3)).is_ok());
        assert_eq!(check_weight(WEIGHT_MAX).unwrap(), WEIGHT_MAX);
    }

    proptest! {
        #[test]
        fn d1_any_ordered_pair_is_accepted(a in 0_u64..2_000, b in 0_u64..2_000) {
            let base = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
            let (x, y) = (base + chrono::Days::new(a), base + chrono::Days::new(b));
            prop_assert_eq!(check_dates(Some(x), Some(y)).is_ok(), x <= y);
        }

        #[test]
        fn d3_accepted_weights_fit_the_column(cents in -1_000_i64..20_000) {
            let weight = Decimal::new(cents, 2);
            if let Ok(w) = check_weight(weight) {
                prop_assert!(w > Decimal::ZERO && w <= WEIGHT_MAX);
                prop_assert!(w.scale() <= 2);
            } else {
                prop_assert!(weight <= Decimal::ZERO || weight > WEIGHT_MAX);
            }
        }
    }
}
