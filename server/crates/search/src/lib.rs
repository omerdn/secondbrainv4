//! `search`: Global search (PRD AR1, AR2, ARCHITECTURE §12, T2.7.5).
//!
//! [`SearchService`] asks every registered [`kernel::search::SearchSource`] (titles,
//! descriptions and the caller's own tags from `items`, page contents from `notebooks`), merges
//! their answers by item and loads the items through `ItemsService`, which again keeps only
//! what the caller can view (AR2). Search stays behind this interface, so another index
//! (Meilisearch) can replace the sources later.

pub mod http;
mod service;

pub use service::{
    MAX_QUERY_CHARS, SEARCH_LIMIT, SearchField, SearchHit, SearchQuery, SearchService,
};
