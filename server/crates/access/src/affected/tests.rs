// Test module: panics are the failure mechanism.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use proptest::prelude::*;

use super::*;
use crate::compute::compute_access;
use crate::model::Relation as R;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn t(object: Uuid, relation: R, subject: Uuid) -> RelationTuple {
    RelationTuple::new(object, relation, subject)
}

const ALI: Uuid = Uuid::from_u128(1);
const AYSE: Uuid = Uuid::from_u128(2);
const TASK: Uuid = Uuid::from_u128(100);
const PAGE: Uuid = Uuid::from_u128(101);
const PAGE2: Uuid = Uuid::from_u128(102);
const NOTEBOOK: Uuid = Uuid::from_u128(103);
const OTHER: Uuid = Uuid::from_u128(104);
const TEAM: Uuid = Uuid::from_u128(200);

fn graph() -> AccessGraph {
    [
        t(TASK, R::Owner, ALI),
        t(PAGE, R::LinkedTo, TASK),
        t(PAGE, R::InNotebook, NOTEBOOK),
        t(PAGE2, R::LinkedTo, PAGE), // a page linked to a page
        t(OTHER, R::OwnedBy, TEAM),
        t(TEAM, R::TeamMember, AYSE),
    ]
    .into_iter()
    .collect()
}

fn set(ids: &[Uuid]) -> BTreeSet<Uuid> {
    ids.iter().copied().collect()
}

#[test]
fn member_change_affects_the_item_and_its_linked_pages() {
    let g = graph();
    let change = Change::Relation(t(TASK, R::Assignee, AYSE));
    assert_eq!(affected_items(&g, &change), set(&[TASK, PAGE, PAGE2]));
    assert_eq!(changed_team(&change), None);
}

#[test]
fn link_change_affects_the_page() {
    let g = graph();
    let change = Change::Relation(t(PAGE2, R::LinkedTo, PAGE));
    assert_eq!(affected_items(&g, &change), set(&[PAGE2]));
}

#[test]
fn notebook_membership_change_affects_its_pages() {
    let g = graph();
    let change = Change::Relation(t(NOTEBOOK, R::Viewer, AYSE));
    assert_eq!(affected_items(&g, &change), set(&[NOTEBOOK, PAGE, PAGE2]));
}

#[test]
fn team_changes_affect_every_team_item() {
    let g = graph();
    let role = Change::Relation(t(TEAM, R::TeamManager, AYSE));
    assert_eq!(affected_items(&g, &role), set(&[OTHER]));
    assert_eq!(changed_team(&role), Some(TEAM));
    let policy = Change::Team(TEAM);
    assert_eq!(affected_items(&g, &policy), set(&[OTHER]));
    assert_eq!(changed_team(&policy), Some(TEAM));
    assert!(affected_items(&g, &Change::Team(id(999))).is_empty());
}

#[test]
fn visibility_mode_change_affects_the_item() {
    let g = graph();
    let change = Change::Item(OTHER);
    assert_eq!(affected_items(&g, &change), set(&[OTHER]));
    assert_eq!(changed_team(&change), None);
}

#[test]
fn cycles_terminate() {
    let g: AccessGraph = [t(PAGE, R::LinkedTo, PAGE2), t(PAGE2, R::LinkedTo, PAGE)]
        .into_iter()
        .collect();
    assert_eq!(affected_items(&g, &Change::Item(PAGE)), set(&[PAGE, PAGE2]));
}

#[test]
fn compute_inputs_follow_links_notebooks_and_owning_teams() {
    let g = graph();
    let inputs = compute_inputs(&g, [PAGE2, OTHER]);
    let expected: HashSet<(ObjectType, Uuid)> = [
        (ObjectType::Item, PAGE2),
        (ObjectType::Item, PAGE),
        (ObjectType::Item, TASK),
        (ObjectType::Item, NOTEBOOK),
        (ObjectType::Item, OTHER),
        (ObjectType::Team, TEAM),
    ]
    .into_iter()
    .collect();
    assert_eq!(inputs, expected);
}

const USERS: u128 = 4;
const ITEMS: u128 = 6;
const TEAMS: u128 = 2;

fn tuple_strategy() -> impl Strategy<Value = RelationTuple> {
    (0..R::ALL.len(), 0..ITEMS, 0..ITEMS).prop_map(|(r, o, s)| {
        let relation = R::ALL[r];
        let object = match relation.object_type() {
            ObjectType::Item => id(100 + o),
            ObjectType::Team => id(200 + o % TEAMS),
        };
        let subject = match relation.subject_type() {
            crate::SubjectType::User => id(1 + s % USERS),
            crate::SubjectType::Team => id(200 + s % TEAMS),
            crate::SubjectType::Item => id(100 + s),
        };
        RelationTuple::new(object, relation, subject)
    })
}

proptest! {
    /// Soundness: adding or removing any tuple changes `compute_access` only for items in the
    /// affected set.
    #[test]
    fn unaffected_items_keep_their_access(
        tuples in proptest::collection::vec(tuple_strategy(), 0..30),
        change in tuple_strategy(),
    ) {
        let before: AccessGraph = tuples.iter().copied().collect();
        let with: AccessGraph = tuples.iter().copied().chain([change]).collect();
        let without: AccessGraph = tuples.iter().copied().filter(|x| *x != change).collect();
        for (old, new) in [(&before, &with), (&before, &without)] {
            // As in the database: the affected set is computed after the change is applied.
            let affected = affected_items(new, &Change::Relation(change));
            for n in 0..ITEMS {
                let item = id(100 + n);
                if !affected.contains(&item) {
                    prop_assert_eq!(compute_access(old, item), compute_access(new, item), "item {}", n);
                }
            }
        }
    }
}
