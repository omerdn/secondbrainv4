//! `teams`: teams, membership and (from T5.1.2) the role × permission matrix (PRD TK1–TK7,
//! ARCHITECTURE §5.2).
//!
//! - The role × permission matrix (TK3, T5.1.2): see [`TeamMatrix`]; admins edit it and change
//!   roles, and access is recomputed at once (V8).
//! - [`TeamsService`]: create a team (the creator is its admin), invite people with a role,
//!   accept or decline (TK2). An invitation is a `team_members` row with status `invited`; only
//!   an active member holds the role, which is also written as an `access` relation
//!   (team → user) so team items can be computed (T5.1.3).
//! - Teammates may share personal items with each other without being friends (V1):
//!   [`TeamsService`] is an [`items::SharingPolicy`].
//! - A new invitation publishes [`TeamInvitationCreated`]; the notification center tells the
//!   invited person (TK2).

pub mod http;
mod matrix;
mod model;
mod service;

pub use matrix::{ItemPermission, RoleRights, TeamMatrix, TeamPermission};
pub use model::{
    CreateTeamRequest, InviteToTeamRequest, NAME_MAX_CHARS, SetRoleRequest, Team, TeamDetail,
    TeamInvitation, TeamInvitationCreated, TeamMember, TeamRole,
};
pub use service::TeamsService;
