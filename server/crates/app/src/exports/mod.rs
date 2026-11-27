//! The personal data export (PRD HS3, T4.3.2). It lives in `app`, the composition root,
//! because it reads through every module's services; it owns only `data_exports`.
//!
//! A user asks for an export; a job reads everything they can see **through the owning
//! modules' services, as that user** ([`build::Sources`]), so the result shows exactly what the
//! API would (V9: no hidden link, member or item). The JSON and Markdown files are stored in
//! `data_exports` and downloaded through a random link that works for 24 hours; expired exports
//! are deleted daily. Nothing is written to disk (PRD §17).

pub mod build;
pub mod http;
mod service;

pub use build::{Export, ExportedItem, Sources, markdown};
pub use service::{
    BuildExport, CLEANUP_CRON, CLEANUP_SCHEDULE, CleanupExports, DataExport, DataExports,
    ExportFile, ExportStatus, LINK_LIFETIME, register,
};
