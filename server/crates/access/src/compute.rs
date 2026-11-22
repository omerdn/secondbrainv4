//! `compute_access`: who can do what on an item, derived from the relation graph (ARCHITECTURE
//! §6.3, PRD V2, V3, V5, V6, V12, T1.1.2). Pure: no I/O, no clock.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{ObjectType, Perms, Relation, RelationTuple};

/// A team role, highest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRole {
    Admin,
    Manager,
    Member,
    Guest,
}

impl TeamRole {
    /// The role a team relation stands for; `None` for non-team relations.
    #[must_use]
    pub const fn from_relation(relation: Relation) -> Option<Self> {
        match relation {
            Relation::TeamAdmin => Some(Self::Admin),
            Relation::TeamManager => Some(Self::Manager),
            Relation::TeamMember => Some(Self::Member),
            Relation::TeamGuest => Some(Self::Guest),
            _ => None,
        }
    }

    /// Manager and above always see team items, whatever their visibility mode (V12).
    #[must_use]
    pub const fn is_manager_or_above(self) -> bool {
        matches!(self, Self::Admin | Self::Manager)
    }
}

/// What each role may do on the team's items: the item-level part of the role × permission matrix
/// (TK3). Admins edit it per team (T5.1.2); [`RolePolicy::default`] holds the built-in defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RolePolicy {
    pub admin: Perms,
    pub manager: Perms,
    pub member: Perms,
    pub guest: Perms,
}

impl Default for RolePolicy {
    /// Admins can do everything; deleting is theirs alone (V6). Managers share, link and manage
    /// (V4, V5); members edit; guests view.
    fn default() -> Self {
        Self {
            admin: Perms::ALL,
            manager: Perms::VIEW | Perms::EDIT | Perms::SHARE | Perms::LINK | Perms::MANAGE,
            member: Perms::VIEW | Perms::EDIT,
            guest: Perms::VIEW,
        }
    }
}

impl RolePolicy {
    #[must_use]
    pub const fn perms(&self, role: TeamRole) -> Perms {
        match role {
            TeamRole::Admin => self.admin,
            TeamRole::Manager => self.manager,
            TeamRole::Member => self.member,
            TeamRole::Guest => self.guest,
        }
    }
}

/// Visibility mode of a team item (V12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TeamVisibility {
    /// Every team member, according to their role.
    #[default]
    All,
    /// Only the creator, assignees, watchers and managers and above.
    Assignees,
}

/// Item attributes that affect access but are not relations; owned by the `items` module and
/// passed in by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemAttrs {
    pub team_visibility: TeamVisibility,
    pub created_by: Option<Uuid>,
}

/// Why a user has access ("Why am I seeing this?", V11). Stored in `item_access.reasons`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// Personal owner.
    Owner,
    Assignee,
    Watcher,
    /// Notebook member (on the notebook itself).
    Editor,
    Viewer,
    /// Role in the team that owns the item.
    TeamRole {
        team_id: Uuid,
        role: TeamRole,
    },
    /// Access to the notebook that contains this page.
    Notebook {
        notebook_id: Uuid,
    },
    /// The page is linked to an item the user can view (V3).
    Linked {
        item_id: Uuid,
    },
}

impl Reason {
    /// Whether this reason may grant more than view and edit. Everything inherited (notebook,
    /// link) and every membership role is capped at view + edit (V3, N2).
    #[must_use]
    pub const fn can_exceed_edit(self) -> bool {
        matches!(self, Self::Owner | Self::TeamRole { .. })
    }
}

/// A user's derived access to one item.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Access {
    pub perms: Perms,
    /// Distinct reasons, in the order they were found.
    pub reasons: Vec<Reason>,
}

