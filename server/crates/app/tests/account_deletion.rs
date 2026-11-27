//! Account deletion (T4.3.1, PRD HS2, V8) across identity, items and tasks, against Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Perms, Relation as R, RelationTuple};
use chrono::{DateTime, Duration, TimeZone, Utc};
use db::testing::TestDb;
use identity::deletion::{GRACE, SharedDeletionHook};
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService, UserId};
use items::{ItemsAccessContext, ItemsService, NewItem, Owner};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use uuid::Uuid;

fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 27, 9, 0, 0).unwrap()
}

struct World {
    db: TestDb,
    clock: FixedClock,
    access: AccessEngine<ItemsAccessContext>,
    items: ItemsService,
    identity: IdentityService,
    hooks: Vec<SharedDeletionHook>,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(start());
        let shared: kernel::SharedClock = Arc::new(clock.clone());
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(db.pool().clone(), shared.clone(), access.clone());
        let identity = IdentityService::new(
            db.pool().clone(),
            shared.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        let tasks = tasks::TasksService::new(
            db.pool().clone(),
            shared,
            access.permissions(),
            items.clone(),
        );
        let hooks: Vec<SharedDeletionHook> = vec![
            Arc::new(items.clone()),
            Arc::new(tasks),
            Arc::new(app::idempotency::ForgetUserKeys(db.pool().clone())),
        ];
        Self {
            db,
            clock,
            access,
            items,
            identity,
            hooks,
        }
    }

    async fn user(&self, email: &str, name: &str) -> Uuid {
        let request = RegisterRequest {
            email: email.into(),
            password: "correct horse".into(),
            display_name: name.into(),
            client: ClientKind::Native,
            device: None,
        }
        .validate()
        .unwrap();
        let (user, _) = self.identity.register(request).await.unwrap();
        user.id.as_uuid()
    }

    async fn item(&self, owner: Uuid, kind: &str, title: &str) -> Uuid {
        self.items
            .create(
                owner,
                NewItem {
                    kind: kind.into(),
                    title: title.into(),
                    description: None,
                },
            )
            .await
            .unwrap()
            .id
    }

    async fn grant(&self, tuple: RelationTuple) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.access.grant(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    async fn can_view(&self, user: Uuid, item: Uuid) -> bool {
        self.access
            .permissions()
            .check(self.db.pool(), user, item, Perms::VIEW)
            .await
            .unwrap()
    }

    async fn owner(&self, item: Uuid) -> Option<Uuid> {
        let row: Option<Option<Uuid>> =
            sqlx::query_scalar("SELECT owner_user_id FROM items WHERE id = $1")
                .bind(item)
                .fetch_optional(self.db.pool())
                .await
                .unwrap();
        row.flatten()
    }

    async fn count(&self, sql: &str, user: Uuid) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(user)
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }

    async fn run_deletion(&self) -> usize {
        self.identity
            .delete_due_accounts(&self.hooks)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn hs2_a_request_waits_thirty_days_and_can_be_cancelled() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let id = UserId::from_uuid(ada);
    assert_eq!(
        w.identity.account_deletion(id).await.unwrap().requested_at,
        None
    );
    let asked = w.identity.request_deletion(id).await.unwrap();
    assert_eq!(asked.requested_at, Some(start()));
    assert_eq!(asked.deletes_at, Some(start() + GRACE));
    // Asking again keeps the first date.
    w.clock.advance(Duration::days(3));
    assert_eq!(
        w.identity.request_deletion(id).await.unwrap().requested_at,
        Some(start())
    );
    // One second short of 30 days: nothing happens.
    w.clock.set(start() + GRACE - Duration::seconds(1));
    assert_eq!(w.run_deletion().await, 0);
    assert!(w.identity.current_user(id).await.is_ok());
    // Cancelled: the account stays even long after.
    w.identity.cancel_deletion(id).await.unwrap();
    w.clock.set(start() + GRACE * 2);
    assert_eq!(w.run_deletion().await, 0);
    assert!(w.identity.current_user(id).await.is_ok());
}

#[tokio::test]
async fn hs2_after_thirty_days_shared_items_go_to_the_oldest_member_and_the_rest_is_deleted() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    // A shared task: Bob joined before Can.
    let shared = w.item(ada, "task", "Ortak").await;
    w.grant(RelationTuple::new(shared, R::Watcher, bob)).await;
    w.clock.advance(Duration::minutes(1));
    w.grant(RelationTuple::new(shared, R::Assignee, can)).await;
    // A shared notebook with a page (Y5: the page belongs to the notebook's owner).
    let notebook = w.item(ada, "notebook", "Defter").await;
    let page = w.item(ada, "page", "Sayfa").await;
    w.grant(RelationTuple::new(page, R::InNotebook, notebook))
        .await;
    w.grant(RelationTuple::new(notebook, R::Editor, can)).await;
    // Ada's own task, and Bob's task Ada watches.
    let private = w.item(ada, "task", "Özel").await;
    let bobs = w.item(bob, "task", "Bob'un").await;
    w.grant(RelationTuple::new(bobs, R::Watcher, ada)).await;
    // The guide (HS2.1) lists what will be handed over and to whom.
    let guide = w
        .items
        .owned_shared(ada, |ids| async move {
            Ok(ids.into_iter().map(|id| (id, "?".to_owned())).collect())
        })
        .await
        .unwrap();
    let mut listed: Vec<(Uuid, Uuid)> = guide.iter().map(|g| (g.item_id, g.heir_id)).collect();
    listed.sort();
    let mut expected = vec![(shared, bob), (notebook, can)];
    expected.sort();
    assert_eq!(listed, expected);

