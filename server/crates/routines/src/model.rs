//! Routine DTOs, validation and pure plan rules (PRD R1–R3, R11, R13).

use std::str::FromStr;

use chrono::{NaiveDate, NaiveTime};
use chrono_tz::Tz;
use items::Item;
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use recurrence::RuleSet;
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Highest daily target (R3).
pub const DAILY_TARGET_MAX: i32 = 100;
/// Most reminder times one routine may have (R13).
pub const REMINDER_LIMIT: usize = 24;
/// Longest stored plan text; "yearly on selected dates" takes one line per date (R1).
const RRULE_MAX_CHARS: usize = 20_000;

/// `POST /routines` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateRoutineRequest {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
    /// IANA time zone; the routine's days end in this zone (R2).
    pub timezone: String,
    /// One RRULE per line (R1).
    pub rrule: String,
    /// 1 when absent (R3).
    #[serde(default)]
    #[ts(optional)]
    pub daily_target: Option<i32>,
    /// Today in the routine's zone when absent.
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub start_date: Option<NaiveDate>,
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub end_date: Option<NaiveDate>,
    /// `HH:MM` local times (R13).
    #[serde(default)]
    #[ts(optional)]
    pub reminder_times: Option<Vec<String>>,
    /// How a shared routine's day is completed; a shared pool when absent (R5).
    #[serde(default)]
    #[ts(optional)]
    pub completion_mode: Option<crate::counts::CompletionMode>,
    /// Who is responsible rotates between the members (R6); fixed assignees when absent.
    #[serde(default)]
    #[ts(optional)]
    pub rotation: Option<crate::rotation::RoutineRotation>,
    /// The team that owns it, with its visibility and people (V12, TK4, T5.1.8); personal
    /// when absent.
    #[serde(default)]
    #[ts(optional)]
    pub team: Option<items::TeamPlacementRequest>,
}

/// `PATCH /routines/{id}` body: only the fields present change, and a `null` end date clears it.
/// The title and description belong to the item (`PATCH /items/{id}`); the plan and daily target
/// change through `POST /routines/{id}/plan` (R11).
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateRoutineRequest {
    #[serde(default)]
    #[ts(optional)]
    pub timezone: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[ts(optional, type = "string | null")]
    pub end_date: Option<Option<NaiveDate>>,
    #[serde(default)]
    #[ts(optional)]
    pub reminder_times: Option<Vec<String>>,
    /// R5; past days are read under the new mode too.
    #[serde(default)]
    #[ts(optional)]
    pub completion_mode: Option<crate::counts::CompletionMode>,
    /// R6; `null` goes back to fixed assignees and drops the swaps.
    #[serde(default, deserialize_with = "present")]
    #[ts(optional = nullable, as = "Option<crate::rotation::RoutineRotation>")]
    pub rotation: Option<Option<crate::rotation::RoutineRotation>>,
    /// The turn order: current members, each once. Members left out join at the end.
    #[serde(default)]
    #[ts(optional)]
    pub rotation_order: Option<Vec<Uuid>>,
}

/// `POST /routines/{id}/plan` body: opens a new plan version (R11).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct ChangePlanRequest {
    /// One RRULE per line (R1).
    pub rrule: String,
    /// The current target when absent.
    #[serde(default)]
    #[ts(optional)]
    pub daily_target: Option<i32>,
    /// Today (or the routine's first day, if later) when absent. Never in the past (R11).
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub valid_from: Option<NaiveDate>,
}

/// Tells a field set to `null` (`Some(None)`) from an absent one (`None`).
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    de: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(de).map(Some)
}

/// One plan version as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "RoutinePlanVersion")]
pub struct PlanVersionView {
    pub id: Uuid,
    pub rrule: String,
    pub daily_target: i32,
    #[ts(type = "string")]
    pub valid_from: NaiveDate,
    #[ts(type = "string | null")]
    pub valid_to: Option<NaiveDate>,
}

/// A routine as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "Routine")]
pub struct RoutineView {
    pub item: Item,
    pub timezone: String,
    /// The target of the version in force today (the latest one if none is).
    pub daily_target: i32,
    #[ts(type = "string")]
    pub start_date: NaiveDate,
    #[ts(type = "string | null")]
    pub end_date: Option<NaiveDate>,
    /// `HH:MM` local times, in order.
    pub reminder_times: Vec<String>,
    pub completion_mode: crate::counts::CompletionMode,
    /// `null` for fixed assignees (R6).
    pub rotation: Option<crate::rotation::RoutineRotation>,
    /// Plan history, oldest first (R11).
    pub versions: Vec<PlanVersionView>,
}

pub(crate) fn invalid(field: &str, message: impl Into<String>) -> AppError {
    let mut errors = ValidationErrors::new();
    errors.push(FieldError::new(
        field,
        FieldErrorCode::Invalid,
        message.into(),
    ));
    match errors.into_result() {
        Err(err) => err,
        Ok(()) => AppError::NotFound,
    }
}

/// # Errors
/// [`AppError::Validation`] for an unknown IANA zone.
pub fn parse_timezone(timezone: &str) -> Result<Tz, AppError> {
    Tz::from_str(timezone.trim()).map_err(|_| invalid("timezone", "unknown time zone"))
}

