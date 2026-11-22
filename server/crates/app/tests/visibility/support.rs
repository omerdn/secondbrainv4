//! Shared fixtures for the visibility suite. Grows with the access engine: T1.1.5 adds users and
//! items through the public services, T3.1.1 friendships, T5.1.1 teams.

use db::testing::TestDb;

/// One isolated, fully migrated database per test.
pub(crate) struct World {
    db: TestDb,
}

impl World {
    pub(crate) async fn new() -> Self {
        Self {
            db: TestDb::new().await,
        }
    }

    pub(crate) fn pool(&self) -> &db::PgPool {
        self.db.pool()
    }
}

/// Body of a scenario whose behaviour is not implemented yet. The test is `#[ignore]`d with the
/// task that implements it; that task replaces this call with the real scenario.
pub(crate) fn pending() -> ! {
    panic!("scenario not implemented yet; see the #[ignore] reason for the task that enables it")
}
