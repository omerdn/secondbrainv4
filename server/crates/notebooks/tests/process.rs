//! Process pages and entry notes on them (T2.5.5, PRD S1–S3, T3, V2) against a real Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{CreateItemRequest, ItemsAccessContext, ItemsService};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock, PageRequest};
use notebooks::collab::Collab;
use notebooks::process::{ProcessPages, StatusLine};
use notebooks::{NotebooksService, snapshots};
use rust_decimal::Decimal;
use tracking::{
    Aggregation, CreateEntryRequest, CreateTrackerRequest, Direction, EntryNoted, Period,
    TrackerEntries,
};
use uuid::Uuid;
use yrs::types::xml::{XmlFragment, XmlOut};
use yrs::updates::decoder::Decode;
use yrs::{GetString, ReadTxn, Text, Transact, Update};

fn clock() -> kernel::SharedClock {
    Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap(),
    ))
}

struct World {
    db: TestDb,
    identity: IdentityService,
    items: ItemsService,
    notebooks: NotebooksService,
    collab: Collab,
    process: ProcessPages,
    entries: TrackerEntries,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let identity = IdentityService::new(
            pool.clone(),
            clock(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "http://localhost:8081".into(),
            },
        );
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(pool.clone(), clock(), access.clone());
        let notebooks = NotebooksService::new(pool.clone(), clock(), access.clone(), items.clone());
        let collab = Collab::new(pool.clone(), clock());
        let process = ProcessPages::new(
            pool.clone(),
            notebooks.clone(),
            (items.clone(), identity.clone()),
            collab.clone(),
            &access,
        );
        let entries = TrackerEntries::new(
            pool,
            clock(),
            access.permissions(),
            items.clone(),
            identity.clone(),
        )
        .with_notes(Arc::new(process.clone()));
        Self {
            db,
            identity,
            items,
            notebooks,
            collab,
            process,
            entries,
        }
    }

    async fn user(&self, email: &str) -> Uuid {
        let request = RegisterRequest {
            email: email.into(),
            password: "correct horse".into(),
            display_name: "Ada".into(),
            client: ClientKind::Native,
            device: None,
        }
        .validate()
        .unwrap();
        let (user, _) = self.identity.register(request).await.unwrap();
        user.id.as_uuid()
    }

    async fn tracker(&self, owner: Uuid) -> Uuid {
        self.entries
            .create_tracker(
                owner,
                CreateTrackerRequest {
                    title: "Gitar".into(),
                    description: None,
                    unit: "dk".into(),
                    target_value: Decimal::from(30),
                    period: Period::Day,
                    aggregation: Aggregation::Sum,
                    direction: Direction::Increase,
                },
            )
            .await
            .unwrap()
            .item
            .id
    }

    async fn add(&self, user: Uuid, tracker: Uuid, note: Option<&str>) -> Uuid {
        self.entries
            .add(
                user,
                tracker,
                CreateEntryRequest {
                    value: Decimal::from(20),
                    occurred_at: None,
                    note: note.map(Into::into),
                },
            )
            .await
            .unwrap()
            .id
    }

    /// The `tracking.entry_noted` events published so far, as the dispatcher would deliver them.
    async fn noted(&self) -> Vec<EntryNoted> {
        let payloads: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT payload FROM outbox WHERE event_type = 'tracking.entry_noted' ORDER BY id",
        )
        .fetch_all(self.db.pool())
        .await
        .unwrap();
        payloads
            .into_iter()
            .map(|p| serde_json::from_value(p).unwrap())
            .collect()
    }

    async fn deliver(&self) {
        for event in self.noted().await {
            self.process.add_note(&event, &self.entries).await.unwrap();
        }
    }

    async fn notes(&self, user: Uuid, tracker: Uuid) -> Vec<Option<String>> {
        let page = PageRequest::from_params(kernel::PageParams::default()).unwrap();
        self.entries
            .list(user, tracker, (None, None), &page)
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|e| e.note)
            .collect()
    }
}

#[tokio::test]
async fn s1_t3_the_first_entry_note_creates_the_page_with_a_dated_line() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let tracker = w.tracker(ada).await;
    w.add(ada, tracker, None).await;
    // No note, no page.
    assert!(w.noted().await.is_empty());
    assert!(matches!(
        w.process.get(ada, tracker).await,
        Err(AppError::NotFound)
    ));

    w.add(ada, tracker, Some("Barre'da kaldım")).await;
    w.deliver().await;
    // Delivered twice (a retry): still one line.
    w.deliver().await;
    let process = w.process.get(ada, tracker).await.unwrap();
    assert_eq!(process.page.item.title, "Gitar");
    assert_eq!(process.page.process_of_item_id, Some(tracker));
    assert_eq!(
        process.latest,
        Some(StatusLine {
            text: "Barre'da kaldım".into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 11, 26),
        })
    );
    // S2: in the owner's default notebook.
    let notebooks = w.notebooks.notebooks(ada).await.unwrap();
    let default = notebooks.iter().find(|n| n.is_default).unwrap();
    assert_eq!(process.page.notebook_id, default.item.id);
    let doc = snapshots::load(w.db.pool(), process.page.item.id)
        .await
        .unwrap();
    assert_eq!(snapshots::plain_text(&doc), "26.11.2026 · Barre'da kaldım");
    // A second note is a second line on the same page.
    w.add(ada, tracker, Some("Akorlar oturdu")).await;
    w.deliver().await;
    let again = w.process.get(ada, tracker).await.unwrap();
    assert_eq!(again.page.item.id, process.page.item.id);
    assert_eq!(again.latest.unwrap().text, "Akorlar oturdu");
    // Same clock, so the order of the entries is arbitrary.
    let mut notes = w.notes(ada, tracker).await;
    notes.sort();
    assert_eq!(
        notes,
        vec![
            None,
            Some("Akorlar oturdu".into()),
            Some("Barre'da kaldım".into()),
        ]
    );
}

