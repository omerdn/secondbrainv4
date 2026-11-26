//! Notebook and page DTOs (PRD Y1, Y2, Y5).

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
}
