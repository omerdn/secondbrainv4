//! Recompute of `item_access` after relation changes (ARCHITECTURE §6.1, §6.4, PRD V8, T1.1.3,
//! T1.1.4). Domain modules call [`AccessEngine::grant`], [`AccessEngine::revoke`] and
//! [`AccessEngine::recompute`]; they never write the access tables themselves (ADR-0007).
//!
//! Small affected sets are recomputed in the caller's transaction. Above
//! [`AccessConfig::sync_limit`] items, removals still apply synchronously and only the widening is
//! left to an [`ExpandAccess`] job. `compute_access` is monotone in the relations, so a granted
//! relation only widens access and a revoked one only narrows it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::affected::{Change, affected_items, changed_team, compute_inputs};
use crate::compute::{AccessGraph, ItemAttrs, RolePolicy, compute_access};
use crate::events::AccessRevoked;
use crate::model::{ObjectType, Perms, Relation, RelationTuple};
use crate::repo::{self, AccessEntry};

/// Facts other modules own that `compute_access` needs: item attributes (`items`) and team role
/// matrices (`teams`). They are loaded through this interface, never with SQL on their tables.
pub trait AccessContext: Send + Sync + 'static {
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

/// Tuning of the recompute (`ACCESS_SYNC_LIMIT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessConfig {
    /// Largest affected set whose widening is applied synchronously (§6.4 step 2).
    pub sync_limit: usize,
}

impl AccessConfig {
    pub const DEFAULT_SYNC_LIMIT: usize = 500;
}

impl Default for AccessConfig {
    fn default() -> Self {
        Self {
            sync_limit: Self::DEFAULT_SYNC_LIMIT,
        }
    }
}

/// What a recompute did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recomputed {
    /// Every affected item, sorted.
    pub affected: Vec<Uuid>,
    /// `true` if widening was left to an [`ExpandAccess`] job (removals are always applied).
    pub deferred: bool,
}

/// Which way a change can move access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// A relation was added: access can only grow.
    Widen,
    /// A relation was removed: access can only shrink.
    Narrow,
    /// Attributes or a role matrix changed: either way.
    Mixed,
}

/// Background job finishing a deferred widening (`access.expand`). It recomputes the affected
/// items from the current state, so running it twice, late or after a retry is harmless.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpandAccess {
    change: ChangeDto,
}

impl jobs::Job for ExpandAccess {
    const KIND: &'static str = "access.expand";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ChangeDto {
    Relation {
        object_id: Uuid,
        relation: String,
        subject_id: Uuid,
    },
    Item {
        item_id: Uuid,
    },
    Team {
        team_id: Uuid,
    },
}

impl From<Change> for ChangeDto {
    fn from(change: Change) -> Self {
        match change {
            Change::Relation(t) => Self::Relation {
                object_id: t.object_id(),
                relation: t.relation().as_str().to_owned(),
                subject_id: t.subject_id(),
            },
            Change::Item(item_id) => Self::Item { item_id },
            Change::Team(team_id) => Self::Team { team_id },
        }
    }
}

impl ChangeDto {
    fn into_change(self) -> Option<Change> {
        Some(match self {
            Self::Relation {
                object_id,
                relation,
                subject_id,
            } => Change::Relation(RelationTuple::new(
                object_id,
                Relation::parse(&relation)?,
                subject_id,
            )),
            Self::Item { item_id } => Change::Item(item_id),
            Self::Team { team_id } => Change::Team(team_id),
        })
    }
}

/// The access engine: relation writes and `item_access` recompute.
pub struct AccessEngine<C> {
    ctx: Arc<C>,
    config: AccessConfig,
}

impl<C> Clone for AccessEngine<C> {
    fn clone(&self) -> Self {
        Self {
            ctx: Arc::clone(&self.ctx),
            config: self.config,
        }
    }
}

impl<C: AccessContext> AccessEngine<C> {
    pub fn new(ctx: C, config: AccessConfig) -> Self {
        Self {
            ctx: Arc::new(ctx),
            config,
        }
    }