impl Access {
    fn grant(&mut self, perms: Perms, reason: Reason) {
        self.perms |= perms;
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

/// The part of the relation graph `compute_access` reads, plus item attributes and team policies.
/// Callers load the neighbourhood of the items they recompute (T1.1.3).
#[derive(Debug, Clone, Default)]
pub struct AccessGraph {
    relations: HashMap<(ObjectType, Uuid), Vec<(Relation, Uuid)>>,
    /// Reverse index of the edges whose access flows from subject to object: `linked_to` and
    /// `in_notebook` (item -> dependent pages) and `owned_by` (team -> its items).
    dependents: HashMap<Uuid, Vec<Uuid>>,
    items: HashMap<Uuid, ItemAttrs>,
    policies: HashMap<Uuid, RolePolicy>,
}

impl AccessGraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tuple; adding the same tuple twice has no effect.
    pub fn add(&mut self, tuple: RelationTuple) {
        let edges = self
            .relations
            .entry((tuple.object_type(), tuple.object_id()))
            .or_default();
        let edge = (tuple.relation(), tuple.subject_id());
        if edges.contains(&edge) {
            return;
        }
        edges.push(edge);
        if matches!(
            tuple.relation(),
            Relation::LinkedTo | Relation::InNotebook | Relation::OwnedBy
        ) {
            self.dependents
                .entry(tuple.subject_id())
                .or_default()
                .push(tuple.object_id());
        }
    }

    /// Items without attributes use [`ItemAttrs::default`].
    pub fn set_item(&mut self, item_id: Uuid, attrs: ItemAttrs) {
        self.items.insert(item_id, attrs);
    }

    /// Teams without a policy use [`RolePolicy::default`].
    pub fn set_team_policy(&mut self, team_id: Uuid, policy: RolePolicy) {
        self.policies.insert(team_id, policy);
    }

    pub(crate) fn edges(&self, object_type: ObjectType, id: Uuid) -> &[(Relation, Uuid)] {
        self.relations
            .get(&(object_type, id))
            .map_or(&[], Vec::as_slice)
    }

    /// Items whose access derives from `subject` (an item or a team) through one edge.
    pub(crate) fn dependents(&self, subject: Uuid) -> &[Uuid] {
        self.dependents.get(&subject).map_or(&[], Vec::as_slice)
    }
}

impl FromIterator<RelationTuple> for AccessGraph {
    fn from_iter<I: IntoIterator<Item = RelationTuple>>(iter: I) -> Self {
        let mut graph = Self::new();
        for tuple in iter {
            graph.add(tuple);
        }
        graph
    }
}

/// Inherited access (notebook, link) never goes beyond these (V3).
const INHERITED: Perms = Perms::VIEW.union(Perms::EDIT);

/// Every user with any permission on `item`, with their permissions and reasons (§6.3).
///
/// - Personal owner: everything. Assignee: view + edit. Watcher: view.
/// - Notebook editor / viewer (on the notebook): view + edit / view.
/// - Team item: each role's permissions from the team's [`RolePolicy`]. In
///   [`TeamVisibility::Assignees`] mode only the creator, assignees, watchers and managers and
///   above get their role's permissions (V12).
/// - Page in a notebook: whatever the user may do on the notebook, capped at view + edit.
/// - Page linked to items: view + edit for everyone who can view **any** of them (V2, V3).
///
/// Cycles (a page linked to a page linked back) are cut: an item reached again while it is being
/// computed contributes nothing.
#[must_use]
pub fn compute_access(graph: &AccessGraph, item: Uuid) -> BTreeMap<Uuid, Access> {
    compute(graph, item, &mut HashSet::new())
}

fn compute(
    graph: &AccessGraph,
    item: Uuid,
    visiting: &mut HashSet<Uuid>,
) -> BTreeMap<Uuid, Access> {
    let mut result: BTreeMap<Uuid, Access> = BTreeMap::new();
    if !visiting.insert(item) {
        return result;
    }
    let mut grant = |user: Uuid, perms: Perms, reason: Reason| {
        if !perms.is_empty() {
            result.entry(user).or_default().grant(perms, reason);
        }
    };
    let edges = graph.edges(ObjectType::Item, item);
    for &(relation, subject) in edges {
        match relation {
            Relation::Owner => grant(subject, Perms::ALL, Reason::Owner),
            Relation::Assignee => grant(subject, INHERITED, Reason::Assignee),
            Relation::Watcher => grant(subject, Perms::VIEW, Reason::Watcher),
            Relation::Editor => grant(subject, INHERITED, Reason::Editor),
            Relation::Viewer => grant(subject, Perms::VIEW, Reason::Viewer),
            Relation::OwnedBy => {
                let attrs = graph.items.get(&item).copied().unwrap_or_default();
                let policy = graph.policies.get(&subject).copied().unwrap_or_default();
                let involved = |user: Uuid| {
                    attrs.created_by == Some(user)
                        || edges.iter().any(|&(r, s)| {
                            s == user && matches!(r, Relation::Assignee | Relation::Watcher)
                        })
                };
                for &(team_relation, user) in graph.edges(ObjectType::Team, subject) {
                    let Some(role) = TeamRole::from_relation(team_relation) else {
                        continue;
                    };
                    let visible = attrs.team_visibility == TeamVisibility::All
                        || role.is_manager_or_above()
                        || involved(user);
                    if visible {
                        grant(
                            user,
                            policy.perms(role),
                            Reason::TeamRole {
                                team_id: subject,
                                role,
                            },
                        );
                    }
                }
            }
            Relation::InNotebook => {
                for (user, access) in compute(graph, subject, visiting) {
                    grant(
                        user,
                        access.perms & INHERITED,
                        Reason::Notebook {
                            notebook_id: subject,
                        },
                    );
                }
            }
            Relation::LinkedTo => {
                for (user, access) in compute(graph, subject, visiting) {
                    if access.perms.contains(Perms::VIEW) {
                        grant(user, INHERITED, Reason::Linked { item_id: subject });
                    }
                }
            }
            // Team roles only exist on team objects.
            Relation::TeamAdmin
            | Relation::TeamManager
            | Relation::TeamMember
            | Relation::TeamGuest => {}
        }
    }
    visiting.remove(&item);
    result
}

#[cfg(test)]
mod tests;
