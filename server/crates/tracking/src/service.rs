//! Tracker entries: add, list, delete (PRD T2, T1.3.4). Every authorization goes through
//! `access::PermissionService`; facts about the item come from `ItemsService` (ADR-0007).

use std::str::FromStr;

use access::{PermissionService, Perms};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use identity::IdentityService;
use identity::model::UserId;
use items::{ItemsAccessContext, ItemsService, TouchAction};
use kernel::{
    AppError, Cursor, FieldError, FieldErrorCode, Page, PageRequest, SharedClock, ValidationErrors,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{NewEntry, TrackerEntry};
use crate::repo;

/// Longest entry note (T2: "short").
pub const NOTE_MAX_CHARS: usize = 500;
/// Decimal places a value may have (`numeric(20, 4)`); more is refused rather than rounded.
const VALUE_MAX_SCALE: u32 = 4;
/// Values stay below 10^16 (`numeric(20, 4)`).
const VALUE_LIMIT: Decimal = Decimal::from_parts(1_874_919_424, 2_328_306, 0, false, 0);
/// How far ahead of the server clock a device's `occurred_at` may be.
const CLOCK_SKEW: Duration = Duration::minutes(5);

/// `POST /trackers/{id}/entries` body. The value is a decimal string, so it never passes through
/// a float.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, rename = "CreateTrackerEntryRequest")]
pub struct CreateEntryRequest {
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
    /// Now when absent.
    #[serde(default)]
    #[ts(optional)]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[ts(optional)]
    pub note: Option<String>,
}

/// `GET /trackers/{id}/entries` query: local days, both ends included.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListEntriesQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

/// An entry as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "TrackerEntry")]
pub struct EntryView {
    pub id: Uuid,
    pub tracker_item_id: Uuid,
    pub user_id: Uuid,
    /// Decimal string without trailing zeros.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
    pub occurred_at: DateTime<Utc>,
    /// The day it counts for, in the time zone of the person who entered it.
    #[ts(type = "string")]
    pub local_date: NaiveDate,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<TrackerEntry> for EntryView {
    fn from(entry: TrackerEntry) -> Self {
        Self {
            id: entry.id,
            tracker_item_id: entry.tracker_item_id,
            user_id: entry.user_id,
            value: entry.value.normalize(),
            occurred_at: entry.occurred_at,
            local_date: entry.local_date,
            note: entry.note,
            created_at: entry.created_at,
        }
    }
}

/// A validated entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidEntry {
    value: Decimal,
    occurred_at: DateTime<Utc>,
    note: Option<String>,
}

fn validate(request: CreateEntryRequest, now: DateTime<Utc>) -> Result<ValidEntry, AppError> {
    let mut errors = ValidationErrors::new();
    if request.value.normalize().scale() > VALUE_MAX_SCALE {
        errors.push(FieldError::new(
            "value",
            FieldErrorCode::Invalid,
            format!("at most {VALUE_MAX_SCALE} decimal places"),
        ));
    }
    if request.value.abs() >= VALUE_LIMIT {
        errors.push(FieldError::new(
            "value",
            FieldErrorCode::OutOfRange,
            "value is too large",
        ));
    }
    let occurred_at = request.occurred_at.unwrap_or(now);
    if occurred_at > now + CLOCK_SKEW {
        errors.push(FieldError::new(
            "occurred_at",
            FieldErrorCode::Invalid,
            "cannot be in the future",
        ));
    }
    let note = request
        .note
        .map(|note| note.trim().to_owned())
        .filter(|note| !note.is_empty());
    if note
        .as_ref()
        .is_some_and(|note| note.chars().count() > NOTE_MAX_CHARS)
    {
        errors.push(FieldError::new(
            "note",
            FieldErrorCode::TooLong,
            format!("at most {NOTE_MAX_CHARS} characters"),
        ));
    }
    errors.into_result()?;
    Ok(ValidEntry {
        value: request.value,
        occurred_at,
        note,
    })
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct TrackerEntries {
    pool: PgPool,
    clock: SharedClock,
    permissions: PermissionService<ItemsAccessContext>,
    items: ItemsService,
    identity: IdentityService,
}

impl TrackerEntries {
    #[must_use]
    pub const fn new(
        pool: PgPool,
        clock: SharedClock,
        permissions: PermissionService<ItemsAccessContext>,
        items: ItemsService,
        identity: IdentityService,
    ) -> Self {
        Self {
            pool,
            clock,
            permissions,
            items,
            identity,
        }
    }

    /// Adds an entry (EDIT). Its `local_date` is the actor's calendar day at `occurred_at`.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a tracker the actor cannot see, [`AppError::Forbidden`] without
    /// EDIT, [`AppError::Validation`], [`AppError::Conflict`] for an item in the trash, or
    /// [`AppError::Internal`].
    pub async fn add(
        &self,
        actor: Uuid,
        tracker: Uuid,
        request: CreateEntryRequest,
    ) -> Result<EntryView, AppError> {
        self.permissions
            .require(&self.pool, actor, tracker, Perms::EDIT)
            .await?;
        self.require_live_tracker(actor, tracker).await?;
        let now = self.clock.now();
        let entry = validate(request, now)?;
        let user = self.identity.current_user(UserId::from_uuid(actor)).await?;
        let timezone = Tz::from_str(&user.timezone).map_err(internal)?;
        let new = NewEntry {
            tracker_item_id: tracker,
            user_id: actor,
            value: entry.value,
            occurred_at: entry.occurred_at,
            local_date: recurrence::local_date(entry.occurred_at, timezone),
            note_block_id: None,
            note: entry.note,
        };
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let stored = repo::insert_entry(&mut *tx, &new, now)
            .await
            .map_err(internal)?;
        self.items
            .touch(&mut tx, tracker, TouchAction::EntryLogged)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(stored.into())
    }

    /// A tracker's entries, newest first (VIEW).
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Validation`] for a bad cursor, or
    /// [`AppError::Internal`].
    pub async fn list(
        &self,
        actor: Uuid,
        tracker: Uuid,
        days: (Option<NaiveDate>, Option<NaiveDate>),
        page: &PageRequest,
    ) -> Result<Page<EntryView>, AppError> {
        self.permissions
            .require(&self.pool, actor, tracker, Perms::VIEW)
            .await?;
        repo::get_tracker(&self.pool, tracker)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let after: Option<(DateTime<Utc>, Uuid)> =
            page.cursor.as_ref().map(Cursor::decode).transpose()?;
        let entries = repo::page_entries(&self.pool, tracker, days, after, page.fetch_limit())
            .await
            .map_err(internal)?;
        let views = entries.into_iter().map(EntryView::from).collect();
        Ok(Page::from_overfetch(
            views,
            page.limit,
            |view: &EntryView| (view.occurred_at, view.id),
        ))
    }

    /// Deletes an entry. One's own needs EDIT, someone else's needs MANAGE.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an unknown or already deleted entry, [`AppError::Forbidden`],
    /// [`AppError::Conflict`] for an item in the trash, or [`AppError::Internal`].
    pub async fn delete(&self, actor: Uuid, tracker: Uuid, entry: Uuid) -> Result<(), AppError> {
        self.permissions
            .require(&self.pool, actor, tracker, Perms::EDIT)
            .await?;
        self.require_live_tracker(actor, tracker).await?;
        let stored = repo::get_entry(&self.pool, tracker, entry)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        if stored.user_id != actor {
            self.permissions
                .require(&self.pool, actor, tracker, Perms::MANAGE)
                .await?;
        }
        let deleted = repo::soft_delete_entry(&self.pool, tracker, entry, self.clock.now())
            .await
            .map_err(internal)?;
        if deleted {
            Ok(())
        } else {
            Err(AppError::NotFound)
        }
    }

    async fn require_live_tracker(&self, actor: Uuid, tracker: Uuid) -> Result<(), AppError> {
        let item = self.items.get(actor, tracker).await?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        repo::get_tracker(&self.pool, tracker)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap()
    }

    fn request(value: Decimal) -> CreateEntryRequest {
        CreateEntryRequest {
            value,
            occurred_at: None,
            note: None,
        }
    }

    #[test]
    fn value_limit_is_ten_to_the_sixteenth() {
        assert_eq!(VALUE_LIMIT, Decimal::from(10_000_000_000_000_000_i64));
    }

    #[test]
    fn valid_entries() {
        let entry = validate(request(Decimal::new(12_345, 4)), now()).unwrap();
        assert_eq!(entry.occurred_at, now());
        assert_eq!(entry.note, None);
        // Trailing zeros are not extra precision; negative values are allowed.
        assert!(validate(request(Decimal::new(1_000_000, 6)), now()).is_ok());
        assert!(validate(request(Decimal::new(-5, 0)), now()).is_ok());
        let noted = CreateEntryRequest {
            note: Some("  ok  ".into()),
            occurred_at: Some(now() + CLOCK_SKEW),
            ..request(Decimal::ONE)
        };
        assert_eq!(validate(noted, now()).unwrap().note.as_deref(), Some("ok"));
        let blank = CreateEntryRequest {
            note: Some("   ".into()),
            ..request(Decimal::ONE)
        };
        assert_eq!(validate(blank, now()).unwrap().note, None);
    }

    #[test]
    fn invalid_entries() {
        let bad = [
            request(Decimal::new(12_345, 5)),
            request(VALUE_LIMIT),
            request(-VALUE_LIMIT),
            CreateEntryRequest {
                occurred_at: Some(now() + CLOCK_SKEW + Duration::seconds(1)),
                ..request(Decimal::ONE)
            },
            CreateEntryRequest {
                note: Some("x".repeat(NOTE_MAX_CHARS + 1)),
                ..request(Decimal::ONE)
            },
        ];
        for item in bad {
            assert!(validate(item.clone(), now()).is_err(), "{item:?}");
        }
    }
}
