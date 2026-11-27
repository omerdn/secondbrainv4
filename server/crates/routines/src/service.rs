//! `RoutinesService`: routines and plan versions (PRD R1–R3, R11, R13, T2.2.1). Every
//! authorization goes through `access::PermissionService`; facts about the item come from
//! `ItemsService` (ADR-0007).

use access::{PermissionService, Perms};
use chrono::NaiveDate;
use items::{CreateItemRequest, Item, ItemsAccessContext, ItemsService, TouchAction};
use kernel::{AppError, SharedClock};
use recurrence::{PlanVersion, Schedule, local_date};
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{
    ChangePlanRequest, CreateRoutineRequest, PlanVersionView, RoutineView, UpdateRoutineRequest,
    check_target, invalid, parse_reminders, parse_rules, parse_timezone, target_on,
};
use crate::repo::{self, RoutineRow};

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn view(
    item: Item,
    row: RoutineRow,
    versions: Vec<PlanVersionView>,
    today: NaiveDate,
) -> RoutineView {
    RoutineView {
        item,
        daily_target: target_on(&versions, today),
        timezone: row.timezone,
        start_date: row.start_date,
        end_date: row.end_date,
        reminder_times: row
            .reminder_times
            .iter()
            .map(|time| time.format("%H:%M").to_string())
            .collect(),
        completion_mode: crate::counts::CompletionMode::parse(&row.completion_mode)
            .unwrap_or_default(),
        versions,
    }
}

