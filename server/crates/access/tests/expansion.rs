//! Deferred widening for large affected sets (T1.1.4, ARCHITECTURE §6.4 steps 2–3) against a real
//! Postgres 17. Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use access::{
    AccessConfig, AccessContext, AccessEngine, Change, ExpandAccess, ItemAttrs, Perms,
    Relation as R, RelationTuple, RolePolicy,
};
use db::PgPool;
use db::testing::TestDb;
use jobs::Job;
use sqlx::PgConnection;
use uuid::Uuid;

const VIEW: i32 = 1;
const VE: i32 = 3;

fn id() -> Uuid {
    Uuid::now_v7()
}

fn t(object: Uuid, relation: R, subject: Uuid) -> RelationTuple {
    RelationTuple::new(object, relation, subject)
}

async fn perms(pool: &PgPool, item: Uuid, user: Uuid) -> Option<i32> {
    sqlx::query_scalar("SELECT perms FROM item_access WHERE item_id = $1 AND user_id = $2")
        .bind(item)
        .bind(user)
        .fetch_optional(pool)
        .await
        .unwrap()
}

async fn pending_expansions(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = $1 AND completed_at IS NULL")
        .bind(ExpandAccess::KIND)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[derive(Default)]
struct Policies(Mutex<HashMap<Uuid, RolePolicy>>);

impl AccessContext for Policies {
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
        teams: &[Uuid],
    ) -> sqlx::Result<HashMap<Uuid, RolePolicy>> {
        let map = self.0.lock().unwrap();
        Ok(teams
            .iter()
            .filter_map(|t| map.get(t).map(|p| (*t, *p)))
            .collect())
    }
}

/// A team that owns three items, with Ali as admin; the engine applies at most two items
/// synchronously.
struct Fixture {
    db: TestDb,
    engine: AccessEngine<Policies>,
    worker: jobs::Worker,
    team: Uuid,
    items: [Uuid; 3],
}

impl Fixture {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let engine = AccessEngine::new(Policies::default(), AccessConfig { sync_limit: 2 });
        let mut worker = jobs::Worker::new(jobs::WorkerConfig::default());
        engine.register(&mut worker, db.pool().clone());
        let (team, items) = (id(), [id(), id(), id()]);
        let fixture = Self {
            db,
            engine,
            worker,
            team,
            items,
        };
        // Ownership is granted item by item: each change affects one item, below the limit.
        for item in items {
            fixture.commit_grant(t(item, R::OwnedBy, team)).await;
        }
        fixture
    }

    fn pool(&self) -> &PgPool {
        self.db.pool()
    }

    async fn commit_grant(&self, tuple: RelationTuple) {
        let mut tx = self.pool().begin().await.unwrap();
        assert!(self.engine.grant(&mut tx, tuple).await.unwrap());
        tx.commit().await.unwrap();
    }

    async fn commit_revoke(&self, tuple: RelationTuple) {
        let mut tx = self.pool().begin().await.unwrap();
        assert!(self.engine.revoke(&mut tx, tuple).await.unwrap());
        tx.commit().await.unwrap();
    }

    async fn run_jobs(&self) {
        let outcome = self.worker.run_batch(self.pool()).await.unwrap();
        assert_eq!((outcome.failed, outcome.retried), (0, 0), "{outcome:?}");
    }

    async fn perms_everywhere(&self, user: Uuid) -> Vec<Option<i32>> {
        let mut all = Vec::new();
        for item in self.items {
            all.push(perms(self.pool(), item, user).await);
        }
        all
    }
}

#[tokio::test]
async fn large_widening_is_deferred_to_a_job() {
    let f = Fixture::new().await;
    let ayse = id();

    // Three affected items > limit 2: the grant commits, the access arrives with the job.
    f.commit_grant(t(f.team, R::TeamMember, ayse)).await;
    assert_eq!(f.perms_everywhere(ayse).await, [None, None, None]);
    assert_eq!(pending_expansions(f.pool()).await, 1);

    f.run_jobs().await;
    assert_eq!(f.perms_everywhere(ayse).await, [Some(VE); 3]);
    assert_eq!(pending_expansions(f.pool()).await, 0);
}