#[tokio::test]
async fn t3_a_note_corrected_on_the_page_is_corrected_on_the_entry() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let tracker = w.tracker(ada).await;
    w.add(ada, tracker, Some("Barre da kaldim")).await;
    w.deliver().await;
    let page = w.process.get(ada, tracker).await.unwrap().page.item.id;

    // Someone fixes the line in the editor (here: the server applies the same change).
    w.collab
        .change(page, ada, |doc| {
            let scratch = snapshots::rebuild(Some(&snapshots::encode(doc)), &[]).unwrap();
            let before = scratch.transact().state_vector();
            let root = scratch.get_or_insert_xml_fragment("default");
            {
                let mut txn = scratch.transact_mut();
                let Some(XmlOut::Element(line)) = root.get(&txn, 0) else {
                    panic!("no line")
                };
                let Some(XmlOut::Text(text)) = line.get(&txn, 0) else {
                    panic!("no text")
                };
                let all = text.get_string(&txn);
                let len = u32::try_from(all.len()).unwrap();
                text.remove_range(&mut txn, 0, len);
                text.insert(&mut txn, 0, "26.11.2026 · Barre'da kaldım");
            }
            scratch.transact().encode_state_as_update_v1(&before)
        })
        .await
        .unwrap();
    assert_eq!(
        w.notes(ada, tracker).await,
        vec![Some("Barre'da kaldım".into())]
    );
    // Deleting the line deletes the note: the line is its only copy.
    w.collab
        .change(page, ada, |doc| {
            let scratch = snapshots::rebuild(Some(&snapshots::encode(doc)), &[]).unwrap();
            let before = scratch.transact().state_vector();
            let root = scratch.get_or_insert_xml_fragment("default");
            root.remove_range(&mut scratch.transact_mut(), 0, 1);
            scratch.transact().encode_state_as_update_v1(&before)
        })
        .await
        .unwrap();
    assert_eq!(w.notes(ada, tracker).await, vec![None]);
}

#[tokio::test]
async fn s1_writing_a_first_note_opens_the_page_once() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let task = w
        .items
        .create(
            ada,
            CreateItemRequest {
                kind: "task".into(),
                title: "Taşınma".into(),
                description: None,
            }
            .validate()
            .unwrap(),
        )
        .await
        .unwrap()
        .id;
    let opened = futures_util::future::join_all((0..3).map(|_| w.process.open(ada, task))).await;
    let pages: Vec<Uuid> = opened
        .into_iter()
        .map(|p| p.unwrap().page.item.id)
        .collect();
    assert!(pages.iter().all(|p| *p == pages[0]), "{pages:?}");
    let process = w.process.get(ada, task).await.unwrap();
    assert_eq!(process.latest, None);
    // A notebook cannot have a process page.
    let notebook = process.page.notebook_id;
    assert!(matches!(
        w.process.open(ada, notebook).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn older_notes_reach_the_page_by_the_sweep() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let tracker = w.tracker(ada).await;
    w.add(ada, tracker, Some("eski not")).await;
    // Not delivered (an entry from before process pages, or a failed attempt).
    assert_eq!(w.notes(ada, tracker).await, vec![Some("eski not".into())]);
    assert_eq!(w.process.add_pending_notes(&w.entries).await.unwrap(), 1);
    assert_eq!(w.process.add_pending_notes(&w.entries).await.unwrap(), 0);
    let latest = w.process.get(ada, tracker).await.unwrap().latest.unwrap();
    assert_eq!(latest.text, "eski not");
    assert_eq!(w.notes(ada, tracker).await, vec![Some("eski not".into())]);
}

#[tokio::test]
async fn v2_the_page_follows_the_item_and_others_learn_nothing() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let bob = w.user("bob@example.com").await;
    let tracker = w.tracker(ada).await;
    w.add(ada, tracker, Some("gizli not")).await;
    w.deliver().await;
    let page = w.process.get(ada, tracker).await.unwrap().page.item.id;

    // Bob cannot see the item: no page, no latest line, no way to open one, not the page itself.
    assert!(matches!(
        w.process.get(bob, tracker).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.process.open(bob, tracker).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.notebooks.page(bob, page).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.process.get(bob, Uuid::now_v7()).await,
        Err(AppError::NotFound)
    ));
    // The page is linked to the item, so access to the item reaches it (V2).
    let links = w.items.links(ada, page).await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].item.id, tracker);
    let _ = Update::decode_v1(&w.collab.snapshot(page).await.unwrap()).unwrap();
}
