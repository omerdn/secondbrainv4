//! Tracker definitions: create, read, change (PRD T1, T5, K1, T1.3.5). A tracker is created
//! together with its item in one transaction, so neither can exist without the other.

use access::Perms;
use chrono::{DateTime, Utc};
use items::{CreateItemRequest, Item, TouchAction};
use kernel::validation::{max_chars, non_blank};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{Aggregation, Direction, NewTracker, OwnerKind, Period, Tracker};
use crate::repo::{self, TrackerChanges};
use crate::service::{TrackerEntries, VALUE_LIMIT, VALUE_MAX_SCALE};

pub const UNIT_MAX_CHARS: usize = 30;

/// `POST /trackers` body. The target is a decimal string, so it never passes through a float.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateTrackerRequest {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
    pub unit: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub target_value: Decimal,
    pub period: Period,
    pub aggregation: Aggregation,
    pub direction: Direction,
}

/// `PATCH /trackers/{id}` body: only the fields present change. The title and description belong
/// to the item (`PATCH /items/{id}`).
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateTrackerRequest {
    #[serde(default)]
    #[ts(optional)]
    pub unit: Option<String>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[ts(optional, type = "string")]
    pub target_value: Option<Decimal>,
    #[serde(default)]
    #[ts(optional)]
    pub period: Option<Period>,
    #[serde(default)]
    #[ts(optional)]
    pub aggregation: Option<Aggregation>,
    #[serde(default)]
    #[ts(optional)]
    pub direction: Option<Direction>,
}