#[tokio::test]
async fn expansion_job_is_idempotent_and_reads_the_current_state() {
    let f = Fixture::new().await;
    let (ayse, can) = (id(), id());
    f.commit_grant(t(f.team, R::TeamMember, ayse)).await;
    f.commit_grant(t(f.team, R::TeamGuest, can)).await;
    assert_eq!(pending_expansions(f.pool()).await, 2);

    // Running the same job again (a retry) changes nothing; each job recomputes everything
    // current, so the first one already brings both users in.
    let job = || -> ExpandAccess {
        serde_json::from_value(serde_json::json!({
            "change": { "kind": "team", "team_id": f.team }
        }))
        .unwrap()
    };
    let first = f.engine.expand(f.pool(), job()).await.unwrap();
    assert!(!first.deferred);
    assert_eq!(first.affected.len(), 3);
    assert_eq!(f.perms_everywhere(ayse).await, [Some(VE); 3]);
    assert_eq!(f.perms_everywhere(can).await, [Some(VIEW); 3]);
    f.engine.expand(f.pool(), job()).await.unwrap();
    f.run_jobs().await;
    assert_eq!(f.perms_everywhere(ayse).await, [Some(VE); 3]);
    assert_eq!(f.perms_everywhere(can).await, [Some(VIEW); 3]);
}

#[tokio::test]
async fn removal_is_always_synchronous() {
    let f = Fixture::new().await;
    let ayse = id();
    f.commit_grant(t(f.team, R::TeamMember, ayse)).await;
    f.run_jobs().await;
    assert_eq!(f.perms_everywhere(ayse).await, [Some(VE); 3]);

    // Above the limit, yet the access is gone as soon as the revoke commits: the first request
    // afterwards sees nothing (V8, V10).
    f.commit_revoke(t(f.team, R::TeamMember, ayse)).await;
    assert_eq!(f.perms_everywhere(ayse).await, [None, None, None]);
    assert_eq!(pending_expansions(f.pool()).await, 0);
}

#[tokio::test]
async fn mixed_changes_restrict_now_and_widen_later() {
    let f = Fixture::new().await;
    let (ayse, can) = (id(), id());
    f.commit_grant(t(f.team, R::TeamMember, ayse)).await;
    f.commit_grant(t(f.team, R::TeamGuest, can)).await;
    f.run_jobs().await;

    // The admin takes guests' access away and lets members share (TK3).
    f.engine.context().0.lock().unwrap().insert(
        f.team,
        RolePolicy {
            member: Perms::VIEW | Perms::EDIT | Perms::SHARE,
            guest: Perms::NONE,
            ..RolePolicy::default()
        },
    );
    let mut tx = f.pool().begin().await.unwrap();
    let done = f
        .engine
        .recompute(&mut tx, Change::Team(f.team))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(done.deferred);
    assert_eq!(done.affected.len(), 3);
    // Narrowing applied at once; widening not yet.
    assert_eq!(f.perms_everywhere(can).await, [None, None, None]);
    assert_eq!(f.perms_everywhere(ayse).await, [Some(VE); 3]);

    f.run_jobs().await;
    assert_eq!(f.perms_everywhere(ayse).await, [Some(VE | 4); 3]);
    assert_eq!(f.perms_everywhere(can).await, [None, None, None]);
}

#[tokio::test]
async fn small_sets_stay_synchronous() {
    let f = Fixture::new().await;
    let ayse = id();
    let mut tx = f.pool().begin().await.unwrap();
    f.engine
        .grant(&mut tx, t(f.items[0], R::Watcher, ayse))
        .await
        .unwrap();
    let done = f
        .engine
        .recompute(&mut tx, Change::Item(f.items[0]))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!done.deferred);
    assert_eq!(perms(f.pool(), f.items[0], ayse).await, Some(VIEW));
    assert_eq!(pending_expansions(f.pool()).await, 0);
    // The access map is otherwise untouched.
    let rows: BTreeMap<Uuid, i64> = sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT item_id, count(*) FROM item_access GROUP BY item_id",
    )
    .fetch_all(f.pool())
    .await
    .unwrap()
    .into_iter()
    .collect();
    assert_eq!(rows.get(&f.items[0]), Some(&1));
}

#[tokio::test]
async fn unknown_relation_in_a_job_fails_it() {
    let f = Fixture::new().await;
    let job: ExpandAccess = serde_json::from_value(serde_json::json!({
        "change": { "kind": "relation", "object_id": f.team, "relation": "future", "subject_id": f.team }
    }))
    .unwrap();
    assert!(f.engine.expand(f.pool(), job).await.is_err());
}
