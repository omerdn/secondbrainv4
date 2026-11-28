//! Notebook and page DTOs (PRD Y1, Y2, Y5).

use chrono::{DateTime, Utc};
use items::Item;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// `POST /notebooks` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateNotebookRequest {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
}

/// Most editors plus viewers a team notebook is created with.
pub const TEAM_NOTEBOOK_MAX_PEOPLE: usize = 50;

/// `POST /teams/{team_id}/notebooks` body (Y2, T5.1.5): visible to the whole team, or to the
/// chosen people (`visibility: "assignees"`) and managers and above.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateTeamNotebookRequest {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
    /// `all` when absent.
    #[serde(default)]
    #[ts(optional)]
    pub visibility: Option<items::TeamVisibilityMode>,
    /// Chosen team members who edit its pages.
    #[serde(default)]
    #[ts(optional)]
    pub editors: Option<Vec<Uuid>>,
    /// Chosen team members who read its pages; an editor listed here stays an editor.
    #[serde(default)]
    #[ts(optional)]
    pub viewers: Option<Vec<Uuid>>,
}

/// `POST /notebooks/{id}/pages` body. The page belongs to the notebook's owner (Y5).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreatePageRequest {
    pub title: String,
}

/// A notebook in the caller's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct NotebookSummary {
    pub item: Item,
    /// The notebook every user gets once, automatically.
    pub is_default: bool,
    /// Someone besides the owner is a member: the UI shows an "ortak" badge (Y2).
    pub shared: bool,
}

/// A page as the API returns it. Its owner is the notebook's owner (Y5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "NotebookPage")]
pub struct Page {
    pub item: Item,
    pub notebook_id: Uuid,
    /// Set when this is an item's process page.
    pub process_of_item_id: Option<Uuid>,
}

/// A notebook with the pages the caller can see, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Notebook {
    pub item: Item,
    pub is_default: bool,
    pub pages: Vec<Page>,
}

/// One version of a page in its history (PRD Y3): a snapshot, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PageVersion {
    pub version: i32,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
    /// The first line of the version's text, for the list.
    pub preview: String,
}

/// A version's content: its text, and its Yjs state to show it read-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PageVersionContent {
    pub version: i32,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
    pub plain_text: String,
    /// The whole document as one Yjs update (v1), base64.
    pub state: String,
}

/// `GET /pages/{id}/versions` query.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct VersionsQuery {
    /// Only versions older than this one (the next page of the list).
    pub before: Option<i32>,
}

/// Longest preview of a version in the list.
pub(crate) const PREVIEW_MAX_CHARS: usize = 120;

/// The first line of `plain_text`, shortened.
#[must_use]
pub(crate) fn preview(plain_text: &str) -> String {
    plain_text
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(PREVIEW_MAX_CHARS)
        .collect()
}

/// Title of the automatic notebook in the user's language.
#[must_use]
pub fn default_title(locale: &str) -> &'static str {
    if locale.starts_with("tr") {
        "Defterim"
    } else {
        "My notebook"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_notebook_is_named_in_the_users_language() {
        assert_eq!(default_title("tr"), "Defterim");
        assert_eq!(default_title("tr-TR"), "Defterim");
        assert_eq!(default_title("en"), "My notebook");
    }

    #[test]
    fn a_preview_is_the_first_line_shortened() {
        assert_eq!(preview("Başlık\nikinci"), "Başlık");
        assert_eq!(preview(""), "");
        assert_eq!(preview(&"ş".repeat(500)).chars().count(), PREVIEW_MAX_CHARS);
    }
}
