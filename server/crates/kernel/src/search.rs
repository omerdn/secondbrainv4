//! The search source interface (PRD AR1, ARCHITECTURE §12, T2.7.5).
//!
//! Each module searches its own tables and answers with item ids (ADR-0007): `items` for titles,
//! descriptions and the caller's own tags, `notebooks` for page contents. The `search` crate
//! merges the answers, keeps only what the caller can view (AR2) and loads the items. It lives
//! here so that the modules can implement it without depending on `search`.

use std::future::Future;
use std::pin::Pin;

use uuid::Uuid;

use crate::AppError;

/// Where an item matched, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchField {
    Title,
    Tag,
    Description,
    Content,
}

/// One item a source found.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceHit {
    pub item_id: Uuid,
    /// Higher is better; comparable across sources.
    pub rank: f32,
    pub field: MatchField,
    /// A short excerpt around the match, for page contents and tags.
    pub snippet: Option<String>,
}

pub type SearchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<SourceHit>, AppError>> + Send + 'a>>;

/// A module's search over its own tables. `query` is trimmed and non-empty; a source returns
/// only items `actor` can view, at most `limit`.
pub trait SearchSource: Send + Sync {
    fn search<'a>(&'a self, actor: Uuid, query: &'a str, limit: i64) -> SearchFuture<'a>;
}
