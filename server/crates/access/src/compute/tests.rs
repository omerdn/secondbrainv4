// Test module: panics are the failure mechanism.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use proptest::prelude::*;

use super::*;
use crate::model::{Relation as R, SubjectType};

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn t(object: Uuid, relation: R, subject: Uuid) -> RelationTuple {
    RelationTuple::new(object, relation, subject)
}

fn perms(access: &BTreeMap<Uuid, Access>, user: Uuid) -> Perms {
    access.get(&user).map_or(Perms::NONE, |a| a.perms)
}

const VE: Perms = Perms::VIEW.union(Perms::EDIT);
const ABOVE_EDIT: Perms = Perms::SHARE
    .union(Perms::LINK)
    .union(Perms::MANAGE)
    .union(Perms::DELETE);

// Users, items and teams used by the scenarios.
const ALI: Uuid = Uuid::from_u128(1);
const AYSE: Uuid = Uuid::from_u128(2);
const CAN: Uuid = Uuid::from_u128(3);
const DENIZ: Uuid = Uuid::from_u128(4);
const MEHMET: Uuid = Uuid::from_u128(5);
const TASK: Uuid = Uuid::from_u128(100);
const TASK2: Uuid = Uuid::from_u128(101);
const PAGE: Uuid = Uuid::from_u128(102);
const NOTEBOOK: Uuid = Uuid::from_u128(103);
const TEAM: Uuid = Uuid::from_u128(200);

// --- personal items: ownership and membership (N2, V5, V6) -----------------------------------

#[test]
fn personal_owner_assignee_and_watcher() {
    let g: AccessGraph = [
        t(TASK, R::Owner, ALI),
        t(TASK, R::Assignee, AYSE),
        t(TASK, R::Watcher, CAN),
    ]
    .into_iter()
    .collect();
    let a = compute_access(&g, TASK);
    assert_eq!(perms(&a, ALI), Perms::ALL);
    assert_eq!(perms(&a, AYSE), VE);
    assert_eq!(perms(&a, CAN), Perms::VIEW);
    assert_eq!(perms(&a, DENIZ), Perms::NONE);
    assert_eq!(a.len(), 3);
    assert_eq!(a[&ALI].reasons, [Reason::Owner]);
}

#[test]
fn only_the_owner_deletes_and_adds_members() {
    // V5, V6: SHARE and DELETE belong to the personal owner only, never to members.
    let g: AccessGraph = [
        t(TASK, R::Owner, ALI),
        t(TASK, R::Assignee, AYSE),
        t(TASK, R::Watcher, AYSE),
        t(TASK, R::Watcher, CAN),
    ]
    .into_iter()
    .collect();
    let a = compute_access(&g, TASK);
    for (user, access) in &a {
        let owner = *user == ALI;
        assert_eq!(access.perms.contains(Perms::DELETE), owner);
        assert_eq!(access.perms.contains(Perms::SHARE), owner);
        assert_eq!(access.perms.contains(Perms::MANAGE), owner);
    }
    assert_eq!(a[&AYSE].reasons, [Reason::Assignee, Reason::Watcher]);
}

#[test]
fn unknown_item_has_no_access() {
    assert!(compute_access(&AccessGraph::new(), TASK).is_empty());
}

// --- pages: notebooks and links (V2, V3) ------------------------------------------------------

#[test]
fn page_viewers_are_notebook_members_and_viewers_of_every_linked_item() {
    // V2: PAGE is in Ali's NOTEBOOK (Mehmet is a viewer) and linked to TASK (Ayşe) and TASK2 (Can).
    let g: AccessGraph = [
        t(NOTEBOOK, R::Owner, ALI),
        t(NOTEBOOK, R::Viewer, MEHMET),
        t(PAGE, R::Owner, ALI),
        t(PAGE, R::InNotebook, NOTEBOOK),
        t(PAGE, R::LinkedTo, TASK),
        t(PAGE, R::LinkedTo, TASK2),
        t(TASK, R::Owner, ALI),
        t(TASK, R::Assignee, AYSE),
        t(TASK2, R::Owner, CAN),
    ]
    .into_iter()
    .collect();
    let a = compute_access(&g, PAGE);
    let viewers: Vec<Uuid> = a.keys().copied().collect();
    assert_eq!(viewers, [ALI, AYSE, CAN, MEHMET]);
    assert_eq!(perms(&a, DENIZ), Perms::NONE);
    // V3: through a link, view + edit only, even for the linked item's owner.
    assert_eq!(perms(&a, AYSE), VE);
    assert_eq!(perms(&a, CAN), VE);
    assert_eq!(a[&CAN].reasons, [Reason::Linked { item_id: TASK2 }]);
    // Notebook viewer: view only. Page owner keeps everything.
    assert_eq!(perms(&a, MEHMET), Perms::VIEW);
    assert_eq!(
        a[&MEHMET].reasons,
        [Reason::Notebook {
            notebook_id: NOTEBOOK
        }]
    );
    assert_eq!(perms(&a, ALI), Perms::ALL);
    assert_eq!(
        a[&ALI].reasons,
        [
            Reason::Owner,
            Reason::Notebook {
                notebook_id: NOTEBOOK
            },
            Reason::Linked { item_id: TASK },
        ]
    );
}

