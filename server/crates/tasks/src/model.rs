//! Task DTOs, validation and the pure rules of G2 and G4.

use chrono::{DateTime, NaiveDate, Utc};
use items::{Item, ItemLink, TITLE_MAX_CHARS, Tag};
use kernel::validation::{max_chars, non_blank};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Most subtasks one task may have.
pub const SUBTASK_LIMIT: usize = 200;

/// A task's item status while a subtask is open (F1, T3.0.3).
pub const STATUS_OPEN: &str = "open";
/// A task's item status once every subtask is done (G3).
pub const STATUS_DONE: &str = "done";

/// `POST /tasks` body. Without subtasks, one is created from the title (G2).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub start_date: Option<NaiveDate>,
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub due_date: Option<NaiveDate>,
    #[serde(default)]
    #[ts(optional)]
    pub subtasks: Option<Vec<CreateSubtaskRequest>>,
}

/// `POST /tasks/{id}/subtasks` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateSubtaskRequest {
    pub title: String,
}

/// `PATCH /tasks/{id}/subtasks/{subtask_id}` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct UpdateSubtaskRequest {
    pub title: String,
}

/// `PATCH /tasks/{id}` body: only the fields present change, and `null` clears a date. The title
/// and description belong to the item (`PATCH /items/{id}`).
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateTaskRequest {
    #[serde(default, deserialize_with = "present")]
    #[ts(optional, type = "string | null")]
    pub start_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "present")]
    #[ts(optional, type = "string | null")]
    pub due_date: Option<Option<NaiveDate>>,
}

/// Tells a field set to `null` (`Some(None)`) from an absent one (`None`).
fn present<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Option<NaiveDate>>, D::Error> {
    Option::<NaiveDate>::deserialize(de).map(Some)
}

/// A subtask as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "Subtask")]
pub struct SubtaskView {
    pub id: Uuid,
    pub task_id: Uuid,
    pub title: String,
    pub position: i32,
    pub done_at: Option<DateTime<Utc>>,
    pub done_by: Option<Uuid>,
    pub assignee_ids: Vec<Uuid>,
}

/// A task as the API returns it, with every G1 field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "Task")]
pub struct TaskView {
    /// Title and description live on the item.
    pub item: Item,
    #[ts(type = "string | null")]
    pub start_date: Option<NaiveDate>,
    #[ts(type = "string | null")]
    pub due_date: Option<NaiveDate>,
    pub subtasks: Vec<SubtaskView>,
    /// The caller's own tags (E2).
    pub tags: Vec<Tag>,
    /// Linked pages the caller can see (V9).
    pub pages: Vec<ItemLink>,
    /// The owner and the subtask assignees.
    pub member_ids: Vec<Uuid>,
}

/// The subtasks a new task starts with: the given ones, or its title (G2).
#[must_use]
pub fn subtask_titles(title: &str, subtasks: Option<&[CreateSubtaskRequest]>) -> Vec<String> {
    match subtasks {
        Some(list) if !list.is_empty() => list.iter().map(|s| s.title.trim().to_owned()).collect(),
        _ => vec![title.trim().to_owned()],
    }
}

/// Whether `actor` may mark a subtask done or undone (G4): anyone with EDIT when nobody is
/// assigned; otherwise one of the assignees, or someone who manages the task.
#[must_use]
pub fn may_mark(actor: Uuid, assignees: &[Uuid], can_manage: bool) -> bool {
    assignees.is_empty() || assignees.contains(&actor) || can_manage
}

pub(crate) fn check_subtask_title(errors: &mut ValidationErrors, field: &str, title: &str) {
    errors.check(non_blank(field, title));
    errors.check(max_chars(field, title, TITLE_MAX_CHARS));
}

pub(crate) fn check_dates(
    start: Option<NaiveDate>,
    due: Option<NaiveDate>,
) -> Result<(), AppError> {
    let mut errors = ValidationErrors::new();
    if let (Some(start), Some(due)) = (start, due)
        && start > due
    {
        errors.push(FieldError::new(
            "due_date",
            FieldErrorCode::Invalid,
            "must not be before the start date",
        ));
    }
    errors.into_result()
}

impl CreateTaskRequest {
    pub(crate) fn check(&self) -> Result<(), AppError> {
        check_dates(self.start_date, self.due_date)?;
        let mut errors = ValidationErrors::new();
        if let Some(subtasks) = &self.subtasks {
            if subtasks.len() > SUBTASK_LIMIT {
                errors.push(FieldError::new(
                    "subtasks",
                    FieldErrorCode::OutOfRange,
                    format!("at most {SUBTASK_LIMIT} subtasks"),
                ));
            }
            for subtask in subtasks {
                check_subtask_title(&mut errors, "subtasks", subtask.title.trim());
            }
        }
        errors.into_result()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn sub(title: &str) -> CreateSubtaskRequest {
        CreateSubtaskRequest {
            title: title.into(),
        }
    }

    #[test]
    fn g2_title_becomes_the_subtask_when_none_given() {
        assert_eq!(subtask_titles(" Write ", None), ["Write"]);
        assert_eq!(subtask_titles("Write", Some(&[])), ["Write"]);
        assert_eq!(
            subtask_titles("Write", Some(&[sub(" a "), sub("b")])),
            ["a", "b"]
        );
    }

    #[test]
    fn g4_who_may_mark() {
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        assert!(may_mark(a, &[], false));
        assert!(may_mark(a, &[b, a], false));
        assert!(!may_mark(a, &[b], false));
        assert!(may_mark(a, &[b], true));
    }

    #[test]
    fn dates_must_be_in_order() {
        assert!(check_dates(Some(day(1)), Some(day(1))).is_ok());
        assert!(check_dates(None, Some(day(1))).is_ok());
        assert!(check_dates(Some(day(2)), Some(day(1))).is_err());
    }

    #[test]
    fn update_tells_null_from_absent() {
        let absent: UpdateTaskRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.due_date, None);
        let cleared: UpdateTaskRequest = serde_json::from_str(r#"{"due_date":null}"#).unwrap();
        assert_eq!(cleared.due_date, Some(None));
        let set: UpdateTaskRequest = serde_json::from_str(r#"{"due_date":"2026-11-02"}"#).unwrap();
        assert_eq!(set.due_date, Some(Some(day(2))));
    }
}

/// Someone assigned another person to a subtask (BL2, T4.3.7); published in the assignment's
/// transaction. Self-assignment publishes nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubtaskAssigned {
    pub task_id: Uuid,
    pub subtask_id: Uuid,
    pub subtask_title: String,
    pub assigned_by: Uuid,
    pub user_id: Uuid,
}

impl kernel::DomainEvent for SubtaskAssigned {
    const EVENT_TYPE: &'static str = "task.subtask_assigned";
}
