//! Shared fixtures for the visibility suite: relations go through the access engine (T1.1.3), checks
//! through `PermissionService` (T1.1.5). Grows with items (T1.2.1), friendships (T3.1.1), teams (T5.1.1).

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, PermissionService, Relation, RelationTuple};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService};
use uuid::Uuid;

/// One isolated, fully migrated database per test.
pub(crate) struct World {
    db: TestDb,
    engine: AccessEngine<ItemsAccessContext>,
}

impl World {
    pub(crate) async fn new() -> Self {
        Self {
            db: TestDb::new().await,
            engine: AccessEngine::new(ItemsAccessContext, AccessConfig::default()),
        }
    }

    pub(crate) fn pool(&self) -> &db::PgPool {
        self.db.pool()
    }

    /// Database schema of this test; also its notification channel name.
    pub(crate) fn schema(&self) -> &str {
        self.db.schema()
    }

    pub(crate) async fn revoke(&self, tuple: RelationTuple) {
        let mut tx = self.pool().begin().await.unwrap();
        self.engine.revoke(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    /// The items service on this world's database and access engine, with the system clock.
    pub(crate) fn items(&self) -> ItemsService {
        ItemsService::new(
            self.pool().clone(),
            Arc::new(kernel::SystemClock),
            self.engine.clone(),
        )
    }

    pub(crate) fn permissions(&self) -> PermissionService<ItemsAccessContext> {
        self.engine.permissions()
    }

    /// Writes relations through the access engine in one committed transaction.
    pub(crate) async fn grant(&self, tuples: &[RelationTuple]) {
        let mut tx = self.pool().begin().await.unwrap();
        for tuple in tuples {
            self.engine.grant(&mut tx, *tuple).await.unwrap();
        }
        tx.commit().await.unwrap();
    }
}

pub(crate) fn id() -> Uuid {
    Uuid::now_v7()
}

pub(crate) fn t(object: Uuid, relation: Relation, subject: Uuid) -> RelationTuple {
    RelationTuple::new(object, relation, subject)
}

/// Body of a scenario whose behaviour is not implemented yet. The test is `#[ignore]`d with the
/// task that implements it; that task replaces this call with the real scenario.
pub(crate) fn pending() -> ! {
    panic!("scenario not implemented yet; see the #[ignore] reason for the task that enables it")
}