/// Parses and canonicalizes a plan (R1).
///
/// # Errors
/// [`AppError::Validation`] for an empty, unsupported or oversized plan.
pub fn parse_rules(rrule: &str) -> Result<RuleSet, AppError> {
    if rrule.chars().count() > RRULE_MAX_CHARS {
        return Err(invalid("rrule", "the plan is too long"));
    }
    RuleSet::from_str(rrule).map_err(|err| invalid("rrule", err.to_string()))
}

/// Parses `HH:MM` times into a sorted list without duplicates (R13).
///
/// # Errors
/// [`AppError::Validation`] for a malformed time or more than [`REMINDER_LIMIT`].
pub fn parse_reminders(times: &[String]) -> Result<Vec<NaiveTime>, AppError> {
    let mut parsed = times
        .iter()
        .map(|time| {
            let time = time.trim();
            if time.len() != 5 {
                return Err(invalid("reminder_times", "use HH:MM"));
            }
            NaiveTime::parse_from_str(time, "%H:%M")
                .map_err(|_| invalid("reminder_times", "use HH:MM"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    parsed.sort_unstable();
    parsed.dedup();
    if parsed.len() > REMINDER_LIMIT {
        return Err(invalid(
            "reminder_times",
            format!("at most {REMINDER_LIMIT} reminders"),
        ));
    }
    Ok(parsed)
}

pub(crate) fn check_target(target: i32) -> Result<(), AppError> {
    if (1..=DAILY_TARGET_MAX).contains(&target) {
        Ok(())
    } else {
        let mut errors = ValidationErrors::new();
        errors.push(FieldError::new(
            "daily_target",
            FieldErrorCode::OutOfRange,
            format!("must be between 1 and {DAILY_TARGET_MAX}"),
        ));
        errors.into_result()
    }
}

/// The daily target on `date`: the version in force, or the latest one outside every version.
#[must_use]
pub fn target_on(versions: &[PlanVersionView], date: NaiveDate) -> i32 {
    versions
        .iter()
        .find(|v| v.valid_from <= date && v.valid_to.is_none_or(|end| date <= end))
        .or_else(|| versions.last())
        .map_or(1, |v| v.daily_target)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn version(target: i32, from: u32, to: Option<u32>) -> PlanVersionView {
        PlanVersionView {
            id: Uuid::now_v7(),
            rrule: "FREQ=DAILY".into(),
            daily_target: target,
            valid_from: day(from),
            valid_to: to.map(day),
        }
    }

    #[test]
    fn r1_every_recurrence_kind_parses() {
        for rule in [
            "FREQ=DAILY",
            "FREQ=DAILY;INTERVAL=3",
            "FREQ=WEEKLY;BYDAY=MO,WE",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=FR",
            "FREQ=MONTHLY;BYMONTHDAY=15,20",
            "FREQ=MONTHLY;BYMONTHDAY=-1",
            "FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=8\nFREQ=YEARLY;BYMONTH=9;BYMONTHDAY=1",
        ] {
            assert!(parse_rules(rule).is_ok(), "{rule}");
        }
    }

    #[test]
    fn unsupported_plans_are_refused() {
        for rule in ["", "FREQ=HOURLY", "FREQ=DAILY;COUNT=3", "nonsense"] {
            assert!(parse_rules(rule).is_err(), "{rule}");
        }
        assert!(parse_rules(&"FREQ=DAILY\n".repeat(3_000)).is_err());
    }

    #[test]
    fn r2_time_zones_are_checked() {
        assert_eq!(
            parse_timezone(" Europe/Istanbul ").unwrap(),
            Tz::Europe__Istanbul
        );
        assert!(parse_timezone("Mars/Base").is_err());
    }

    #[test]
    fn r13_reminders_are_sorted_and_unique() {
        let times = ["20:30".to_owned(), "08:00".to_owned(), "20:30".to_owned()];
        let parsed = parse_reminders(&times).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].format("%H:%M").to_string(), "08:00");
        for bad in ["8:00", "25:00", "08:00:00", "x"] {
            assert!(parse_reminders(&[bad.to_owned()]).is_err(), "{bad}");
        }
        let many: Vec<String> = (0..25).map(|h| format!("{:02}:00", h % 24)).collect();
        assert!(parse_reminders(&many).is_ok());
        let too_many: Vec<String> = (0..25).map(|m| format!("08:{m:02}")).collect();
        assert!(parse_reminders(&too_many).is_err());
    }

    #[test]
    fn r3_target_range() {
        assert!(check_target(1).is_ok());
        assert!(check_target(DAILY_TARGET_MAX).is_ok());
        assert!(check_target(0).is_err());
        assert!(check_target(DAILY_TARGET_MAX + 1).is_err());
    }

    #[test]
    fn r11_target_follows_the_version_in_force() {
        let versions = [version(2, 1, Some(9)), version(8, 10, Some(20))];
        assert_eq!(target_on(&versions, day(9)), 2);
        assert_eq!(target_on(&versions, day(10)), 8);
        assert_eq!(target_on(&versions, day(25)), 8);
        assert_eq!(target_on(&[], day(1)), 1);
    }

    proptest! {
        #[test]
        fn reminders_never_panic_and_stay_sorted(raw in proptest::collection::vec("\\PC{0,6}", 0..6)) {
            if let Ok(parsed) = parse_reminders(&raw) {
                prop_assert!(parsed.windows(2).all(|w| w[0] < w[1]));
            }
        }
    }
}
