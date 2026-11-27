//! Filter engine (PRD F1, F3, T1.2.7). A filter only ever narrows the items the caller can
//! already access. Values within one dimension are alternatives (OR); dimensions combine (AND).
//!
//! Tags match the caller's own tags only (E2). People match only those the caller may share with
//! (F3): asking for anyone else is refused ([`ItemsService::check_people`], T3.2.4), and when a
//! saved view runs, a person who is no longer a friend is ignored, so the view keeps working
//! after an unfriending and still cannot be used to probe who sees what.

use std::collections::HashSet;

use access::Perms;
use chrono::{DateTime, Utc};
use kernel::{AppError, FieldError, FieldErrorCode, Page, PageRequest, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{Item, ListState, valid_kind};
use crate::service::{COLUMNS, ItemRow, ItemsService, state_filter};

/// Most values in one filter dimension.
pub const FILTER_MAX_VALUES: usize = 50;
/// Most people in one filter: each one costs an access lookup.
pub const FILTER_MAX_PEOPLE: usize = 20;
const STATUS_MAX_CHARS: usize = 40;

/// Which timestamp a date filter looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "ItemFilterDateField")]
pub enum DateField {
    Created,
    Updated,
    LastTouched,
}

impl DateField {
    const fn column(self) -> &'static str {
        match self {
            Self::Created => "created_at",
            Self::Updated => "updated_at",
            Self::LastTouched => "last_touched_at",
        }
    }
}

/// A time range: `from` is included, `to` is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "ItemFilterDate")]
pub struct DateFilter {
    pub field: DateField,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub from: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub to: Option<DateTime<Utc>>,
}

/// What to show. An absent or empty dimension does not filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ItemFilter {
    /// The caller's own tags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tag_ids: Option<Vec<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kinds: Option<Vec<String>>,
    /// Items shared with any of these friends (F1, F3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub people: Option<Vec<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub team_ids: Option<Vec<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date: Option<DateFilter>,
    /// Kind-specific status values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub statuses: Option<Vec<String>>,
    /// Active when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<ListState>,
}

fn too_many(errors: &mut ValidationErrors, field: &str, len: usize, max: usize) {
    if len > max {
        errors.push(FieldError::new(
            field,
            FieldErrorCode::OutOfRange,
            format!("at most {max} values"),
        ));
    }
}

fn non_empty<T>(values: Option<&Vec<T>>) -> Option<&[T]> {
    values.map(Vec::as_slice).filter(|v| !v.is_empty())
}

impl ItemFilter {
    /// # Errors
    /// [`AppError::Validation`] for too many values, an unknown kind, a long status or an
    /// inverted date range.
    pub fn validate(&self) -> Result<(), AppError> {
        let mut errors = ValidationErrors::new();
        let len = |v: Option<usize>| v.unwrap_or(0);
        too_many(
            &mut errors,
            "tag_ids",
            len(self.tag_ids.as_ref().map(Vec::len)),
            FILTER_MAX_VALUES,
        );
        too_many(
            &mut errors,
            "kinds",
            len(self.kinds.as_ref().map(Vec::len)),
            FILTER_MAX_VALUES,
        );
        too_many(
            &mut errors,
            "people",
            len(self.people.as_ref().map(Vec::len)),
            FILTER_MAX_PEOPLE,
        );
        too_many(
            &mut errors,
            "team_ids",
            len(self.team_ids.as_ref().map(Vec::len)),
            FILTER_MAX_VALUES,
        );
        too_many(
            &mut errors,
            "statuses",
            len(self.statuses.as_ref().map(Vec::len)),
            FILTER_MAX_VALUES,
        );
        if self.kinds.iter().flatten().any(|kind| !valid_kind(kind)) {
            errors.push(FieldError::new(
                "kinds",
                FieldErrorCode::Invalid,
                "unknown item kind",
            ));
        }
        if self
            .statuses
            .iter()
            .flatten()
            .any(|status| status.is_empty() || status.chars().count() > STATUS_MAX_CHARS)
        {
            errors.push(FieldError::new(
                "statuses",
                FieldErrorCode::Invalid,
                "invalid status",
            ));
        }
        if let Some(DateFilter {
            from: Some(from),
            to: Some(to),
            ..
        }) = self.date
            && from >= to
        {
            errors.push(FieldError::new(
                "date",
                FieldErrorCode::Invalid,
                "`from` must be before `to`",
            ));
        }
        errors.into_result()
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl ItemsService {
    /// The accessible items matching `filter`, newest first.
    ///
    /// # Errors
    /// [`AppError::Validation`] for an invalid filter or cursor, or [`AppError::Internal`].
    pub async fn filter_items(
        &self,
        actor: Uuid,
        filter: &ItemFilter,
        page: &PageRequest,
    ) -> Result<Page<Item>, AppError> {
        filter.validate()?;
        let after: Option<(DateTime<Utc>, Uuid)> = page
            .cursor
            .as_ref()
            .map(kernel::Cursor::decode)
            .transpose()?;
        let (required, state_sql) = state_filter(filter.state.unwrap_or_default());
        let mut ids = self
            .access
            .permissions()
            .list_accessible(&self.pool, actor, required)
            .await
            .map_err(internal)?;
        if let Some(people) = non_empty(filter.people.as_ref()) {
            let shared = self.shared_with(actor, people).await?;
            ids.retain(|id| shared.contains(id));
        }
        if ids.is_empty() {
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let date_column = filter
            .date
            .map_or(DateField::Created, |date| date.field)
            .column();
        let rows = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM items
             WHERE id = ANY($1) AND {state_sql}
               AND ($2::text[] IS NULL OR kind = ANY($2))
               AND ($3::text[] IS NULL OR status = ANY($3))
               AND ($4::uuid[] IS NULL OR owner_team_id = ANY($4))
               AND ($5::uuid[] IS NULL OR id IN (
                     SELECT it.item_id FROM item_tags it JOIN tags t ON t.id = it.tag_id
                     WHERE it.tag_id = ANY($5) AND t.owner_user_id = $6))
               AND ($7::timestamptz IS NULL OR {date_column} >= $7)
               AND ($8::timestamptz IS NULL OR {date_column} < $8)
               AND ($9::timestamptz IS NULL OR (created_at, id) < ($9, $10))
             ORDER BY created_at DESC, id DESC
             LIMIT $11"
        )))
        .bind(&ids)
        .bind(non_empty(filter.kinds.as_ref()))
        .bind(non_empty(filter.statuses.as_ref()))
        .bind(non_empty(filter.team_ids.as_ref()))
        .bind(non_empty(filter.tag_ids.as_ref()))
        .bind(actor)
        .bind(filter.date.and_then(|date| date.from))
        .bind(filter.date.and_then(|date| date.to))
        .bind(after.map(|(at, _)| at))
        .bind(after.map(|(_, id)| id))
        .bind(page.fetch_limit())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let items = rows.into_iter().map(Item::from).collect();
        Ok(Page::from_overfetch(items, page.limit, |item: &Item| {
            (item.created_at, item.id)
        }))
    }

