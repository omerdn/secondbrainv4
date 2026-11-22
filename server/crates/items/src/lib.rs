//! `items`: the shared base of every object (ARCHITECTURE §5.3, PRD N1–N4): ownership, archive,
//! trash and the access facts `compute_access` needs about items.

mod context;
pub mod http;
mod invitations;
mod links;
mod model;
pub mod purge;
mod service;
mod tags;

pub use context::ItemsAccessContext;
pub use invitations::{
    CreateInvitationRequest, Invitation, InvitationCreated, InvitationRole, InvitationStatus,
    NoSharing, SharingFuture, SharingPolicy, role_allowed,
};
pub use links::{ItemLink, LinkDirection, link_allowed, register_link_cleanup};
pub use model::{
    CreateItemRequest, DESCRIPTION_MAX_CHARS, Item, ItemChanges, ListQuery, ListState, NewItem,
    Owner, TITLE_MAX_CHARS, UpdateItemRequest, valid_kind,
};
pub use service::{ItemsService, LIST_LIMIT, TRASH_RETENTION};
pub use tags::{
    CreateTagRequest, NewTag, TAG_LIST_LIMIT, TAG_NAME_MAX_CHARS, Tag, TagChanges, UpdateTagRequest,
};
