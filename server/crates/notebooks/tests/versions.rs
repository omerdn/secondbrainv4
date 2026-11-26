//! Version history and restore (T2.5.4, PRD Y3, ARCHITECTURE §9) against a real Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService};
use kernel::{AppError, FixedClock};
use notebooks::collab::Collab;
use notebooks::snapshots;
use notebooks::{CreateNotebookRequest, CreatePageRequest, NotebooksService, repo};
use uuid::Uuid;
use yrs::{Doc, GetString, ReadTxn, Text, Transact};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap()
}

struct World {
    db: TestDb,
    access: AccessEngine<ItemsAccessContext>,
    notebooks: NotebooksService,
    collab: Collab,
    owner: Uuid,
    /// The writer's own copy, from which updates are produced.
    doc: Doc,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let clock = Arc::new(FixedClock::new(now()));
        let items = ItemsService::new(db.pool().clone(), clock.clone(), access.clone());
        let notebooks =
            NotebooksService::new(db.pool().clone(), clock.clone(), access.clone(), items);
        let collab = Collab::new(db.pool().clone(), clock);
        Self {
            db,
            access,
            notebooks,
            collab,
            owner: Uuid::now_v7(),
            doc: Doc::new(),
        }
    }

    async fn page(&self) -> Uuid {
        let notebook = self
            .notebooks
            .create(
                self.owner,
                CreateNotebookRequest {
                    title: "Defter".into(),
                    description: None,
                },
            )
            .await
            .unwrap()
            .item
            .id;
        self.notebooks
            .create_page(
                self.owner,
                notebook,
                CreatePageRequest {
                    title: "Sayfa".into(),
                },
            )
            .await
            .unwrap()
            .item
            .id
    }

    async fn grant(&self, tuple: RelationTuple) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.access.grant(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    /// Appends `chunk` to the writer's copy and stores the update, as the collab server does.
    async fn write(&self, page: Uuid, chunk: &str) {
        let text = self.doc.get_or_insert_text("content");
        let before = self.doc.transact().state_vector();
        let len = text.len(&self.doc.transact());
        text.insert(&mut self.doc.transact_mut(), len, chunk);
        let update = self.doc.transact().encode_state_as_update_v1(&before);
        repo::append_update(self.db.pool(), page, &update, self.owner, now())
            .await
            .unwrap();
    }

    /// Writes `chunk` and makes a version of the result.
    async fn version(&self, page: Uuid, chunk: &str) {
        self.write(page, chunk).await;
        snapshots::compact(self.db.pool(), page, now())
            .await
            .unwrap();
    }

    async fn stored(&self, page: Uuid) -> String {
        let doc = snapshots::load(self.db.pool(), page).await.unwrap();
        let text = doc.get_or_insert_text("content");
        text.get_string(&doc.transact())
    }
}

#[tokio::test]
async fn y3_versions_list_newest_first_and_show_their_content() {
    let w = World::new().await;
    let page = w.page().await;
    assert!(
        w.notebooks
            .versions(w.owner, page, None)
            .await
            .unwrap()
            .is_empty()
    );
    w.version(page, "Bir").await;
    w.version(page, " iki").await;
    let list = w.notebooks.versions(w.owner, page, None).await.unwrap();
    let numbers: Vec<i32> = list.iter().map(|v| v.version).collect();
    assert_eq!(numbers, vec![2, 1]);
    assert_eq!(list[0].preview, "Bir iki");
    assert_eq!(list[1].created_at, now());
    // The next page of the list.
    let older = w.notebooks.versions(w.owner, page, Some(2)).await.unwrap();
    assert_eq!(older.iter().map(|v| v.version).collect::<Vec<_>>(), vec![1]);

    let first = w.notebooks.version(w.owner, page, 1).await.unwrap();
    assert_eq!(first.plain_text, "Bir");
    let state = base64::engine::general_purpose::STANDARD
        .decode(first.state)
        .unwrap();
    let doc = snapshots::rebuild(Some(&state), &[]).unwrap();
    assert_eq!(snapshots::plain_text(&doc), "Bir");
    assert!(matches!(
        w.notebooks.version(w.owner, page, 3).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn y3_restoring_is_a_new_change_and_the_history_stays() {
    let w = World::new().await;
    let page = w.page().await;
    w.version(page, "Bir").await;
    w.version(page, " iki").await;
    // Not yet a version: restoring must keep it in the history too.
    w.write(page, " üç").await;

    let restored = w
        .notebooks
        .restore_version(w.owner, page, 1, &w.collab)
        .await
        .unwrap();
    assert_eq!(w.stored(page).await, "Bir");
    assert_eq!(restored.version, 4);
    assert_eq!(restored.preview, "Bir");
    let texts: Vec<String> = w
        .notebooks
        .versions(w.owner, page, None)
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.preview)
        .collect();
    assert_eq!(texts, vec!["Bir", "Bir iki üç", "Bir iki", "Bir"]);

    // Restoring the newest content back is just another change.
    w.notebooks
        .restore_version(w.owner, page, 3, &w.collab)
        .await
        .unwrap();
    assert_eq!(w.stored(page).await, "Bir iki üç");
    // Restoring what the page already holds changes nothing.
    let same = w
        .notebooks
        .restore_version(w.owner, page, 3, &w.collab)
        .await
        .unwrap();
    assert_eq!(same.version, 5);
}

#[tokio::test]
async fn restoring_needs_edit_and_nothing_leaks_to_others() {
    let w = World::new().await;
    let page = w.page().await;
    w.version(page, "gizli metin").await;
    let viewer = Uuid::now_v7();
    let stranger = Uuid::now_v7();
    w.grant(RelationTuple::new(page, Relation::Viewer, viewer))
        .await;

    // A viewer reads the history but cannot restore.
    assert_eq!(
        w.notebooks
            .versions(viewer, page, None)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(w.notebooks.version(viewer, page, 1).await.is_ok());
    assert!(matches!(
        w.notebooks
            .restore_version(viewer, page, 1, &w.collab)
            .await,
        Err(AppError::Forbidden)
    ));

    // Someone who cannot see the page learns nothing: not that it exists, not how many
    // versions it has, not whether a version number is real.
    for version in [1, 99] {
        let read = w.notebooks.version(stranger, page, version).await;
        let restore = w
            .notebooks
            .restore_version(stranger, page, version, &w.collab)
            .await;
        assert!(matches!(read, Err(AppError::NotFound)), "{read:?}");
        assert!(matches!(restore, Err(AppError::NotFound)), "{restore:?}");
    }
    assert!(matches!(
        w.notebooks.versions(stranger, page, None).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.notebooks.versions(stranger, Uuid::now_v7(), None).await,
        Err(AppError::NotFound)
    ));
    // Nothing was written by the refused attempts.
    assert_eq!(w.stored(page).await, "gizli metin");
    assert!(
        repo::page_updates(w.db.pool(), page)
            .await
            .unwrap()
            .is_empty()
    );
}