#[test]
fn notebook_access_is_capped_at_view_and_edit_on_pages() {
    let g: AccessGraph = [
        t(NOTEBOOK, R::Owner, AYSE),
        t(NOTEBOOK, R::Editor, CAN),
        t(PAGE, R::Owner, ALI),
        t(PAGE, R::InNotebook, NOTEBOOK),
    ]
    .into_iter()
    .collect();
    let nb = compute_access(&g, NOTEBOOK);
    assert_eq!(perms(&nb, CAN), VE);
    assert_eq!(nb[&CAN].reasons, [Reason::Editor]);
    let a = compute_access(&g, PAGE);
    // The notebook's owner may edit the page but not delete or share it.
    assert_eq!(perms(&a, AYSE), VE);
    assert_eq!(perms(&a, CAN), VE);
    assert_eq!(perms(&a, ALI), Perms::ALL);
}

#[test]
fn removing_a_link_drops_inherited_access() {
    // V8 at the function level: without the tuple, the access is gone.
    let mut g: AccessGraph = [t(PAGE, R::Owner, ALI), t(TASK, R::Owner, AYSE)]
        .into_iter()
        .collect();
    assert_eq!(perms(&compute_access(&g, PAGE), AYSE), Perms::NONE);
    g.add(t(PAGE, R::LinkedTo, TASK));
    g.add(t(PAGE, R::LinkedTo, TASK)); // duplicates are ignored
    assert_eq!(perms(&compute_access(&g, PAGE), AYSE), VE);
}

#[test]
fn link_cycles_terminate_and_diamonds_resolve() {
    let (p1, p2, p3) = (id(301), id(302), id(303));
    let g: AccessGraph = [
        t(p1, R::Owner, ALI),
        t(p2, R::Owner, AYSE),
        t(p3, R::Owner, CAN),
        t(p1, R::LinkedTo, p2),
        t(p2, R::LinkedTo, p1),
        // diamond: p3 -> p1 and p3 -> p2
        t(p3, R::LinkedTo, p1),
        t(p3, R::LinkedTo, p2),
    ]
    .into_iter()
    .collect();
    let a1 = compute_access(&g, p1);
    assert_eq!(perms(&a1, ALI), Perms::ALL);
    assert_eq!(perms(&a1, AYSE), VE);
    let a3 = compute_access(&g, p3);
    assert_eq!(perms(&a3, ALI), VE);
    assert_eq!(perms(&a3, AYSE), VE);
    assert_eq!(perms(&a3, CAN), Perms::ALL);
}

// --- team items (TK3, V12) --------------------------------------------------------------------

fn team_graph(visibility: TeamVisibility) -> AccessGraph {
    let mut g: AccessGraph = [
        t(TEAM, R::TeamAdmin, ALI),
        t(TEAM, R::TeamManager, MEHMET),
        t(TEAM, R::TeamMember, AYSE),
        t(TEAM, R::TeamMember, CAN),
        t(TEAM, R::TeamGuest, DENIZ),
        t(TASK, R::OwnedBy, TEAM),
        t(TASK, R::Assignee, CAN),
    ]
    .into_iter()
    .collect();
    g.set_item(
        TASK,
        ItemAttrs {
            team_visibility: visibility,
            created_by: Some(MEHMET),
        },
    );
    g
}

