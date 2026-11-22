//! Which items need recomputing after a change (ARCHITECTURE §6.4, PRD V8, T1.1.3). Pure.

use std::collections::{BTreeSet, HashSet, VecDeque};

use uuid::Uuid;

use crate::compute::AccessGraph;
use crate::model::{ObjectType, Relation, RelationTuple};

/// Something that may change who can access what (V8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A relation was added or removed: link/unlink, member add/remove, notebook membership,
    /// team role, item ownership.
    Relation(RelationTuple),
    /// An item's [`crate::ItemAttrs`] changed (team visibility mode, V12).
    Item(Uuid),
    /// A team's role matrix changed (TK3).
    Team(Uuid),
}

/// Items whose access may differ after `change`:
/// - the changed item itself, or every item owned by the changed team;
/// - transitively, every page whose access derives from an affected item: pages linked to it and,
///   if it is a notebook, the pages in it.
///
/// `graph` must hold the reverse edges (`linked_to`, `in_notebook`, `owned_by`) of every item and
/// team reached; the result is sorted.
#[must_use]
pub fn affected_items(graph: &AccessGraph, change: &Change) -> BTreeSet<Uuid> {
    let seeds: Vec<Uuid> = match *change {
        Change::Relation(tuple) if tuple.object_type() == ObjectType::Team => {
            graph.dependents(tuple.object_id()).to_vec()
        }
        Change::Relation(tuple) => vec![tuple.object_id()],
        Change::Item(item) => vec![item],
        Change::Team(team) => graph.dependents(team).to_vec(),
    };
    let mut affected = BTreeSet::new();
    let mut queue: VecDeque<Uuid> = seeds.into();
    while let Some(item) = queue.pop_front() {
        if affected.insert(item) {
            queue.extend(graph.dependents(item));
        }
    }
    affected
}

/// The team whose items a change touches, if any (its reverse edges must be loaded first).
pub(crate) fn changed_team(change: &Change) -> Option<Uuid> {
    match *change {
        Change::Relation(tuple) if tuple.object_type() == ObjectType::Team => {
            Some(tuple.object_id())
        }
        Change::Team(team) => Some(team),
        Change::Relation(_) | Change::Item(_) => None,
    }
}

/// Every object whose forward edges `compute_access` reads for `items`: the items, the items they
/// are linked to or contained in (transitively) and the teams that own any of them. Objects
/// without loaded edges end the walk, so a loader calls this until it returns nothing new.
pub(crate) fn compute_inputs(
    graph: &AccessGraph,
    items: impl IntoIterator<Item = Uuid>,
) -> HashSet<(ObjectType, Uuid)> {
    let mut needed = HashSet::new();
    let mut queue: VecDeque<Uuid> = items.into_iter().collect();
    while let Some(item) = queue.pop_front() {
        if !needed.insert((ObjectType::Item, item)) {
            continue;
        }
        for &(relation, subject) in graph.edges(ObjectType::Item, item) {
            match relation {
                Relation::LinkedTo | Relation::InNotebook => queue.push_back(subject),
                Relation::OwnedBy => {
                    needed.insert((ObjectType::Team, subject));
                }
                _ => {}
            }
        }
    }
    needed
}

#[cfg(test)]
mod tests;
