//! Item DTOs and validation (ARCHITECTURE §5.3, PRD N1–N4).

use chrono::{DateTime, Utc};
use kernel::validation::{max_chars, non_blank};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

pub const TITLE_MAX_CHARS: usize = 500;
pub const DESCRIPTION_MAX_CHARS: usize = 20_000;

/// Who owns an item (N1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export, rename = "ItemOwner")]
pub enum Owner {
    User { user_id: Uuid },
    Team { team_id: Uuid },
}

/// An item as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Item {
    pub id: Uuid,
    /// `task`, `routine`, `goal`, `tracker`, `notebook`, `page` or `module:<name>`.
    pub kind: String,
    pub title: String,
    pub description: Option<String>,
    pub owner: Owner,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_touched_at: DateTime<Utc>,
    /// Set while archived (reversible, N4).
    pub archived_at: Option<DateTime<Utc>>,
    /// Set while in the trash; purged 30 days later (N4).
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Which items a list returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "ItemListState")]
pub enum ListState {
    /// Neither archived nor in the trash.
    #[default]
    Active,
    Archived,
    /// In the trash; only items the caller may delete (restore).
    Trash,
}

/// `GET /items` query.
#[derive(Debug, Clone, Copy, Default, Deserialize, TS)]
#[ts(export, rename = "ListItemsQuery")]
pub struct ListQuery {
    #[serde(default)]
    #[ts(optional)]
    pub state: Option<ListState>,
}

/// `POST /items` body. Items are personal for now; team items arrive with teams (T5.1.3).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateItemRequest {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
}

/// `PATCH /items/{id}` body: only the fields present change. An empty `description` clears it.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateItemRequest {
    #[serde(default)]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
}

/// A validated new item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    pub kind: String,
    pub title: String,
    pub description: Option<String>,
}

/// Validated changes; `description: Some(None)` clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemChanges {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
}

impl ItemChanges {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.description.is_none()
    }
}

const BUILT_IN_KINDS: [&str; 6] = ["task", "routine", "goal", "tracker", "notebook", "page"];

/// Whether `kind` is a built-in kind or `module:<name>` (lowercase letters, digits, `_`, `-`).
#[must_use]
pub fn valid_kind(kind: &str) -> bool {
    if BUILT_IN_KINDS.contains(&kind) {
        return true;
    }
    kind.strip_prefix("module:").is_some_and(|name| {
        (1..=40).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    })
}

fn check_title(title: &str) -> [Result<(), FieldError>; 2] {
    [
        non_blank("title", title),
        max_chars("title", title, TITLE_MAX_CHARS),
    ]
}

fn normalize_description(description: Option<String>) -> Option<String> {
    description
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty())
}

impl CreateItemRequest {
    /// # Errors
    /// [`AppError::Validation`] for an unknown kind, a blank or long title or a long description.
    pub fn validate(self) -> Result<NewItem, AppError> {
        let mut errors = ValidationErrors::new();
        if !valid_kind(&self.kind) {
            errors.push(FieldError::new(
                "kind",
                FieldErrorCode::Invalid,
                "unknown item kind",
            ));
        }
        for result in check_title(&self.title) {
            errors.check(result);
        }
        let description = normalize_description(self.description);
        if let Some(d) = &description {
            errors.check(max_chars("description", d, DESCRIPTION_MAX_CHARS));
        }
        errors.into_result()?;
        Ok(NewItem {
            kind: self.kind,
            title: self.title.trim().to_owned(),
            description,
        })
    }
}

impl UpdateItemRequest {
    /// # Errors
    /// [`AppError::Validation`] for a blank or long title, a long description, or no field.
    pub fn validate(self) -> Result<ItemChanges, AppError> {
        let mut errors = ValidationErrors::new();
        if let Some(title) = &self.title {
            for result in check_title(title) {
                errors.check(result);
            }
        }
        let description = self.description.map(|d| normalize_description(Some(d)));
        if let Some(Some(d)) = &description {
            errors.check(max_chars("description", d, DESCRIPTION_MAX_CHARS));
        }
        let changes = ItemChanges {
            title: self.title.map(|t| t.trim().to_owned()),
            description,
        };
        if changes.is_empty() {
            errors.push(FieldError::new(
                "title",
                FieldErrorCode::Required,
                "nothing to change",
            ));
        }
        errors.into_result()?;
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        for kind in BUILT_IN_KINDS {
            assert!(valid_kind(kind));
        }
        assert!(valid_kind("module:budget"));
        assert!(valid_kind("module:a_b-1"));
        assert!(!valid_kind("module:"));
        assert!(!valid_kind("module:Budget"));
        assert!(!valid_kind(&format!("module:{}", "x".repeat(41))));
        assert!(!valid_kind("Task"));
        assert!(!valid_kind(""));
    }

    #[test]
    fn create_validation() {
        let ok = CreateItemRequest {
            kind: "task".into(),
            title: "  Buy milk ".into(),
            description: Some("   ".into()),
        }
        .validate()
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ok.title, "Buy milk");
        assert_eq!(ok.description, None);
        let err = CreateItemRequest {
            kind: "nope".into(),
            title: " ".into(),
            description: Some("x".repeat(DESCRIPTION_MAX_CHARS + 1)),
        }
        .validate();
        let Err(AppError::Validation(errors)) = err else {
            panic!("expected validation error");
        };
        let fields: Vec<&str> = errors.errors().iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, ["kind", "title", "description"]);
    }

    #[test]
    fn update_validation() {
        let changes = UpdateItemRequest {
            title: None,
            description: Some(String::new()),
        }
        .validate()
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            changes.description,
            Some(None),
            "empty description clears it"
        );
        assert!(UpdateItemRequest::default().validate().is_err());
        assert!(
            UpdateItemRequest {
                title: Some("x".repeat(TITLE_MAX_CHARS + 1)),
                description: None
            }
            .validate()
            .is_err()
        );
    }
}