    #[must_use]
    pub fn context(&self) -> &C {
        &self.ctx
    }

    /// The read side: permission checks, listing, explanations and previews.
    #[must_use]
    pub fn permissions(&self) -> crate::PermissionService<C> {
        crate::PermissionService::new(Arc::clone(&self.ctx))
    }

    /// Adds a relation and recomputes the affected items. Returns `false` (and recomputes nothing)
    /// if the relation already existed. Call it inside the transaction that makes the domain
    /// change.
    ///
    /// # Errors
    /// Database errors.
    pub async fn grant(&self, conn: &mut PgConnection, tuple: RelationTuple) -> sqlx::Result<bool> {
        if !repo::insert_relation(&mut *conn, &tuple).await? {
            return Ok(false);
        }
        self.run(conn, Change::Relation(tuple), Direction::Widen)
            .await?;
        Ok(true)
    }

    /// Removes a relation and recomputes the affected items; access that came through it drops
    /// immediately, whatever the size of the affected set (V8, V10). Returns `false` (and
    /// recomputes nothing) if the relation did not exist. Call it inside the transaction that makes
    /// the domain change.
    ///
    /// # Errors
    /// Database errors.
    pub async fn revoke(
        &self,
        conn: &mut PgConnection,
        tuple: RelationTuple,
    ) -> sqlx::Result<bool> {
        if !repo::delete_relation(&mut *conn, &tuple).await? {
            return Ok(false);
        }
        self.run(conn, Change::Relation(tuple), Direction::Narrow)
            .await?;
        Ok(true)
    }

    /// Recomputes the items affected by a change that is not a relation: [`Change::Item`]
    /// (visibility mode) or [`Change::Team`] (role matrix). Must run inside the transaction that
    /// made the change. A relation passed here is treated as possibly widening and narrowing.
    ///
    /// # Errors
    /// Database errors.
    pub async fn recompute(
        &self,
        conn: &mut PgConnection,
        change: Change,
    ) -> sqlx::Result<Recomputed> {
        self.run(conn, change, Direction::Mixed).await
    }

    /// Runs an [`ExpandAccess`] job: a full recompute of the change in its own transaction.
    ///
    /// # Errors
    /// Database errors, or a payload naming a relation this binary does not know.
    pub async fn expand(
        &self,
        pool: &PgPool,
        job: ExpandAccess,
    ) -> Result<Recomputed, jobs::HandlerError> {
        let change = job
            .change
            .into_change()
            .ok_or("access.expand: unknown relation in payload")?;
        let mut tx = pool.begin().await?;
        let done = self.run_full(&mut tx, change).await?;
        tx.commit().await?;
        Ok(done)
    }

    /// Registers the [`ExpandAccess`] handler on `worker`.
    pub fn register(&self, worker: &mut jobs::Worker, pool: PgPool) {
        let engine = self.clone();
        worker.register(move |job: ExpandAccess, _| {
            let engine = engine.clone();
            let pool = pool.clone();
            async move {
                let done = engine.expand(&pool, job).await?;
                tracing::info!(
                    items = done.affected.len(),
                    "deferred access expansion applied"
                );
                Ok(())
            }
        });
    }

    async fn run(
        &self,
        conn: &mut PgConnection,
        change: Change,
        direction: Direction,
    ) -> sqlx::Result<Recomputed> {
        lock(conn).await?;
        let mut loader = Loader::default();
        let affected = loader.affected(conn, &change).await?;
        if affected.len() <= self.config.sync_limit || direction == Direction::Narrow {
            loader
                .write(conn, &*self.ctx, &affected, Write::Replace)
                .await?;
            return Ok(Recomputed {
                affected: affected.into_iter().collect(),
                deferred: false,
            });
        }
        if direction == Direction::Mixed {
            loader
                .write(conn, &*self.ctx, &affected, Write::Restrict)
                .await?;
        }
        let job = ExpandAccess {
            change: change.into(),
        };
        jobs::enqueue(conn, &job, jobs::EnqueueOptions::default())
            .await
            .map_err(|err| sqlx::Error::Protocol(format!("enqueue access.expand: {err}")))?;
        Ok(Recomputed {
            affected: affected.into_iter().collect(),
            deferred: true,
        })
    }

