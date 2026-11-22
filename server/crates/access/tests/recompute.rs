//! Synchronous recompute of `item_access` for every V8 trigger (T1.1.3, ARCHITECTURE §6.4)
//! against a real Postgres 17. Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use access::{
    AccessConfig, AccessContext, AccessEngine, Change, DefaultAccessContext, ItemAttrs,
    Relation as R, RelationTuple, RolePolicy, TeamVisibility,
};
use db::PgPool;
use db::testing::TestDb;
use sqlx::PgConnection;
use uuid::Uuid;

const VIEW: i32 = 1;
const VE: i32 = 3;
const ALL: i32 = 63;

fn id() -> Uuid {
    Uuid::now_v7()
}

fn t(object: Uuid, relation: R, subject: Uuid) -> RelationTuple {
    RelationTuple::new(object, relation, subject)
}

/// `item_access` rows of `item`: user -> perms bits.
async fn access_of(pool: &PgPool, item: Uuid) -> BTreeMap<Uuid, i32> {
    sqlx::query_as::<_, (Uuid, i32)>("SELECT user_id, perms FROM item_access WHERE item_id = $1")
        .bind(item)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

async fn reasons_of(pool: &PgPool, item: Uuid, user: Uuid) -> serde_json::Value {
    sqlx::query_scalar("SELECT reasons FROM item_access WHERE item_id = $1 AND user_id = $2")
        .bind(item)
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Commits a grant in its own transaction.
async fn granted(pool: &PgPool, tuple: RelationTuple, engine: &AccessEngine<impl AccessContext>) {
    let mut tx = pool.begin().await.unwrap();
    assert!(engine.grant(&mut tx, tuple).await.unwrap());
    tx.commit().await.unwrap();
}

async fn revoked(pool: &PgPool, tuple: RelationTuple, engine: &AccessEngine<impl AccessContext>) {
    let mut tx = pool.begin().await.unwrap();
    assert!(engine.revoke(&mut tx, tuple).await.unwrap());
    tx.commit().await.unwrap();
}

/// Item attributes and team policies the test controls, standing in for `items` and `teams`.
#[derive(Default)]
struct TestContext {
    attrs: Mutex<HashMap<Uuid, ItemAttrs>>,
    policies: Mutex<HashMap<Uuid, RolePolicy>>,
}

impl AccessContext for TestContext {
    async fn item_attrs(
        &self,
        _conn: &mut PgConnection,
        items: &[Uuid],
    ) -> sqlx::Result<HashMap<Uuid, ItemAttrs>> {
        let attrs = self.attrs.lock().unwrap();
        Ok(items
            .iter()
            .filter_map(|i| attrs.get(i).map(|a| (*i, *a)))
            .collect())
    }

    async fn team_policies(
        &self,
        _conn: &mut PgConnection,
        teams: &[Uuid],
    ) -> sqlx::Result<HashMap<Uuid, RolePolicy>> {
        let policies = self.policies.lock().unwrap();
        Ok(teams
            .iter()
            .filter_map(|t| policies.get(t).map(|p| (*t, *p)))
            .collect())
    }
}

fn engine() -> AccessEngine<DefaultAccessContext> {
    AccessEngine::new(DefaultAccessContext, AccessConfig::default())
}

#[tokio::test]
async fn linking_and_unlinking_a_page() {
    let db = TestDb::new().await;
    let (ali, ayse, page, task) = (id(), id(), id(), id());
    granted(db.pool(), t(page, R::Owner, ali), &engine()).await;
    granted(db.pool(), t(task, R::Owner, ayse), &engine()).await;
    assert_eq!(
        access_of(db.pool(), page).await,
        BTreeMap::from([(ali, ALL)])
    );

    granted(db.pool(), t(page, R::LinkedTo, task), &engine()).await;
    assert_eq!(
        access_of(db.pool(), page).await,
        BTreeMap::from([(ali, ALL), (ayse, VE)])
    );
    assert_eq!(
        reasons_of(db.pool(), page, ayse).await,
        serde_json::json!([{ "kind": "linked", "item_id": task }])
    );

    // V8: access that came through the link drops with it.
    revoked(db.pool(), t(page, R::LinkedTo, task), &engine()).await;
    assert_eq!(
        access_of(db.pool(), page).await,
        BTreeMap::from([(ali, ALL)])
    );
}

#[tokio::test]
async fn adding_and_removing_a_member_reaches_linked_pages() {
    let db = TestDb::new().await;
    let (ali, can, page, page2, task) = (id(), id(), id(), id(), id());
    for tuple in [
        t(task, R::Owner, ali),
        t(page, R::Owner, ali),
        t(page, R::LinkedTo, task),
        t(page2, R::Owner, ali),
        t(page2, R::LinkedTo, page),
    ] {
        granted(db.pool(), tuple, &engine()).await;
    }

    let mut tx = db.pool().begin().await.unwrap();
    assert!(
        engine()
            .grant(&mut tx, t(task, R::Assignee, can))
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert_eq!(access_of(db.pool(), task).await[&can], VE);
    assert_eq!(access_of(db.pool(), page).await[&can], VE);
    assert_eq!(access_of(db.pool(), page2).await[&can], VE);

    revoked(db.pool(), t(task, R::Assignee, can), &engine()).await;
    for item in [task, page, page2] {
        assert!(!access_of(db.pool(), item).await.contains_key(&can));
    }
}

#[tokio::test]
async fn notebook_membership_change() {
    let db = TestDb::new().await;
    let (ali, ayse, notebook, page) = (id(), id(), id(), id());
    granted(db.pool(), t(notebook, R::Owner, ali), &engine()).await;
    granted(db.pool(), t(page, R::Owner, ali), &engine()).await;
    granted(db.pool(), t(page, R::InNotebook, notebook), &engine()).await;

    granted(db.pool(), t(notebook, R::Viewer, ayse), &engine()).await;
    assert_eq!(access_of(db.pool(), notebook).await[&ayse], VIEW);
    assert_eq!(access_of(db.pool(), page).await[&ayse], VIEW);

    revoked(db.pool(), t(notebook, R::Viewer, ayse), &engine()).await;
    assert!(!access_of(db.pool(), page).await.contains_key(&ayse));
}

#[tokio::test]
async fn team_role_change() {
    let db = TestDb::new().await;
    let engine = AccessEngine::new(TestContext::default(), AccessConfig::default());
    let ctx = engine.context();
    let (ali, ayse, team, task, page) = (id(), id(), id(), id(), id());
    ctx.attrs.lock().unwrap().insert(
        task,
        ItemAttrs {
            team_visibility: TeamVisibility::Assignees,
            created_by: Some(ali),
        },
    );
    granted(db.pool(), t(team, R::TeamAdmin, ali), &engine).await;
    granted(db.pool(), t(team, R::TeamMember, ayse), &engine).await;
    granted(db.pool(), t(task, R::OwnedBy, team), &engine).await;
    granted(db.pool(), t(page, R::Owner, ali), &engine).await;
    granted(db.pool(), t(page, R::LinkedTo, task), &engine).await;
    // Assignee-only team task: the member does not see it, nor the page linked to it.
    assert_eq!(
        access_of(db.pool(), task).await,
        BTreeMap::from([(ali, ALL)])
    );
    assert!(!access_of(db.pool(), page).await.contains_key(&ayse));

    // Promotion to manager: managers see every team item (V12), and the linked page follows.
    let mut tx = db.pool().begin().await.unwrap();
    engine
        .grant(&mut tx, t(team, R::TeamManager, ayse))
        .await
        .unwrap();
    let affected = engine
        .recompute(&mut tx, Change::Relation(t(team, R::TeamManager, ayse)))
        .await
        .unwrap()
        .affected;
    tx.commit().await.unwrap();
    let mut expected = vec![task, page];
    expected.sort();
    assert_eq!(affected, expected);
    assert_eq!(access_of(db.pool(), task).await[&ayse], 31);
    assert_eq!(access_of(db.pool(), page).await[&ayse], VE);

    revoked(db.pool(), t(team, R::TeamManager, ayse), &engine).await;
    assert!(!access_of(db.pool(), task).await.contains_key(&ayse));
    assert!(!access_of(db.pool(), page).await.contains_key(&ayse));
}

#[tokio::test]
async fn visibility_mode_and_role_matrix_changes() {
    let db = TestDb::new().await;
    let engine = AccessEngine::new(TestContext::default(), AccessConfig::default());
    let ctx = engine.context();
    let (ali, ayse, team, task) = (id(), id(), id(), id());
    granted(db.pool(), t(team, R::TeamAdmin, ali), &engine).await;
    granted(db.pool(), t(team, R::TeamGuest, ayse), &engine).await;
    granted(db.pool(), t(task, R::OwnedBy, team), &engine).await;
    assert_eq!(access_of(db.pool(), task).await[&ayse], VIEW);

    // V12: switching to "assignees only" removes the guest.
    ctx.attrs.lock().unwrap().insert(
        task,
        ItemAttrs {
            team_visibility: TeamVisibility::Assignees,
            created_by: Some(ali),
        },
    );
    let mut tx = db.pool().begin().await.unwrap();
    assert_eq!(
        engine
            .recompute(&mut tx, Change::Item(task))
            .await
            .unwrap()
            .affected,
        [task]
    );
    tx.commit().await.unwrap();
    assert_eq!(
        access_of(db.pool(), task).await,
        BTreeMap::from([(ali, ALL)])
    );

    // Back to "all team", then the admin lets guests edit (TK3).
    ctx.attrs.lock().unwrap().clear();
    ctx.policies.lock().unwrap().insert(
        team,
        RolePolicy {
            guest: access::Perms::VIEW | access::Perms::EDIT,
            ..RolePolicy::default()
        },
    );
    let mut tx = db.pool().begin().await.unwrap();
    assert_eq!(
        engine
            .recompute(&mut tx, Change::Team(team))
            .await
            .unwrap()
            .affected,
        [task]
    );
    tx.commit().await.unwrap();
    assert_eq!(access_of(db.pool(), task).await[&ayse], VE);
}

#[tokio::test]
async fn recompute_is_part_of_the_callers_transaction() {
    let db = TestDb::new().await;
    let (ali, ayse, task) = (id(), id(), id());
    granted(db.pool(), t(task, R::Owner, ali), &engine()).await;

    // Rolled back: neither the relation nor the access survives.
    let mut tx = db.pool().begin().await.unwrap();
    engine()
        .grant(&mut tx, t(task, R::Watcher, ayse))
        .await
        .unwrap();
    let inside: Option<i32> =
        sqlx::query_scalar("SELECT perms FROM item_access WHERE item_id = $1 AND user_id = $2")
            .bind(task)
            .bind(ayse)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
    assert_eq!(inside, Some(VIEW), "visible inside the transaction");
    tx.rollback().await.unwrap();
    assert_eq!(
        access_of(db.pool(), task).await,
        BTreeMap::from([(ali, ALL)])
    );
}

#[tokio::test]
async fn repeated_grants_and_missing_revokes_are_no_ops() {
    let db = TestDb::new().await;
    let (ali, task) = (id(), id());
    let mut tx = db.pool().begin().await.unwrap();
    assert!(
        engine()
            .grant(&mut tx, t(task, R::Owner, ali))
            .await
            .unwrap()
    );
    assert!(
        !engine()
            .grant(&mut tx, t(task, R::Owner, ali))
            .await
            .unwrap()
    );
    assert!(
        !engine()
            .revoke(&mut tx, t(task, R::Watcher, ali))
            .await
            .unwrap()
    );
    assert!(
        engine()
            .revoke(&mut tx, t(task, R::Owner, ali))
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert!(access_of(db.pool(), task).await.is_empty());
}

#[tokio::test]
async fn concurrent_changes_leave_no_stale_access() {
    // Two transactions change relations that affect the same page; whichever commits last must
    // see the other's committed change (recomputes are serialized).
    let db = TestDb::new().await;
    let (ali, ayse, can, page, t1, t2) = (id(), id(), id(), id(), id(), id());
    for tuple in [
        t(page, R::Owner, ali),
        t(t1, R::Owner, ayse),
        t(t2, R::Owner, can),
    ] {
        granted(db.pool(), tuple, &engine()).await;
    }
    let link = move |target| t(page, R::LinkedTo, target);
    let pool_a = db.pool().clone();
    let pool_b = db.pool().clone();
    let a = tokio::spawn(async move { granted(&pool_a, link(t1), &engine()).await });
    let b = tokio::spawn(async move { granted(&pool_b, link(t2), &engine()).await });
    a.await.unwrap();
    b.await.unwrap();
    assert_eq!(
        access_of(db.pool(), page).await,
        BTreeMap::from([(ali, ALL), (ayse, VE), (can, VE)])
    );
}

/// `access.revoked` outbox events, oldest first: (item, users).
async fn revoked_events(pool: &PgPool) -> Vec<(Uuid, Vec<Uuid>)> {
    sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload FROM outbox WHERE event_type = 'access.revoked' ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|p| {
        let e: access::AccessRevoked = serde_json::from_value(p).unwrap();
        (e.item_id, e.user_ids)
    })
    .collect()
}

#[tokio::test]
async fn losing_view_access_publishes_access_revoked() {
    // V10: the realtime layer learns who lost access, in the same transaction.
    let db = TestDb::new().await;
    let (ali, ayse, page, task) = (id(), id(), id(), id());
    for tuple in [
        t(page, R::Owner, ali),
        t(task, R::Owner, ali),
        t(task, R::Watcher, ayse),
        t(page, R::LinkedTo, task),
    ] {
        granted(db.pool(), tuple, &engine()).await;
    }
    assert!(
        revoked_events(db.pool()).await.is_empty(),
        "grants revoke nothing"
    );

    revoked(db.pool(), t(task, R::Watcher, ayse), &engine()).await;
    let mut events = revoked_events(db.pool()).await;
    events.sort();
    let mut expected = vec![(page, vec![ayse]), (task, vec![ayse])];
    expected.sort();
    assert_eq!(events, expected);

    // A rolled-back revoke publishes nothing.
    let mut tx = db.pool().begin().await.unwrap();
    engine()
        .revoke(&mut tx, t(page, R::LinkedTo, task))
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(revoked_events(db.pool()).await.len(), 2);
}

#[tokio::test]
async fn removing_an_item_clears_its_relations_and_dependents() {
    let db = TestDb::new().await;
    let (ali, ayse, page, task) = (id(), id(), id(), id());
    for tuple in [
        t(page, R::Owner, ali),
        t(task, R::Owner, ali),
        t(task, R::Watcher, ayse),
        t(page, R::LinkedTo, task),
    ] {
        granted(db.pool(), tuple, &engine()).await;
    }
    let mut tx = db.pool().begin().await.unwrap();
    engine().remove_item(&mut tx, task).await.unwrap();
    tx.commit().await.unwrap();
    assert!(access_of(db.pool(), task).await.is_empty());
    // The page lost the inherited viewer but keeps its owner.
    assert_eq!(
        access_of(db.pool(), page).await,
        BTreeMap::from([(ali, ALL)])
    );
    let left: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM access_relations WHERE object_id = $1 OR subject_id = $1",
    )
    .bind(task)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(left, 0);
}