/// The stored versions as a [`Schedule`]; they were validated when written.
fn schedule(timezone: chrono_tz::Tz, versions: &[PlanVersionView]) -> Result<Schedule, AppError> {
    let plans = versions
        .iter()
        .map(|v| {
            Ok(PlanVersion {
                rules: v.rrule.parse().map_err(internal)?,
                valid_from: v.valid_from,
                valid_to: v.valid_to,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Schedule::new(timezone, plans).map_err(internal)
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct RoutinesService {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
    pub(crate) permissions: PermissionService<ItemsAccessContext>,
    pub(crate) items: ItemsService,
}

impl RoutinesService {
    #[must_use]
    pub const fn new(
        pool: PgPool,
        clock: SharedClock,
        permissions: PermissionService<ItemsAccessContext>,
        items: ItemsService,
    ) -> Self {
        Self {
            pool,
            clock,
            permissions,
            items,
        }
    }

    /// Creates a personal routine with its item and first plan version, in one transaction.
    ///
    /// # Errors
    /// [`AppError::Validation`], or [`AppError::Internal`].
    pub async fn create(
        &self,
        actor: Uuid,
        request: CreateRoutineRequest,
    ) -> Result<RoutineView, AppError> {
        let timezone = parse_timezone(&request.timezone)?;
        let rules = parse_rules(&request.rrule)?;
        let daily_target = request.daily_target.unwrap_or(1);
        check_target(daily_target)?;
        let reminder_times = parse_reminders(request.reminder_times.as_deref().unwrap_or(&[]))?;
        let now = self.clock.now();
        let start_date = request
            .start_date
            .unwrap_or_else(|| local_date(now, timezone));
        if request.end_date.is_some_and(|end| end < start_date) {
            return Err(invalid("end_date", "must not be before the start date"));
        }
        let new_item = CreateItemRequest {
            kind: "routine".into(),
            title: request.title,
            description: request.description,
        }
        .validate()?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let item = self.items.create_in(&mut tx, actor, new_item).await?;
        let row = RoutineRow {
            item_id: item.id,
            timezone: timezone.name().to_owned(),
            start_date,
            end_date: request.end_date,
            reminder_times,
            completion_mode: request
                .completion_mode
                .unwrap_or_default()
                .as_str()
                .to_owned(),
        };
        repo::insert_routine(&mut tx, &row, now)
            .await
            .map_err(internal)?;
        repo::insert_version(
            &mut tx,
            item.id,
            (&rules.to_string(), daily_target),
            (start_date, request.end_date),
            now,
        )
        .await
        .map_err(internal)?;
        // Completions live on the routine's own tracker (§5.6).
        crate::completions::ensure_tracker(&mut tx, item.id, daily_target, now)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.routine(actor, item.id).await
    }

    /// The people a day is counted for (R5): the routine's assignees, or its owner when nobody
    /// is assigned (N3).
    pub(crate) async fn people(&self, id: Uuid) -> Result<Vec<Uuid>, AppError> {
        let (owner, assignees) = self.items.owner_and_assignees(id).await?;
        Ok(if assignees.is_empty() {
            owner.into_iter().collect()
        } else {
            assignees
        })
    }

    /// What everyone did on the days of `entries`, read under the routine's mode.
    pub(crate) async fn counts(
        &self,
        row: &RoutineRow,
        entries: &[tracking::TrackerEntry],
    ) -> Result<crate::counts::DayCounts, AppError> {
        let mode = crate::counts::CompletionMode::parse(&row.completion_mode).unwrap_or_default();
        let people = match mode {
            crate::counts::CompletionMode::Pool => Vec::new(),
            crate::counts::CompletionMode::Individual => self.people(row.item_id).await?,
        };
        Ok(crate::counts::DayCounts::new(mode, people, entries))
    }

    /// A routine with its plan history (VIEW).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not visible or the item is not a routine, or
    /// [`AppError::Internal`].
    pub async fn routine(&self, actor: Uuid, id: Uuid) -> Result<RoutineView, AppError> {
        let item = self.items.get(actor, id).await?;
        let row = repo::get_routine(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let versions = repo::list_versions(&self.pool, id)
            .await
            .map_err(internal)?;
        let timezone = parse_timezone(&row.timezone)?;
        let today = local_date(self.clock.now(), timezone);
        Ok(view(item, row, versions, today))
    }

    /// Changes the time zone, end date or reminders (EDIT). Past days stay as they were (R11): the
    /// end cannot move into the past, and reopening an ended routine starts a new version today
    /// rather than filling the gap.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`], [`AppError::Validation`],
    /// [`AppError::Conflict`] for a routine in the trash, or [`AppError::Internal`].
    pub async fn update(
        &self,
        actor: Uuid,
        id: Uuid,
        request: UpdateRoutineRequest,
    ) -> Result<RoutineView, AppError> {
        self.require_live(actor, id).await?;
        if request.timezone.is_none()
            && request.end_date.is_none()
            && request.reminder_times.is_none()
            && request.completion_mode.is_none()
        {
            return Err(invalid("timezone", "nothing to change"));
        }
        let timezone = request
            .timezone
            .as_deref()
            .map(parse_timezone)
            .transpose()?;
        let reminders = request
            .reminder_times
            .as_deref()
            .map(parse_reminders)
            .transpose()?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let mut row = repo::lock_routine(&mut tx, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        if let Some(timezone) = timezone {
            timezone.name().clone_into(&mut row.timezone);
        }
        if let Some(reminders) = reminders {
            row.reminder_times = reminders;
        }
        if let Some(mode) = request.completion_mode {
            mode.as_str().clone_into(&mut row.completion_mode);
        }
        if let Some(end) = request.end_date {
            let today = local_date(now, parse_timezone(&row.timezone)?);
            let versions = repo::list_versions(&mut *tx, id).await.map_err(internal)?;
            let last = versions.last().ok_or(AppError::NotFound)?;
            if end.is_some_and(|end| end < today) {
                return Err(invalid("end_date", "must not be in the past"));
            }
            if end.is_some_and(|end| end < last.valid_from) {
                return Err(invalid(
                    "end_date",
                    "must not be before the latest plan starts",
                ));
            }
            if last.valid_to.is_some_and(|old| old < today) {
                repo::insert_version(
                    &mut tx,
                    id,
                    (&last.rrule, last.daily_target),
                    (today, end),
                    now,
                )
                .await
                .map_err(internal)?;
            } else {
                repo::set_version_end(&mut tx, last.id, end)
                    .await
                    .map_err(internal)?;
            }
            row.end_date = end;
        }
        repo::update_routine(&mut tx, &row, now)
            .await
            .map_err(internal)?;
        self.items
            .touch(&mut tx, id, TouchAction::Edited)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.routine(actor, id).await
    }

    /// Opens a new plan version (EDIT, R11): earlier days keep their rules and target. A change
    /// on the current version's first day replaces it.
    ///
    /// # Errors
    /// As [`Self::update`]; [`AppError::Validation`] for a start in the past.
    pub async fn change_plan(
        &self,
        actor: Uuid,
        id: Uuid,
        request: ChangePlanRequest,
    ) -> Result<RoutineView, AppError> {
        self.require_live(actor, id).await?;
        let rules = parse_rules(&request.rrule)?;
        if let Some(target) = request.daily_target {
            check_target(target)?;
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row = repo::lock_routine(&mut tx, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let timezone = parse_timezone(&row.timezone)?;
        let today = local_date(now, timezone);
        let versions = repo::list_versions(&mut *tx, id).await.map_err(internal)?;
        let last = versions.last().ok_or(AppError::NotFound)?;
        let from = request.valid_from.unwrap_or(today.max(last.valid_from));
        if from < today {
            return Err(invalid("valid_from", "must not be in the past"));
        }
        let daily_target = request
            .daily_target
            .unwrap_or_else(|| target_on(&versions, from));
        let mut plan = schedule(timezone, &versions)?;
        plan.change_plan(rules.clone(), from)
            .map_err(|err| invalid("valid_from", err.to_string()))?;
        let text = rules.to_string();
        if plan.versions().len() == versions.len() {
            repo::replace_version(&mut tx, last.id, &text, daily_target)
                .await
                .map_err(internal)?;
        } else {
            let day_before = from
                .pred_opt()
                .ok_or_else(|| invalid("valid_from", "out of range"))?;
            repo::set_version_end(&mut tx, last.id, Some(day_before))
                .await
                .map_err(internal)?;
            repo::insert_version(
                &mut tx,
                id,
                (&text, daily_target),
                (from, last.valid_to),
                now,
            )
            .await
            .map_err(internal)?;
        }
        self.items
            .touch(&mut tx, id, TouchAction::Edited)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.routine(actor, id).await
    }

    /// EDIT on a routine that is not in the trash. An item that is not a routine is not found.
    pub(crate) async fn require_live(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        self.permissions
            .require(&self.pool, actor, id, Perms::EDIT)
            .await?;
        let item = self.items.get(actor, id).await?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        repo::get_routine(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        Ok(())
    }
}
