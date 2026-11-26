//! `SearchService`: merges the modules' search sources (ADR-0007).

use std::collections::HashMap;
use std::sync::Arc;

use items::{Item, ItemsService};
use kernel::search::{MatchField, SearchSource, SourceHit};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Longest query accepted.
pub const MAX_QUERY_CHARS: usize = 100;
/// Most results of one search.
pub const SEARCH_LIMIT: usize = 50;

/// `GET /search` query.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
}

/// Where a result matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SearchField {
    Title,
    Tag,
    Description,
    Content,
}

impl From<MatchField> for SearchField {
    fn from(field: MatchField) -> Self {
        match field {
            MatchField::Title => Self::Title,
            MatchField::Tag => Self::Tag,
            MatchField::Description => Self::Description,
            MatchField::Content => Self::Content,
        }
    }
}

/// One result: an item the caller can view (AR2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SearchHit {
    pub item: Item,
    pub field: SearchField,
    /// The matching tag's name, or an excerpt of the page.
    pub snippet: Option<String>,
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct SearchService {
    items: ItemsService,
    sources: Arc<Vec<Arc<dyn SearchSource>>>,
}

fn check_query(raw: &str) -> Result<&str, AppError> {
    let query = raw.trim();
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return Err(ValidationErrors::from(FieldError::new(
            "q",
            if query.is_empty() {
                FieldErrorCode::Required
            } else {
                FieldErrorCode::TooLong
            },
            format!("1 to {MAX_QUERY_CHARS} characters"),
        ))
        .into());
    }
    Ok(query)
}

/// The best hit of each item, best first.
fn merge(hits: Vec<SourceHit>) -> Vec<SourceHit> {
    let mut best: HashMap<Uuid, SourceHit> = HashMap::new();
    for hit in hits {
        match best.get_mut(&hit.item_id) {
            Some(kept) if kept.rank >= hit.rank => {
                if kept.snippet.is_none() && kept.field == hit.field {
                    kept.snippet = hit.snippet;
                }
            }
            _ => {
                best.insert(hit.item_id, hit);
            }
        }
    }
    let mut merged: Vec<SourceHit> = best.into_values().collect();
    merged.sort_by(|a, b| {
        b.rank
            .total_cmp(&a.rank)
            .then(a.field.cmp(&b.field))
            .then(a.item_id.cmp(&b.item_id))
    });
    merged
}

impl SearchService {
    #[must_use]
    pub fn new(items: ItemsService, sources: Vec<Arc<dyn SearchSource>>) -> Self {
        Self {
            items,
            sources: Arc::new(sources),
        }
    }

    /// Items `actor` can view that match `query`, best first, at most [`SEARCH_LIMIT`].
    ///
    /// # Errors
    /// [`AppError::Validation`] for an empty or long query, or [`AppError::Internal`].
    pub async fn search(&self, actor: Uuid, query: &str) -> Result<Vec<SearchHit>, AppError> {
        let query = check_query(query)?;
        let limit = i64::try_from(SEARCH_LIMIT).unwrap_or(i64::MAX);
        let mut hits = Vec::new();
        for source in self.sources.iter() {
            hits.extend(source.search(actor, query, limit).await?);
        }
        let merged = merge(hits);
        let ids: Vec<Uuid> = merged.iter().map(|hit| hit.item_id).collect();
        // The sources filter by access already; loading through `ItemsService` checks again.
        let mut items: HashMap<Uuid, Item> = self
            .items
            .visible_items(actor, &ids)
            .await?
            .into_iter()
            .filter(|item| item.deleted_at.is_none())
            .map(|item| (item.id, item))
            .collect();
        Ok(merged
            .into_iter()
            .filter_map(|hit| {
                items.remove(&hit.item_id).map(|item| SearchHit {
                    item,
                    field: hit.field.into(),
                    snippet: hit.snippet,
                })
            })
            .take(SEARCH_LIMIT)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: u128, rank: f32, field: MatchField) -> SourceHit {
        SourceHit {
            item_id: Uuid::from_u128(id),
            rank,
            field,
            snippet: None,
        }
    }

    #[test]
    fn merge_keeps_each_items_best_hit_best_first() {
        let merged = merge(vec![
            hit(1, 0.2, MatchField::Content),
            hit(2, 0.9, MatchField::Title),
            hit(1, 0.5, MatchField::Tag),
        ]);
        let order: Vec<(u128, MatchField)> = merged
            .iter()
            .map(|h| (h.item_id.as_u128(), h.field))
            .collect();
        assert_eq!(order, [(2, MatchField::Title), (1, MatchField::Tag)]);
    }

    #[test]
    fn queries_are_trimmed_and_bounded() {
        assert_eq!(check_query("  gitar ").ok(), Some("gitar"));
        assert!(check_query("   ").is_err());
        assert!(check_query(&"ğ".repeat(MAX_QUERY_CHARS)).is_ok());
        assert!(check_query(&"ğ".repeat(MAX_QUERY_CHARS + 1)).is_err());
    }
}
