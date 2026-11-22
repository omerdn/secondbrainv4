//! `notebooks`: Notebooks, pages and the collaborative editing server (ADR-0005, ADR-0011).
//!
//! - [`collab`]: `GET /collab/{page_id}`, y-sync over WebSocket on [`yrs`] (spike T0.6.2,
//!   development only until E2.5 adds permissions and persistence).

pub mod collab;
