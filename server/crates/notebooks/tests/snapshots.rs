//! Page compaction and plain text (T2.5.3, PRD Y3) against a real Postgres 17. Requires Docker
//! (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{DateTime, TimeZone, Utc};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService};
use kernel::FixedClock;
use notebooks::snapshots::{self, Compacted};
use notebooks::{CreateNotebookRequest, CreatePageRequest, NotebooksService, repo};
use uuid::Uuid;
use yrs::{Doc, GetString, ReadTxn, Text, Transact};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap()
}

struct World {
    db: TestDb,
    notebooks: NotebooksService,
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
        let notebooks = NotebooksService::new(db.pool().clone(), clock, access, items);
        Self {
            db,
            notebooks,
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

    async fn stored(&self, page: Uuid) -> String {
        let doc = snapshots::load(self.db.pool(), page).await.unwrap();
        let text = doc.get_or_insert_text("content");
        text.get_string(&doc.transact())
    }

    async fn pending(&self, page: Uuid) -> usize {
        repo::page_updates(self.db.pool(), page)
            .await
            .unwrap()
            .len()
    }
}

#[tokio::test]
async fn y3_compaction_keeps_the_document_exactly() {
    let w = World::new().await;
    let page = w.page().await;
    for chunk in ["Bir. ", "İki. ", "Üç."] {
        w.write(page, chunk).await;
    }
    let before = w.stored(page).await;
    assert_eq!(before, "Bir. İki. Üç.");
    assert_eq!(
        snapshots::compact(w.db.pool(), page, now()).await.unwrap(),
        Compacted::Snapshot {
            version: 1,
            updates: 3
        }
    );
    assert_eq!(w.stored(page).await, before);
    assert_eq!(w.pending(page).await, 0);
    // The writer goes on; the next snapshot holds the old and the new.
    w.write(page, " Dört.").await;
    assert_eq!(w.stored(page).await, "Bir. İki. Üç. Dört.");
    assert_eq!(
        snapshots::compact(w.db.pool(), page, now()).await.unwrap(),
        Compacted::Snapshot {
            version: 2,
            updates: 1
        }
    );
    assert_eq!(w.stored(page).await, "Bir. İki. Üç. Dört.");
}

#[tokio::test]
async fn compaction_can_run_again_and_at_once() {
    let w = World::new().await;
    let page = w.page().await;
    for i in 0..20 {
        w.write(page, &format!("{i},")).await;
    }
    let expected = w.stored(page).await;
    // Several jobs for the same page at the same moment: one snapshot, nothing lost.
    let runs = (0..4).map(|_| snapshots::compact(w.db.pool(), page, now()));
    let results = futures_util::future::join_all(runs).await;
    let made = results
        .iter()
        .filter(|r| matches!(r.as_ref().unwrap(), Compacted::Snapshot { .. }))
        .count();
    assert_eq!(made, 1);
    assert_eq!(w.stored(page).await, expected);
    // A retry finds nothing to do and changes nothing.
    assert_eq!(
        snapshots::compact(w.db.pool(), page, now()).await.unwrap(),
        Compacted::Nothing
    );
    assert_eq!(w.stored(page).await, expected);
}

#[tokio::test]
async fn plain_text_follows_each_snapshot() {
    let w = World::new().await;
    let page = w.page().await;
    let other = w.page().await;
    w.write(page, "ilk hâli").await;
    assert_eq!(
        snapshots::compact_pending(w.db.pool(), now())
            .await
            .unwrap(),
        1
    );
    let text = |rows: Vec<(Uuid, String)>| rows.into_iter().map(|(_, t)| t).collect::<Vec<_>>();
    assert_eq!(
        text(
            repo::plain_texts(w.db.pool(), &[page, other])
                .await
                .unwrap()
        ),
        vec!["ilk hâli".to_owned()]
    );
    w.write(page, " ve sonrası").await;
    snapshots::compact_pending(w.db.pool(), now())
        .await
        .unwrap();
    assert_eq!(
        text(repo::plain_texts(w.db.pool(), &[page]).await.unwrap()),
        vec!["ilk hâli ve sonrası".to_owned()]
    );
    // Nothing pending: the sweep does nothing.
    assert_eq!(
        snapshots::compact_pending(w.db.pool(), now())
            .await
            .unwrap(),
        0
    );
}
