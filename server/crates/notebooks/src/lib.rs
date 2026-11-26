//! `notebooks`: Notebooks, pages and the collaborative editing server (ADR-0005, ADR-0011).
//!
//! - Notebooks and pages (T2.5.1, PRD Y1, Y2, Y5): detail rows of `notebook` and `page` items;
//!   items are only reached through [`items::ItemsService`] (ADR-0007).
//! - [`collab`]: `GET /collab/{page_id}`, y-sync over WebSocket on [`yrs`] (spike T0.6.2,
//!   development only until E2.5 adds permissions and persistence).

pub mod collab;
pub mod http;
mod model;
pub mod repo;
mod service;
pub mod snapshots;

pub use model::{
    CreateNotebookRequest, CreatePageRequest, Notebook, NotebookSummary, Page, default_title,
};
pub use service::NotebooksService;