    /// Refuses a filter that names someone `actor` may not share with: only friends (and, with
    /// F5, teammates) can be chosen, never oneself (F3, T3.2.4).
    ///
    /// # Errors
    /// [`AppError::Validation`] for such a person, or [`AppError::Internal`].
    pub async fn check_people(&self, actor: Uuid, filter: &ItemFilter) -> Result<(), AppError> {
        for &person in filter.people.as_deref().unwrap_or_default() {
            if person == actor || !self.sharing.can_share(actor, person).await? {
                return Err(ValidationErrors::from(FieldError::new(
                    "people",
                    FieldErrorCode::Invalid,
                    "only friends can be chosen",
                ))
                .into());
            }
        }
        Ok(())
    }

    /// [`Self::filter_items`] for a filter the caller just asked for: people are checked first
    /// ([`Self::check_people`]).
    ///
    /// # Errors
    /// As [`Self::check_people`] and [`Self::filter_items`].
    pub async fn filter_checked(
        &self,
        actor: Uuid,
        filter: &ItemFilter,
        page: &PageRequest,
    ) -> Result<Page<Item>, AppError> {
        filter.validate()?;
        self.check_people(actor, filter).await?;
        self.filter_items(actor, filter, page).await
    }

    /// Items any of `people` can view, counting only those `actor` may share with (F3).
    async fn shared_with(&self, actor: Uuid, people: &[Uuid]) -> Result<HashSet<Uuid>, AppError> {
        let mut shared = HashSet::new();
        for &person in people {
            if person != actor && self.sharing.can_share(actor, person).await? {
                let theirs = self
                    .access
                    .permissions()
                    .list_accessible(&self.pool, person, Perms::VIEW)
                    .await
                    .map_err(internal)?;
                shared.extend(theirs);
            }
        }
        Ok(shared)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;

    #[test]
    fn empty_filter_is_valid() {
        assert!(ItemFilter::default().validate().is_ok());
    }

    #[test]
    fn rejects_bad_values() {
        let at = Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap();
        let bad = [
            ItemFilter {
                kinds: Some(vec!["nope".into()]),
                ..ItemFilter::default()
            },
            ItemFilter {
                statuses: Some(vec![String::new()]),
                ..ItemFilter::default()
            },
            ItemFilter {
                statuses: Some(vec!["x".repeat(STATUS_MAX_CHARS + 1)]),
                ..ItemFilter::default()
            },
            ItemFilter {
                people: Some(vec![Uuid::nil(); FILTER_MAX_PEOPLE + 1]),
                ..ItemFilter::default()
            },
            ItemFilter {
                tag_ids: Some(vec![Uuid::nil(); FILTER_MAX_VALUES + 1]),
                ..ItemFilter::default()
            },
            ItemFilter {
                date: Some(DateFilter {
                    field: DateField::Created,
                    from: Some(at),
                    to: Some(at - Duration::days(1)),
                }),
                ..ItemFilter::default()
            },
        ];
        for filter in bad {
            assert!(filter.validate().is_err(), "{filter:?}");
        }
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let parsed: Result<ItemFilter, _> = serde_json::from_str(r#"{"kind":["task"]}"#);
        assert!(parsed.is_err(), "a typo must not silently widen a filter");
    }

    #[test]
    fn round_trips_without_empty_fields() {
        let filter = ItemFilter {
            kinds: Some(vec!["task".into()]),
            state: Some(ListState::Archived),
            ..ItemFilter::default()
        };
        let json = serde_json::to_string(&filter).unwrap_or_default();
        assert_eq!(json, r#"{"kinds":["task"],"state":"archived"}"#);
        let back: Result<ItemFilter, _> = serde_json::from_str(&json);
        assert_eq!(back.ok(), Some(filter));
    }
}