#[test]
fn team_items_follow_the_default_role_matrix() {
    let a = compute_access(&team_graph(TeamVisibility::All), TASK);
    let policy = RolePolicy::default();
    assert_eq!(perms(&a, ALI), Perms::ALL);
    assert_eq!(perms(&a, MEHMET), policy.manager);
    assert_eq!(perms(&a, AYSE), VE);
    assert_eq!(perms(&a, DENIZ), Perms::VIEW);
    // Can is a member and an assignee.
    assert_eq!(perms(&a, CAN), VE);
    assert_eq!(
        a[&CAN].reasons,
        [
            Reason::TeamRole {
                team_id: TEAM,
                role: TeamRole::Member
            },
            Reason::Assignee,
        ]
    );
    // V6: only the admin role deletes by default; V5: admins and managers add members.
    let deleters: Vec<Uuid> = a
        .iter()
        .filter(|(_, x)| x.perms.contains(Perms::DELETE))
        .map(|(u, _)| *u)
        .collect();
    assert_eq!(deleters, [ALI]);
    let sharers: Vec<Uuid> = a
        .iter()
        .filter(|(_, x)| x.perms.contains(Perms::SHARE))
        .map(|(u, _)| *u)
        .collect();
    assert_eq!(sharers, [ALI, MEHMET]);
}

#[test]
fn assignees_only_mode_limits_viewers() {
    // V12: creator (Mehmet), assignee (Can), managers and above (Ali, Mehmet) see it; Ayşe and
    // Deniz do not.
    let a = compute_access(&team_graph(TeamVisibility::Assignees), TASK);
    let viewers: Vec<Uuid> = a.keys().copied().collect();
    assert_eq!(viewers, [ALI, CAN, MEHMET]);

    // A member who created the item or watches it sees it too.
    let mut g = team_graph(TeamVisibility::Assignees);
    g.set_item(
        TASK,
        ItemAttrs {
            team_visibility: TeamVisibility::Assignees,
            created_by: Some(AYSE),
        },
    );
    g.add(t(TASK, R::Watcher, DENIZ));
    let a = compute_access(&g, TASK);
    assert_eq!(perms(&a, AYSE), VE);
    assert_eq!(perms(&a, DENIZ), Perms::VIEW);
}

#[test]
fn team_policy_overrides_defaults() {
    let mut g = team_graph(TeamVisibility::All);
    g.set_team_policy(
        TEAM,
        RolePolicy {
            member: Perms::VIEW | Perms::EDIT | Perms::SHARE,
            guest: Perms::NONE,
            ..RolePolicy::default()
        },
    );
    let a = compute_access(&g, TASK);
    assert_eq!(perms(&a, AYSE), VE | Perms::SHARE);
    assert_eq!(
        perms(&a, DENIZ),
        Perms::NONE,
        "a role with no permissions gets no row"
    );
}

#[test]
fn team_role_helpers() {
    assert_eq!(TeamRole::from_relation(R::TeamGuest), Some(TeamRole::Guest));
    assert_eq!(TeamRole::from_relation(R::Owner), None);
    assert!(TeamRole::Admin.is_manager_or_above());
    assert!(!TeamRole::Member.is_manager_or_above());
    assert!(!TeamRole::Guest.is_manager_or_above());
}

#[test]
fn reasons_serialize_for_item_access() {
    let json = serde_json::to_value([
        Reason::Owner,
        Reason::TeamRole {
            team_id: TEAM,
            role: TeamRole::Manager,
        },
        Reason::Linked { item_id: TASK },
    ])
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            { "kind": "owner" },
            { "kind": "team_role", "team_id": TEAM, "role": "manager" },
            { "kind": "linked", "item_id": TASK },
        ])
    );
    let back: Vec<Reason> = serde_json::from_value(json).unwrap();
    assert_eq!(back.len(), 3);
}

// --- properties over random graphs ------------------------------------------------------------

const USERS: u128 = 5;
const ITEMS: u128 = 6;
const TEAMS: u128 = 2;

fn user(n: u128) -> Uuid {
    id(1 + n)
}
fn item(n: u128) -> Uuid {
    id(100 + n)
}
fn team(n: u128) -> Uuid {
    id(200 + n)
}

fn tuple_strategy() -> impl Strategy<Value = RelationTuple> {
    (0..R::ALL.len(), 0..ITEMS.max(TEAMS), 0..USERS.max(ITEMS)).prop_map(|(r, o, s)| {
        let relation = R::ALL[r];
        let object = match relation.object_type() {
            ObjectType::Item => item(o % ITEMS),
            ObjectType::Team => team(o % TEAMS),
        };
        let subject = match relation.subject_type() {
            SubjectType::User => user(s % USERS),
            SubjectType::Team => team(s % TEAMS),
            SubjectType::Item => item(s % ITEMS),
        };
        RelationTuple::new(object, relation, subject)
    })
}