    w.identity
        .request_deletion(UserId::from_uuid(ada))
        .await
        .unwrap();
    w.clock.advance(GRACE);
    assert_eq!(w.run_deletion().await, 1);

    // The account and its sign-in data are gone.
    assert!(matches!(
        w.identity.current_user(UserId::from_uuid(ada)).await,
        Err(AppError::Unauthenticated)
    ));
    for table in ["sessions", "auth_identities"] {
        assert_eq!(
            w.count(
                &format!("SELECT count(*) FROM {table} WHERE user_id = $1"),
                ada
            )
            .await,
            0,
            "{table}"
        );
    }
    // Handed over, access recomputed (V8).
    assert_eq!(w.owner(shared).await, Some(bob));
    assert_eq!(w.owner(notebook).await, Some(can));
    assert_eq!(
        w.owner(page).await,
        Some(can),
        "the page follows its notebook"
    );
    assert!(w.can_view(bob, shared).await);
    assert!(w.can_view(can, shared).await);
    assert!(w.can_view(can, page).await);
    assert!(
        matches!(w.items.get(can, page).await.unwrap().owner, Owner::User { user_id } if user_id == can)
    );
    // Nobody else had it: deleted for good.
    assert_eq!(w.owner(private).await, None);
    // No relation of Ada is left anywhere.
    assert_eq!(
        w.count(
            "SELECT count(*) FROM access_relations WHERE subject_type = 'user' AND subject_id = $1",
            ada
        )
        .await,
        0
    );
    assert_eq!(
        w.count("SELECT count(*) FROM item_access WHERE user_id = $1", ada)
            .await,
        0
    );
    assert!(w.can_view(bob, bobs).await);
    // A second run (a retry) changes nothing.
    assert_eq!(w.run_deletion().await, 0);
}

#[tokio::test]
async fn hs2_personal_rows_without_a_foreign_key_are_removed() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    w.item(ada, "task", "Rapor").await;
    let pool = w.db.pool();
    sqlx::query(
        "INSERT INTO tags (id, owner_user_id, name, created_at, updated_at)
         VALUES ($1, $2, 'iş', now(), now())",
    )
    .bind(Uuid::now_v7())
    .bind(ada)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_board_columns (id, user_id, name, position, is_inbox, created_at, updated_at)
         VALUES ($1, $2, 'Gelen', 'V', true, now(), now())",
    )
    .bind(Uuid::now_v7())
    .bind(ada)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(
        w.count(
            "SELECT count(*) FROM user_board_columns WHERE user_id = $1",
            ada
        )
        .await,
        1
    );
    sqlx::query(
        "INSERT INTO idempotency_keys (user_id, key, request_hash, created_at)
         VALUES ($1, 'k1', decode(repeat('00', 32), 'hex'), now())",
    )
    .bind(ada)
    .execute(pool)
    .await
    .unwrap();
    w.identity
        .request_deletion(UserId::from_uuid(ada))
        .await
        .unwrap();
    w.clock.advance(GRACE);
    assert_eq!(w.run_deletion().await, 1);
    for sql in [
        "SELECT count(*) FROM tags WHERE owner_user_id = $1",
        "SELECT count(*) FROM idempotency_keys WHERE user_id = $1",
        "SELECT count(*) FROM user_board_columns WHERE user_id = $1",
        "SELECT count(*) FROM items WHERE owner_user_id = $1",
        "SELECT count(*) FROM email_tokens WHERE user_id = $1",
    ] {
        assert_eq!(w.count(sql, ada).await, 0, "{sql}");
    }
}

#[tokio::test]
async fn hs2_the_owner_hands_an_item_to_a_member_only() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let eve = w.user("eve@example.com", "Eve").await;
    let task = w.item(ada, "task", "Ortak").await;
    w.grant(RelationTuple::new(task, R::Watcher, bob)).await;
    // Not a member; not the owner; a page.
    assert!(matches!(
        w.items.transfer_ownership(ada, task, eve).await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        w.items.transfer_ownership(bob, task, bob).await,
        Err(AppError::NotFound)
    ));
    let notebook = w.item(ada, "notebook", "Defter").await;
    let page = w.item(ada, "page", "Sayfa").await;
    w.grant(RelationTuple::new(page, R::InNotebook, notebook))
        .await;
    assert!(matches!(
        w.items.transfer_ownership(ada, page, bob).await,
        Err(AppError::Validation(_))
    ));
    w.items.transfer_ownership(ada, task, bob).await.unwrap();
    assert_eq!(w.owner(task).await, Some(bob));
    assert!(w.can_view(bob, task).await);
    assert!(
        !w.can_view(ada, task).await,
        "the former owner keeps no role"
    );
    // Ada no longer owns it: she cannot hand it again.
    assert!(matches!(
        w.items.transfer_ownership(ada, task, bob).await,
        Err(AppError::NotFound)
    ));
}
