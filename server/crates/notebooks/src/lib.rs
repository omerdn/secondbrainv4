//! `notebooks`: Notebooks, pages and the collaborative editing server (ADR-0005, ADR-0011).
//!
//! - Notebooks and pages (T2.5.1, PRD Y1, Y2, Y5): detail rows of `notebook` and `page` items;
//!   items are only reached through [`items::ItemsService`] (ADR-0007).
//! - [`collab`]: `GET /collab/{page_id}`, y-sync over WebSocket on [`yrs`] (T2.5.2).
//! - [`process`]: process pages and entry notes on them (T2.5.5, PRD S1–S3, T3).
//! - [`snapshots`]: compaction, plain text and the version history (T2.5.3, T2.5.4, PRD Y3).

pub mod collab;
pub mod http;
mod model;
pub mod process;
pub mod repo;
mod service;
pub mod snapshots;

pub use model::{
    CreateNotebookRequest, CreatePageRequest, Notebook, NotebookSummary, Page, PageVersion,
    PageVersionContent, VersionsQuery, default_title,
};
pub use service::NotebooksService;