fn graph_strategy() -> impl Strategy<Value = AccessGraph> {
    (
        proptest::collection::vec(tuple_strategy(), 0..40),
        proptest::collection::vec(
            (any::<bool>(), proptest::option::of(0..USERS)),
            ITEMS as usize,
        ),
    )
        .prop_map(|(tuples, attrs)| {
            let mut g: AccessGraph = tuples.into_iter().collect();
            for (n, (assignees_only, creator)) in (0..).zip(attrs) {
                g.set_item(
                    item(n),
                    ItemAttrs {
                        team_visibility: if assignees_only {
                            TeamVisibility::Assignees
                        } else {
                            TeamVisibility::All
                        },
                        created_by: creator.map(user),
                    },
                );
            }
            g
        })
}

proptest! {
    /// V3: only ownership (personal owner or the owning team's role matrix) can grant more than
    /// view + edit. Links, notebooks and membership roles never grant share, link, manage or delete.
    #[test]
    fn inherited_access_never_exceeds_view_and_edit(g in graph_strategy()) {
        for n in 0..ITEMS {
            for (u, access) in compute_access(&g, item(n)) {
                prop_assert!(!access.perms.is_empty());
                prop_assert!(!access.reasons.is_empty());
                if !access.reasons.iter().any(|r| r.can_exceed_edit()) {
                    prop_assert_eq!(access.perms & ABOVE_EDIT, Perms::NONE, "user {} on item {}", u, n);
                }
            }
        }
    }

    /// V2: a page's viewers include its notebook's users and everyone who can view any item it is
    /// linked to; every linked/notebook reason is backed by that access.
    #[test]
    fn page_viewers_cover_notebook_and_linked_viewers(g in graph_strategy()) {
        for n in 0..ITEMS {
            let page = item(n);
            let access = compute_access(&g, page);
            for &(relation, target) in g.edges(ObjectType::Item, page) {
                if target == page || !matches!(relation, R::LinkedTo | R::InNotebook) {
                    continue;
                }
                // Compute the target as `compute` sees it: with the page on the path.
                let mut visiting = HashSet::from([page]);
                let target_access = compute(&g, target, &mut visiting);
                for (u, ta) in &target_access {
                    let expected = if relation == R::LinkedTo {
                        if ta.perms.contains(Perms::VIEW) { VE } else { Perms::NONE }
                    } else {
                        ta.perms & VE
                    };
                    prop_assert!(perms(&access, *u).contains(expected));
                }
            }
            for (u, a) in &access {
                for reason in &a.reasons {
                    if let Reason::Linked { item_id } = reason {
                        let mut visiting = HashSet::from([page]);
                        prop_assert!(perms(&compute(&g, *item_id, &mut visiting), *u).contains(Perms::VIEW));
                    }
                }
            }
        }
    }

    /// V6 (personal items): DELETE comes only from the personal owner or a team role.
    #[test]
    fn delete_requires_ownership(g in graph_strategy()) {
        for n in 0..ITEMS {
            let owners: Vec<Uuid> = g
                .edges(ObjectType::Item, item(n))
                .iter()
                .filter(|(r, _)| *r == R::Owner)
                .map(|(_, u)| *u)
                .collect();
            for (u, a) in compute_access(&g, item(n)) {
                if a.perms.contains(Perms::DELETE) {
                    let team_reason = a.reasons.iter().any(|r| matches!(r, Reason::TeamRole { .. }));
                    prop_assert!(owners.contains(&u) || team_reason);
                }
            }
        }
    }

    /// The result does not depend on the order tuples were added in.
    #[test]
    fn order_independent(tuples in proptest::collection::vec(tuple_strategy(), 0..30)) {
        let forward: AccessGraph = tuples.iter().copied().collect();
        let backward: AccessGraph = tuples.iter().rev().copied().collect();
        for n in 0..ITEMS {
            let f: BTreeMap<Uuid, Perms> = compute_access(&forward, item(n)).into_iter().map(|(u, a)| (u, a.perms)).collect();
            let b: BTreeMap<Uuid, Perms> = compute_access(&backward, item(n)).into_iter().map(|(u, a)| (u, a.perms)).collect();
            prop_assert_eq!(f, b);
        }
    }
}
