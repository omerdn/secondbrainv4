//! Today's routines (PRD H1, T2.7.1): every active routine the caller can view that has an
//! occurrence today, in its own time zone, with what was done on it. Answers from this crate's
//! tables; items come from `ItemsService` (ADR-0007).

use access::Perms;
use items::Item;
use kernel::AppError;
use recurrence::local_date;
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::completions::{RoutineDay, routine_day, schedule_of};
use crate::model::{parse_timezone, target_on};
use crate::repo;
use crate::service::RoutinesService;

/// A routine with today's occurrence (H1); it is marked with the completion endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RoutineToday {
    pub item: Item,
    pub timezone: String,
    /// Today in the routine's time zone.
    pub day: RoutineDay,
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl RoutinesService {
    /// Active routines `actor` can view that are scheduled today, by title, at most `limit`.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn today(&self, actor: Uuid, limit: usize) -> Result<Vec<RoutineToday>, AppError> {
        let visible = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        let rows = repo::list_routines(&self.pool, &visible)
            .await
            .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let mut items: Vec<Item> = self
            .items
            .visible_items(actor, &ids)
            .await?
            .into_iter()
            .filter(|item| item.archived_at.is_none() && item.deleted_at.is_none())
            .collect();
        items.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        let now = self.clock.now();
        let mut today = Vec::new();
        for item in items {
            if today.len() >= limit {
                break;
            }
            let Some(row) = rows.iter().find(|row| row.item_id == item.id) else {
                continue;
            };
            let timezone = parse_timezone(&row.timezone)?;
            let date = local_date(now, timezone);
            if date < row.start_date || row.end_date.is_some_and(|end| end < date) {
                continue;
            }
            let versions = repo::list_versions(&self.pool, item.id)
                .await
                .map_err(internal)?;
            if !schedule_of(timezone, &versions)?.occurs_on(date) {
                continue;
            }
            let entries = tracking::repo::list_entries(&self.pool, item.id, Some(date), Some(date))
                .await
                .map_err(internal)?;
            let (done, target) = self
                .counts(row, &entries)
                .await?
                .day(date, target_on(&versions, date));
            today.push(RoutineToday {
                day: routine_day(date, true, target, done),
                timezone: row.timezone.clone(),
                item,
            });
        }
        Ok(today)
    }
}

/// A routine's occurrences in a calendar range (PRD C1), as dates of its own time zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RoutineOccurrences {
    pub item: Item,
    pub timezone: String,
    #[ts(type = "Array<string>")]
    pub dates: Vec<chrono::NaiveDate>,
}

impl RoutinesService {
    /// Active routines `actor` can view with their occurrences on the calendar days
    /// `from..=to`, computed by `recurrence` from the plan versions (ARCHITECTURE §8). A
    /// routine's days are calendar dates of its own zone: a daily routine has exactly one
    /// occurrence a day, DST or not. Routines without one in the range are left out. By title,
    /// at most `limit`.
    ///
    /// # Errors
    /// [`AppError::Validation`] for a range `recurrence` refuses, or [`AppError::Internal`].
    pub async fn occurrences(
        &self,
        actor: Uuid,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
        limit: usize,
    ) -> Result<Vec<RoutineOccurrences>, AppError> {
        let visible = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        let rows = repo::list_routines(&self.pool, &visible)
            .await
            .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let mut items: Vec<Item> = self
            .items
            .visible_items(actor, &ids)
            .await?
            .into_iter()
            .filter(|item| item.archived_at.is_none() && item.deleted_at.is_none())
            .collect();
        items.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        let mut found = Vec::new();
        for item in items {
            if found.len() >= limit {
                break;
            }
            let Some(row) = rows.iter().find(|row| row.item_id == item.id) else {
                continue;
            };
            let timezone = parse_timezone(&row.timezone)?;
            let versions = repo::list_versions(&self.pool, item.id)
                .await
                .map_err(internal)?;
            let dates = schedule_of(timezone, &versions)?
                .occurrences(from, to)
                .map_err(|err| crate::model::invalid("from", err.to_string()))?;
            if dates.is_empty() {
                continue;
            }
            found.push(RoutineOccurrences {
                item,
                timezone: row.timezone.clone(),
                dates,
            });
        }
        Ok(found)
    }
}

/// How well a team kept a routine lately (PRD TK6, T5.2.7): scheduled days of the window and
/// how many of them were complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RoutineAdherence {
    pub item: Item,
    /// Days of the window with an occurrence.
    pub scheduled: i32,
    /// Of those, days whose target was reached.
    pub complete: i32,
}

impl RoutinesService {
    /// Active routines owned by `team` that `actor` can view, with their adherence over the
    /// `days` days ending on `today`, by title (TK6, V9). Routines `actor` cannot see are left out
    /// without a trace.
    ///
    /// # Errors
    /// [`AppError::Validation`] for a bad window, or [`AppError::Internal`].
    pub async fn team_adherence(
        &self,
        actor: Uuid,
        team: Uuid,
        today: chrono::NaiveDate,
        days: u32,
    ) -> Result<Vec<RoutineAdherence>, AppError> {
        let visible = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        let rows = repo::list_routines(&self.pool, &visible)
            .await
            .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let mut items: Vec<Item> = self
            .items
            .visible_items(actor, &ids)
            .await?
            .into_iter()
            .filter(|item| {
                item.archived_at.is_none()
                    && item.deleted_at.is_none()
                    && item.owner == items::Owner::Team { team_id: team }
            })
            .collect();
        items.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        let from = today - chrono::Days::new(u64::from(days.saturating_sub(1)));
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let window = self
                .days(
                    actor,
                    item.id,
                    crate::completions::RoutineDaysQuery {
                        from: Some(from),
                        to: Some(today),
                    },
                )
                .await?;
            let scheduled = window.iter().filter(|day| day.scheduled).count();
            let complete = window
                .iter()
                .filter(|day| day.scheduled && day.complete)
                .count();
            out.push(RoutineAdherence {
                item,
                scheduled: i32::try_from(scheduled).unwrap_or(i32::MAX),
                complete: i32::try_from(complete).unwrap_or(i32::MAX),
            });
        }
        Ok(out)
    }
}
