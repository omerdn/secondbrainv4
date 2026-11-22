//! `items`: the shared base of every object (ARCHITECTURE §5.3, PRD N1–N4): ownership, archive,
//! trash and the access facts `compute_access` needs about items.

mod context;
pub mod http;
mod model;
pub mod purge;
mod service;

pub use context::ItemsAccessContext;
pub use model::{
    CreateItemRequest, DESCRIPTION_MAX_CHARS, Item, ItemChanges, ListQuery, ListState, NewItem,
    Owner, TITLE_MAX_CHARS, UpdateItemRequest, valid_kind,
};
pub use service::{ItemsService, LIST_LIMIT, TRASH_RETENTION};
