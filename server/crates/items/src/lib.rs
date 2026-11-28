//! `items`: the shared base of every object (ARCHITECTURE §5.3, PRD N1–N4): ownership, archive,
//! trash and the access facts `compute_access` needs about items.

mod activity;
mod context;
pub mod deletion;
mod filters;
pub mod http;
mod invitations;
mod links;
mod members;
mod model;
pub mod purge;
mod reactions;
mod search;
mod service;
mod tags;
mod team_items;
mod touch;
mod touched;
mod views;

pub use activity::{Activity, ActivityKind};
pub use context::ItemsAccessContext;
pub use deletion::{OwnedSharedItem, OwnershipTransferred, TransferOwnershipRequest};
pub use filters::{
    DateField, DateFilter, FILTER_MAX_LAST_DAYS, FILTER_MAX_PEOPLE, FILTER_MAX_VALUES, ItemFilter,
};
pub use invitations::{
    AnyOf, CreateInvitationRequest, INVITE_LIMIT, INVITE_WINDOW, Invitation, InvitationCreated,
    InvitationRole, InvitationStatus, NoSharing, SharingFuture, SharingPolicy, role_allowed,
};
pub use links::{ItemLink, LinkDirection, link_allowed, register_link_cleanup};
pub use members::{ItemMember, ItemMembers, MemberRemoved, MemberRole, PendingMember};
pub use model::{
    CreateItemRequest, DESCRIPTION_MAX_CHARS, Item, ItemChanges, ListQuery, ListState, NewItem,
    Owner, TITLE_MAX_CHARS, UpdateItemRequest, valid_kind,
};
pub use reactions::{REACTIONS, ReactRequest, ReactionAdded, ReactionCount, tally};
pub use service::{ItemsService, LIST_LIMIT, TRASH_RETENTION};
pub use tags::{
    CreateTagRequest, NewTag, TAG_LIST_LIMIT, TAG_NAME_MAX_CHARS, Tag, TagChanges, UpdateTagRequest,
};
pub use team_items::{
    CreateTeamItemRequest, NewTeamItem, NoTeams, SetTeamVisibilityRequest, TEAM_ITEM_MAX_PEOPLE,
    TeamAction, TeamDirectory, TeamFuture, TeamVisibilityMode, TeamsFuture,
};
pub use touch::{TouchAction, touched_at};
pub use touched::Touched;
pub use views::{
    CreateSavedViewRequest, SavedView, UpdateSavedViewRequest, VIEW_LIMIT, VIEW_NAME_MAX_CHARS,
};
