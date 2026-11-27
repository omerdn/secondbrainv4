//! The role × permission matrix (PRD TK3, V5, V8, T5.1.2).
//!
//! Two halves, both edited by the team's admins:
//! - **Item permissions** of each role on the team's items (view, edit, share, link, manage,
//!   delete): an input of the access computation, stored by `access` (`team_role_policies`) so a
//!   change recomputes access at once (V8).
//! - **Team permissions** of each role (create items, assign, invite members, manage team tags,
//!   create team notebooks, see the activity log), stored here (`team_permissions`).
//!
//! Admins always have everything: the matrix never locks a team out of itself.

use access::{Perms, RolePolicy};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::TeamRole;

/// What a role may do on the team's items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ItemPermission {
    View,
    Edit,
    Share,
    Link,
    Manage,
    Delete,
}

impl ItemPermission {
    pub const ALL: [Self; 6] = [
        Self::View,
        Self::Edit,
        Self::Share,
        Self::Link,
        Self::Manage,
        Self::Delete,
    ];

    const fn perms(self) -> Perms {
        match self {
            Self::View => Perms::VIEW,
            Self::Edit => Perms::EDIT,
            Self::Share => Perms::SHARE,
            Self::Link => Perms::LINK,
            Self::Manage => Perms::MANAGE,
            Self::Delete => Perms::DELETE,
        }
    }
}

/// What a role may do in the team itself (TK3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TeamPermission {
    CreateItems,
    Assign,
    InviteMembers,
    ManageTags,
    CreateNotebooks,
    ViewActivity,
}

impl TeamPermission {
    pub const ALL: [Self; 6] = [
        Self::CreateItems,
        Self::Assign,
        Self::InviteMembers,
        Self::ManageTags,
        Self::CreateNotebooks,
        Self::ViewActivity,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateItems => "create_items",
            Self::Assign => "assign",
            Self::InviteMembers => "invite_members",
            Self::ManageTags => "manage_tags",
            Self::CreateNotebooks => "create_notebooks",
            Self::ViewActivity => "view_activity",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == value)
    }

    /// The built-in matrix (TK3): managers run the team, members work in it, guests look.
    #[must_use]
    pub fn defaults(role: TeamRole) -> Vec<Self> {
        match role {
            TeamRole::Admin | TeamRole::Manager => Self::ALL.to_vec(),
            TeamRole::Member => vec![Self::CreateItems, Self::CreateNotebooks, Self::ViewActivity],
            TeamRole::Guest => Vec::new(),
        }
    }
}

/// One role's row of the matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RoleRights {
    pub role: TeamRole,
    pub item: Vec<ItemPermission>,
    pub team: Vec<TeamPermission>,
}

/// `GET`/`PUT /teams/{id}/matrix`: every role's rights, strongest role first. On `PUT` the admin
/// row is ignored (admins keep everything).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct TeamMatrix {
    pub roles: Vec<RoleRights>,
}

/// The item permissions in `perms`.
#[must_use]
pub(crate) fn item_permissions(perms: Perms) -> Vec<ItemPermission> {
    ItemPermission::ALL
        .into_iter()
        .filter(|p| perms.contains(p.perms()))
        .collect()
}

/// `Perms` of a list of item permissions; anything at all includes VIEW (one cannot edit what
/// one cannot see).
#[must_use]
pub(crate) fn perms_of(list: &[ItemPermission]) -> Perms {
    let perms = list.iter().fold(Perms::NONE, |acc, p| acc | p.perms());
    if perms == Perms::NONE {
        perms
    } else {
        perms | Perms::VIEW
    }
}

/// The access policy a matrix asks for; admins always get [`Perms::ALL`].
#[must_use]
pub(crate) fn policy_of(matrix: &TeamMatrix, current: RolePolicy) -> RolePolicy {
    let pick = |role: TeamRole, fallback: Perms| {
        matrix
            .roles
            .iter()
            .find(|r| r.role == role)
            .map_or(fallback, |r| perms_of(&r.item))
    };
    RolePolicy {
        admin: Perms::ALL,
        manager: pick(TeamRole::Manager, current.manager),
        member: pick(TeamRole::Member, current.member),
        guest: pick(TeamRole::Guest, current.guest),
    }
}

/// The team permissions a matrix gives `role`, sorted and without repeats; admins always have
/// all.
#[must_use]
pub(crate) fn team_rights(matrix: &TeamMatrix, role: TeamRole) -> Option<Vec<TeamPermission>> {
    if role == TeamRole::Admin {
        return Some(TeamPermission::ALL.to_vec());
    }
    matrix.roles.iter().find(|r| r.role == role).map(|r| {
        let mut list = r.team.clone();
        list.sort_unstable();
        list.dedup();
        list
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tk3_any_item_right_includes_view_and_admins_keep_everything() {
        assert_eq!(perms_of(&[]), Perms::NONE);
        assert_eq!(perms_of(&[ItemPermission::Edit]), Perms::VIEW | Perms::EDIT);
        let matrix = TeamMatrix {
            roles: vec![
                RoleRights {
                    role: TeamRole::Admin,
                    item: vec![],
                    team: vec![],
                },
                RoleRights {
                    role: TeamRole::Manager,
                    item: vec![ItemPermission::View],
                    team: vec![TeamPermission::Assign, TeamPermission::Assign],
                },
            ],
        };
        let policy = policy_of(&matrix, RolePolicy::default());
        assert_eq!(policy.admin, Perms::ALL);
        assert_eq!(policy.manager, Perms::VIEW);
        assert_eq!(policy.member, RolePolicy::default().member, "untouched");
        assert_eq!(
            team_rights(&matrix, TeamRole::Admin).unwrap().len(),
            TeamPermission::ALL.len()
        );
        assert_eq!(
            team_rights(&matrix, TeamRole::Manager).unwrap(),
            [TeamPermission::Assign]
        );
        assert_eq!(team_rights(&matrix, TeamRole::Guest), None);
    }

    #[test]
    fn permissions_round_trip() {
        for p in TeamPermission::ALL {
            assert_eq!(TeamPermission::parse(p.as_str()), Some(p));
        }
        assert_eq!(
            item_permissions(Perms::VIEW | Perms::SHARE),
            [ItemPermission::View, ItemPermission::Share]
        );
        assert!(TeamPermission::defaults(TeamRole::Guest).is_empty());
        assert!(
            TeamPermission::defaults(TeamRole::Manager).contains(&TeamPermission::InviteMembers)
        );
        assert!(
            !TeamPermission::defaults(TeamRole::Member).contains(&TeamPermission::InviteMembers)
        );
    }
}
