//! `items`: the shared base of every object (ARCHITECTURE §5.3, PRD N1–N4): ownership, archive,
//! trash and the access facts `compute_access` needs about items.

mod activity;
mod context;
mod filters;
pub mod http;
mod invitations;
mod links;
mod model;
pub mod purge;
mod service;
mod tags;
mod touch;
mod touched;
mod views;

pub use activity::{Activity, ActivityKind};
pub use context::ItemsAccessContext;
pub use filters::{DateField, DateFilter, FILTER_MAX_PEOPLE, FILTER_MAX_VALUES, ItemFilter};
pub use invitations::{
    CreateInvitationRequest, INVITE_LIMIT, INVITE_WINDOW, Invitation, InvitationCreated,
    InvitationRole, InvitationStatus, NoSharing, SharingFuture, SharingPolicy, role_allowed,
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
pub use touch::{TouchAction, touched_at};
pub use touched::Touched;
pub use views::{
    CreateSavedViewRequest, SavedView, UpdateSavedViewRequest, VIEW_LIMIT, VIEW_NAME_MAX_CHARS,
};