/// A tracker as the API returns it, with its item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "Tracker")]
pub struct TrackerView {
    pub item: Item,
    pub unit: String,
    /// Decimal string without trailing zeros.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub target_value: Decimal,
    pub period: Period,
    pub aggregation: Aggregation,
    pub direction: Direction,
    pub owner_kind: OwnerKind,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn view(item: Item, tracker: Tracker) -> TrackerView {
    TrackerView {
        item,
        unit: tracker.unit,
        target_value: tracker.target_value.normalize(),
        period: tracker.period,
        aggregation: tracker.aggregation,
        direction: tracker.direction,
        owner_kind: tracker.owner_kind,
        created_at: tracker.created_at,
        updated_at: tracker.updated_at,
    }
}

fn check_unit(errors: &mut ValidationErrors, unit: &str) {
    errors.check(non_blank("unit", unit));
    errors.check(max_chars("unit", unit, UNIT_MAX_CHARS));
}

fn check_target(errors: &mut ValidationErrors, target: Decimal) {
    if target <= Decimal::ZERO || target >= VALUE_LIMIT {
        errors.push(FieldError::new(
            "target_value",
            FieldErrorCode::OutOfRange,
            "must be positive and below 10^16",
        ));
    }
    if target.normalize().scale() > VALUE_MAX_SCALE {
        errors.push(FieldError::new(
            "target_value",
            FieldErrorCode::Invalid,
            format!("at most {VALUE_MAX_SCALE} decimal places"),
        ));
    }
}

fn check_period(errors: &mut ValidationErrors, period: Period) {
    if let Period::Range { start, end } = period
        && start > end
    {
        errors.push(FieldError::new(
            "period",
            FieldErrorCode::Invalid,
            "the range must not end before it starts",
        ));
    }
}

impl CreateTrackerRequest {
    fn check(&self) -> Result<(), AppError> {
        let mut errors = ValidationErrors::new();
        check_unit(&mut errors, self.unit.trim());
        check_target(&mut errors, self.target_value);
        check_period(&mut errors, self.period);
        errors.into_result()
    }
}

impl UpdateTrackerRequest {
    fn validate(self) -> Result<TrackerChanges, AppError> {
        let mut errors = ValidationErrors::new();
        let unit = self.unit.map(|unit| unit.trim().to_owned());
        if let Some(unit) = &unit {
            check_unit(&mut errors, unit);
        }
        if let Some(target) = self.target_value {
            check_target(&mut errors, target);
        }
        if let Some(period) = self.period {
            check_period(&mut errors, period);
        }
        let changes = TrackerChanges {
            unit,
            target_value: self.target_value,
            period: self.period,
            aggregation: self.aggregation,
            direction: self.direction,
        };
        if changes == TrackerChanges::default() {
            errors.push(FieldError::new(
                "unit",
                FieldErrorCode::Required,
                "nothing to change",
            ));
        }
        errors.into_result()?;
        Ok(changes)
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl TrackerEntries {
    /// Creates a personal tracker: its item and definition, in one transaction.
    ///
    /// # Errors
    /// [`AppError::Validation`] for an invalid definition, or [`AppError::Internal`].
    pub async fn create_tracker(
        &self,
        actor: Uuid,
        request: CreateTrackerRequest,
    ) -> Result<TrackerView, AppError> {
        request.check()?;
        let new_item = CreateItemRequest {
            kind: "tracker".into(),
            title: request.title,
            description: request.description,
        }
        .validate()?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let item = self.items.create_in(&mut tx, actor, new_item).await?;
        let definition = NewTracker {
            item_id: item.id,
            unit: request.unit.trim().to_owned(),
            target_value: request.target_value,
            period: request.period,
            aggregation: request.aggregation,
            direction: request.direction,
            owner_kind: OwnerKind::Standalone,
        };
        let tracker = repo::insert_tracker(&mut *tx, &definition, now)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(view(item, tracker))
    }

    /// A tracker with its item (VIEW).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not visible or the item has no tracker, or
    /// [`AppError::Internal`].
    pub async fn tracker(&self, actor: Uuid, id: Uuid) -> Result<TrackerView, AppError> {
        let item = self.items.get(actor, id).await?;
        let tracker = repo::get_tracker(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        Ok(view(item, tracker))
    }

    /// Changes a tracker's definition (EDIT). Past entries stay as they are.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`], [`AppError::Validation`],
    /// [`AppError::Conflict`] for an item in the trash, or [`AppError::Internal`].
    pub async fn update_tracker(
        &self,
        actor: Uuid,
        id: Uuid,
        request: UpdateTrackerRequest,
    ) -> Result<TrackerView, AppError> {
        self.permissions
            .require(&self.pool, actor, id, Perms::EDIT)
            .await?;
        self.require_live_tracker(actor, id).await?;
        let changes = request.validate()?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let tracker = repo::update_tracker(&mut *tx, id, &changes, self.clock.now())
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        self.items
            .touch(&mut tx, id, TouchAction::Edited)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        let item = self.items.get(actor, id).await?;
        Ok(view(item, tracker))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn request() -> CreateTrackerRequest {
        CreateTrackerRequest {
            title: "Reading".into(),
            description: None,
            unit: "sayfa".into(),
            target_value: Decimal::from(300),
            period: Period::Total,
            aggregation: Aggregation::Sum,
            direction: Direction::Increase,
        }
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, d).unwrap()
    }

    #[test]
    fn valid_definition() {
        assert!(request().check().is_ok());
        let ranged = CreateTrackerRequest {
            period: Period::Range {
                start: day(1),
                end: day(1),
            },
            target_value: Decimal::new(12_345, 4),
            ..request()
        };
        assert!(ranged.check().is_ok());
    }

    #[test]
    fn invalid_definitions() {
        let bad = [
            CreateTrackerRequest {
                unit: "  ".into(),
                ..request()
            },
            CreateTrackerRequest {
                unit: "x".repeat(UNIT_MAX_CHARS + 1),
                ..request()
            },
            CreateTrackerRequest {
                target_value: Decimal::ZERO,
                ..request()
            },
            CreateTrackerRequest {
                target_value: Decimal::from(-1),
                ..request()
            },
            CreateTrackerRequest {
                target_value: VALUE_LIMIT,
                ..request()
            },
            CreateTrackerRequest {
                target_value: Decimal::new(12_345, 5),
                ..request()
            },
            CreateTrackerRequest {
                period: Period::Range {
                    start: day(2),
                    end: day(1),
                },
                ..request()
            },
        ];
        for item in bad {
            assert!(item.check().is_err(), "{item:?}");
        }
    }

    #[test]
    fn update_validation() {
        assert!(UpdateTrackerRequest::default().validate().is_err());
        let trimmed = UpdateTrackerRequest {
            unit: Some(" dakika ".into()),
            ..UpdateTrackerRequest::default()
        };
        assert_eq!(trimmed.validate().unwrap().unit.as_deref(), Some("dakika"));
        let changes = UpdateTrackerRequest {
            direction: Some(Direction::Limit),
            aggregation: Some(Aggregation::Max),
            ..UpdateTrackerRequest::default()
        };
        assert!(changes.validate().is_ok());
        for bad in [
            UpdateTrackerRequest {
                unit: Some(" ".into()),
                ..UpdateTrackerRequest::default()
            },
            UpdateTrackerRequest {
                target_value: Some(Decimal::ZERO),
                ..UpdateTrackerRequest::default()
            },
            UpdateTrackerRequest {
                period: Some(Period::Range {
                    start: day(2),
                    end: day(1),
                }),
                ..UpdateTrackerRequest::default()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn period_json_shape() {
        let week: Period = serde_json_period(r#"{"type":"week"}"#);
        assert_eq!(week, Period::Week);
        let range: Period =
            serde_json_period(r#"{"type":"range","start":"2026-05-01","end":"2026-05-02"}"#);
        assert_eq!(
            range,
            Period::Range {
                start: day(1),
                end: day(2)
            }
        );
    }

    fn serde_json_period(raw: &str) -> Period {
        serde_json::from_str(raw).unwrap()
    }
}
