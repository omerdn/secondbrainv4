//! Notebooks and pages (T2.5.1, PRD Y1, Y2, Y5, V9) against a real Postgres 17. Requires Docker
//! (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService, Owner};
use kernel::{AppError, FixedClock};
use notebooks::{CreateNotebookRequest, CreatePageRequest, NotebooksService};
use uuid::Uuid;

struct World {
    db: TestDb,
    access: AccessEngine<ItemsAccessContext>,
    items: ItemsService,
    notebooks: NotebooksService,
    ali: Uuid,
    ayse: Uuid,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let clock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap(),
        ));
        let items = ItemsService::new(db.pool().clone(), clock.clone(), access.clone());
        let notebooks =
            NotebooksService::new(db.pool().clone(), clock, access.clone(), items.clone());
        Self {
            db,
            access,
            items,
            notebooks,
            ali: Uuid::now_v7(),
            ayse: Uuid::now_v7(),
        }
    }

    async fn notebook(&self, owner: Uuid, title: &str) -> Uuid {
        self.notebooks
            .create(
                owner,
                CreateNotebookRequest {
                    title: title.into(),
                    description: None,
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
}

fn page(title: &str) -> CreatePageRequest {
    CreatePageRequest {
        title: title.into(),
    }
}

#[tokio::test]
async fn y1_a_user_has_many_notebooks_with_the_default_first() {
    let w = World::new().await;
    let work = w.notebook(w.ali, "İş").await;
    let default = w.notebooks.ensure_default(w.ali, "Defterim").await.unwrap();
    let home = w.notebook(w.ali, "Ev").await;
    let list = w.notebooks.notebooks(w.ali).await.unwrap();
    let mut ids: Vec<Uuid> = list.iter().map(|n| n.item.id).collect();
    assert_eq!(ids[0], default);
    ids.sort();
    let mut expected = vec![default, work, home];
    expected.sort();
    assert_eq!(ids, expected);
    assert!(list[0].is_default);
    assert_eq!(list[0].item.title, "Defterim");
    assert!(!list[1].is_default && !list[2].is_default);
    // Nobody else sees them (V9).
    assert!(w.notebooks.notebooks(w.ayse).await.unwrap().is_empty());
}

#[tokio::test]
async fn the_default_notebook_is_created_once_even_when_asked_at_once() {
    let w = World::new().await;
    let calls = (0..8).map(|_| w.notebooks.ensure_default(w.ali, "Defterim"));
    let ids = futures_util::future::join_all(calls).await;
    let first = ids[0].as_ref().unwrap();
    assert!(ids.iter().all(|id| id.as_ref().unwrap() == first));
    assert_eq!(
        w.notebooks.ensure_default(w.ali, "Other").await.unwrap(),
        *first
    );
    let list = w.notebooks.notebooks(w.ali).await.unwrap();
    assert_eq!(list.iter().filter(|n| n.is_default).count(), 1);
    // Each user has their own.
    let hers = w
        .notebooks
        .ensure_default(w.ayse, "Defterim")
        .await
        .unwrap();
    assert_ne!(hers, *first);
}

#[tokio::test]
async fn y5_a_page_belongs_to_the_notebooks_owner_whoever_writes_it() {
    let w = World::new().await;
    let notebook = w.notebook(w.ali, "Ortak").await;
    w.grant(RelationTuple::new(notebook, Relation::Editor, w.ayse))
        .await;
    let created = w
        .notebooks
        .create_page(w.ayse, notebook, page("Toplantı notları"))
        .await
        .unwrap();
    assert_eq!(created.item.owner, Owner::User { user_id: w.ali });
    assert_eq!(created.item.created_by, w.ayse);
    assert_eq!(created.item.kind, "page");
    assert_eq!(created.notebook_id, notebook);
    // Both see it through the notebook; the page lists under it.
    for user in [w.ali, w.ayse] {
        assert_eq!(
            w.notebooks.page(user, created.item.id).await.unwrap(),
            created
        );
        let read = w.notebooks.notebook(user, notebook).await.unwrap();
        assert_eq!(read.pages, vec![created.clone()]);
    }
    // Editing is not owning: only Ali may delete it (Y5, V6).
    assert!(matches!(
        w.items.set_trashed(w.ayse, created.item.id, true).await,
        Err(AppError::Forbidden)
    ));
    w.items
        .set_trashed(w.ali, created.item.id, true)
        .await
        .unwrap();
    assert!(
        w.notebooks
            .notebook(w.ali, notebook)
            .await
            .unwrap()
            .pages
            .is_empty()
    );
}

#[tokio::test]
async fn v9_pages_follow_the_notebooks_visibility() {
    let w = World::new().await;
    let notebook = w.notebook(w.ali, "Gizli").await;
    let secret = w
        .notebooks
        .create_page(w.ali, notebook, page("Gizli sayfa"))
        .await
        .unwrap()
        .item
        .id;
    let stranger = Uuid::now_v7();
    assert!(matches!(
        w.notebooks.notebook(stranger, notebook).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.notebooks.page(stranger, secret).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.notebooks.create_page(stranger, notebook, page("x")).await,
        Err(AppError::NotFound)
    ));
    // Seeing a notebook is not enough to write in it.
    w.grant(RelationTuple::new(notebook, Relation::Viewer, w.ayse))
        .await;
    assert!(w.notebooks.page(w.ayse, secret).await.is_ok());
    assert!(matches!(
        w.notebooks.create_page(w.ayse, notebook, page("x")).await,
        Err(AppError::Forbidden)
    ));
}

#[tokio::test]
async fn pages_need_a_live_notebook_and_a_title() {
    let w = World::new().await;
    let notebook = w.notebook(w.ali, "Defter").await;
    assert!(matches!(
        w.notebooks.create_page(w.ali, notebook, page("  ")).await,
        Err(AppError::Validation(_))
    ));
    let task = w
        .items
        .create(
            w.ali,
            items::NewItem {
                kind: "task".into(),
                title: "Görev".into(),
                description: None,
            },
        )
        .await
        .unwrap()
        .id;
    assert!(matches!(
        w.notebooks.create_page(w.ali, task, page("x")).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.notebooks.notebook(w.ali, task).await,
        Err(AppError::NotFound)
    ));
    w.items.set_trashed(w.ali, notebook, true).await.unwrap();
    assert!(matches!(
        w.notebooks.create_page(w.ali, notebook, page("x")).await,
        Err(AppError::Conflict(_))
    ));
    assert!(w.notebooks.notebooks(w.ali).await.unwrap().is_empty());
}