    async fn run_full(&self, conn: &mut PgConnection, change: Change) -> sqlx::Result<Recomputed> {
        lock(conn).await?;
        let mut loader = Loader::default();
        let affected = loader.affected(conn, &change).await?;
        loader
            .write(conn, &*self.ctx, &affected, Write::Replace)
            .await?;
        Ok(Recomputed {
            affected: affected.into_iter().collect(),
            deferred: false,
        })
    }
}

/// Recomputes are serialized per database schema with a transaction-scoped advisory lock, so each
/// one reads every relation change committed before it and none leaves stale access behind.
async fn lock(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext(current_schema() || '.access_recompute'))")
        .execute(conn)
        .await
        .map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Write {
    /// Store the computed access as is.
    Replace,
    /// Only remove what is no longer granted; never add (the widening comes later).
    Restrict,
}

/// Loads just enough of the relation graph from the database, level by level, driven by the pure
/// functions that decide what is needed.
#[derive(Default)]
pub(crate) struct Loader {
    pub(crate) graph: AccessGraph,
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
    pub(crate) async fn affected(
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

    /// Loads what `compute_access` reads for `items`, computes and stores it.
    async fn write(
        &mut self,
        conn: &mut PgConnection,
        ctx: &impl AccessContext,
        items: &BTreeSet<Uuid>,
        mode: Write,
    ) -> sqlx::Result<()> {
        self.load_compute_inputs(conn, items).await?;
        self.load_context(conn, ctx).await?;
        for &item in items {
            let entries: Vec<AccessEntry> = compute_access(&self.graph, item)
                .into_iter()
                .map(|(user_id, access)| AccessEntry {
                    item_id: item,
                    user_id,
                    perms: access.perms,
                    reasons: serde_json::to_value(&access.reasons).unwrap_or_default(),
                })
                .collect();
            let before = repo::viewers(&mut *conn, item).await?;
            match mode {
                Write::Replace => repo::replace_item_access(conn, item, &entries).await?,
                Write::Restrict => repo::restrict_item_access(conn, item, &entries).await?,
            }
            // V10: whoever could view the item and no longer can is announced, so open
            // subscriptions end. Restrict keeps (before ∩ new) viewers: the same set is lost.
            let lost: Vec<Uuid> = before
                .into_iter()
                .filter(|user| {
                    !entries
                        .iter()
                        .any(|e| e.user_id == *user && e.perms.contains(Perms::VIEW))
                })
                .collect();
            if !lost.is_empty() {
                let event = AccessRevoked {
                    item_id: item,
                    user_ids: lost,
                };
                db::outbox::publish(conn, &event).await.map_err(|err| {
                    sqlx::Error::Protocol(format!("publish access.revoked: {err}"))
                })?;
            }
        }
        Ok(())
    }

    /// Loads forward edges until everything `compute_access` reads for `items` is present.
    pub(crate) async fn load_compute_inputs(
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

    pub(crate) async fn load_context(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_payload_round_trips() {
        let tuple = RelationTuple::new(Uuid::nil(), Relation::TeamManager, Uuid::max());
        for change in [
            Change::Relation(tuple),
            Change::Item(Uuid::nil()),
            Change::Team(Uuid::max()),
        ] {
            let job = ExpandAccess {
                change: change.into(),
            };
            let json = serde_json::to_value(&job).unwrap_or_default();
            let back: ExpandAccess = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(back.change.into_change(), Some(change));
        }
        let unknown = ChangeDto::Relation {
            object_id: Uuid::nil(),
            relation: "future".into(),
            subject_id: Uuid::nil(),
        };
        assert_eq!(unknown.into_change(), None);
        assert_eq!(AccessConfig::default().sync_limit, 500);
    }
}
