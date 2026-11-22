//! Synchronous recompute of `item_access` in the transaction that changed the relations
//! (ARCHITECTURE §6.1, §6.4, PRD V8, T1.1.3). Domain modules call [`grant`], [`revoke`] and
//! [`recompute`]; they never write the access tables themselves (ADR-0007).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::affected::{Change, affected_items, changed_team, compute_inputs};
use crate::compute::{AccessGraph, ItemAttrs, RolePolicy, compute_access};
use crate::model::{ObjectType, RelationTuple};
use crate::repo::{self, AccessEntry};

/// Facts other modules own that `compute_access` needs: item attributes (`items`) and team role
/// matrices (`teams`). They are loaded through this interface, never with SQL on their tables.
pub trait AccessContext: Sync {
    /// Attributes of `items`; missing items use [`ItemAttrs::default`].
    fn item_attrs(
        &self,
        conn: &mut PgConnection,
        items: &[Uuid],
    ) -> impl Future<Output = sqlx::Result<HashMap<Uuid, ItemAttrs>>> + Send;

    /// Role matrices of `teams`; missing teams use [`RolePolicy::default`].
    fn team_policies(
        &self,
        conn: &mut PgConnection,
        teams: &[Uuid],
    ) -> impl Future<Output = sqlx::Result<HashMap<Uuid, RolePolicy>>> + Send;
}

/// Defaults for every item and team: used until the `items` (T1.2.1) and `teams` (T5.1.x)
/// modules provide their own context.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultAccessContext;

impl AccessContext for DefaultAccessContext {
    async fn item_attrs(
        &self,
        _conn: &mut PgConnection,
        _items: &[Uuid],
    ) -> sqlx::Result<HashMap<Uuid, ItemAttrs>> {
        Ok(HashMap::new())
    }

    async fn team_policies(
        &self,
        _conn: &mut PgConnection,
        _teams: &[Uuid],
    ) -> sqlx::Result<HashMap<Uuid, RolePolicy>> {
        Ok(HashMap::new())
    }
}

/// Adds a relation and recomputes the affected items. Returns `false` (and recomputes nothing) if
/// the relation already existed. Call it inside the transaction that makes the domain change.
///
/// # Errors
/// Database errors.
pub async fn grant(
    conn: &mut PgConnection,
    tuple: RelationTuple,
    ctx: &impl AccessContext,
) -> sqlx::Result<bool> {
    if !repo::insert_relation(&mut *conn, &tuple).await? {
        return Ok(false);
    }
    recompute(conn, Change::Relation(tuple), ctx).await?;
    Ok(true)
}

/// Removes a relation and recomputes the affected items; access that came through it drops (V8).
/// Returns `false` (and recomputes nothing) if the relation did not exist. Call it inside the
/// transaction that makes the domain change.
///
/// # Errors
/// Database errors.
pub async fn revoke(
    conn: &mut PgConnection,
    tuple: RelationTuple,
    ctx: &impl AccessContext,
) -> sqlx::Result<bool> {
    if !repo::delete_relation(&mut *conn, &tuple).await? {
        return Ok(false);
    }
    recompute(conn, Change::Relation(tuple), ctx).await?;
    Ok(true)
}

/// Recomputes `item_access` for every item `change` affects and returns them, sorted. Use it
/// directly for changes that are not relations: [`Change::Item`] (visibility mode) and
/// [`Change::Team`] (role matrix). Must run inside the transaction that made the change.
///
/// Recomputes are serialized per database schema with a transaction-scoped advisory lock, so each
/// one reads every relation change committed before it and none leaves stale access behind.
///
/// # Errors
/// Database errors.
pub async fn recompute(
    conn: &mut PgConnection,
    change: Change,
    ctx: &impl AccessContext,
) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext(current_schema() || '.access_recompute'))")
        .execute(&mut *conn)
        .await?;

    let mut loader = Loader::default();
    let affected = loader.affected(conn, &change).await?;
    loader.load_compute_inputs(conn, &affected).await?;
    loader.load_context(conn, ctx).await?;

    for &item in &affected {
        let entries: Vec<AccessEntry> = compute_access(&loader.graph, item)
            .into_iter()
            .map(|(user_id, access)| AccessEntry {
                item_id: item,
                user_id,
                perms: access.perms,
                reasons: serde_json::to_value(&access.reasons).unwrap_or_default(),
            })
            .collect();
        repo::replace_item_access(conn, item, &entries).await?;
    }
    Ok(affected.into_iter().collect())
}

/// Loads just enough of the relation graph from the database, level by level, driven by the pure
/// functions that decide what is needed.
#[derive(Default)]
struct Loader {
    graph: AccessGraph,
    forward_loaded: HashSet<(ObjectType, Uuid)>,
    reverse_loaded: HashSet<Uuid>,
}

impl Loader {
    fn add(&mut self, tuples: Vec<RelationTuple>) {
        for tuple in tuples {
            self.graph.add(tuple);
        }
    }

    /// Loads reverse edges until the affected set stops growing.
    async fn affected(
        &mut self,
        conn: &mut PgConnection,
        change: &Change,
    ) -> sqlx::Result<BTreeSet<Uuid>> {
        let team = changed_team(change);
        loop {
            let affected = affected_items(&self.graph, change);
            let missing: Vec<Uuid> = team
                .into_iter()
                .chain(affected.iter().copied())
                .filter(|id| self.reverse_loaded.insert(*id))
                .collect();
            if missing.is_empty() {
                return Ok(affected);
            }
            let tuples = repo::dependent_relations(&mut *conn, &missing).await?;
            self.add(tuples);
        }
    }

    /// Loads forward edges until everything `compute_access` reads for `items` is present.
    async fn load_compute_inputs(
        &mut self,
        conn: &mut PgConnection,
        items: &BTreeSet<Uuid>,
    ) -> sqlx::Result<()> {
        loop {
            let missing: Vec<(ObjectType, Uuid)> =
                compute_inputs(&self.graph, items.iter().copied())
                    .into_iter()
                    .filter(|object| self.forward_loaded.insert(*object))
                    .collect();
            if missing.is_empty() {
                return Ok(());
            }
            let tuples = repo::relations_of_objects(&mut *conn, &missing).await?;
            self.add(tuples);
        }
    }

    async fn load_context(
        &mut self,
        conn: &mut PgConnection,
        ctx: &impl AccessContext,
    ) -> sqlx::Result<()> {
        let mut items: Vec<Uuid> = Vec::new();
        let mut teams: Vec<Uuid> = Vec::new();
        for (object_type, id) in &self.forward_loaded {
            match object_type {
                ObjectType::Item => items.push(*id),
                ObjectType::Team => teams.push(*id),
            }
        }
        items.sort_unstable();
        teams.sort_unstable();
        for (item, attrs) in ctx.item_attrs(conn, &items).await? {
            self.graph.set_item(item, attrs);
        }
        for (team, policy) in ctx.team_policies(conn, &teams).await? {
            self.graph.set_team_policy(team, policy);
        }
        Ok(())
    }
}
