//! Global search (T2.7.5, PRD AR1, AR2, E2) against a real Postgres 17. Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation as R, RelationTuple};
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{ItemsAccessContext, ItemsService, NewItem, NewTag};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use notebooks::{CreatePageRequest, NotebooksService};
use search::{SearchField, SearchService};
use uuid::Uuid;

struct World {
    db: TestDb,
    identity: IdentityService,
    access: AccessEngine<ItemsAccessContext>,
    items: ItemsService,
    notebooks: NotebooksService,
    search: SearchService,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let clock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 27, 9, 0, 0).unwrap(),
        ));
        let identity = IdentityService::new(
            pool.clone(),
            clock.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "http://localhost:8081".into(),
            },
        );
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(pool.clone(), clock.clone(), access.clone());
        let notebooks = NotebooksService::new(pool, clock, access.clone(), items.clone());
        let search = SearchService::new(
            items.clone(),
            vec![Arc::new(items.clone()), Arc::new(notebooks.clone())],
        );
        Self {
            db,
            identity,
            access,
            items,
            notebooks,
            search,
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

    async fn item(&self, owner: Uuid, title: &str, description: Option<&str>) -> Uuid {
        self.items
            .create(
                owner,
                NewItem {
                    kind: "task".into(),
                    title: title.into(),
                    description: description.map(Into::into),
                },
            )
            .await
            .unwrap()
            .id
    }

    /// A page whose latest snapshot holds `text`; search reads only snapshots.
    async fn page(&self, owner: Uuid, title: &str, text: &str) -> Uuid {
        let notebook = self
            .notebooks
            .ensure_default(owner, "Defterim")
            .await
            .unwrap();
        let page = self
            .notebooks
            .create_page(
                owner,
                notebook,
                CreatePageRequest {
                    title: title.into(),
                },
            )
            .await
            .unwrap()
            .item
            .id;
        sqlx::query(
            "INSERT INTO page_snapshots (page_id, version, state, plain_text, created_at)
             VALUES ($1, 1, '\\x00', $2, now())",
        )
        .bind(page)
        .bind(text)
        .execute(self.db.pool())
        .await
        .unwrap();
        page
    }

    async fn tag(&self, owner: Uuid, item: Uuid, name: &str) {
        let tag = self
            .items
            .create_tag(
                owner,
                NewTag {
                    name: name.into(),
                    color: None,
                },
            )
            .await
            .unwrap();
        self.items.attach_tag(owner, item, tag.id).await.unwrap();
    }

    async fn share(&self, item: Uuid, with: Uuid) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.access
            .grant(&mut tx, RelationTuple::new(item, R::Watcher, with))
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    async fn found(&self, user: Uuid, query: &str) -> Vec<(Uuid, SearchField)> {
        self.search
            .search(user, query)
            .await
            .unwrap()
            .into_iter()
            .map(|hit| (hit.item.id, hit.field))
            .collect()
    }

    async fn ids(&self, user: Uuid, query: &str) -> Vec<Uuid> {
        self.found(user, query)
            .await
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }
}

#[tokio::test]
async fn ar1_titles_descriptions_tags_and_page_contents_are_searched() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let titled = w.item(ada, "Gitar kursu", None).await;
    let described = w
        .item(ada, "Haftalık plan", Some("Gitar çalışması da var"))
        .await;
    let tagged = w.item(ada, "Akor listesi", None).await;
    w.tag(ada, tagged, "gitarist").await;
    let page = w
        .page(ada, "Notlar", "Bugün yeni bir gitar teli taktım.")
        .await;
    let found = w.found(ada, "gitar").await;
    // The title hit comes first.
    assert_eq!(found[0], (titled, SearchField::Title));
    assert!(found.contains(&(described, SearchField::Description)));
    assert!(found.contains(&(tagged, SearchField::Tag)));
    assert!(found.contains(&(page, SearchField::Content)));
    // A page excerpt comes with the hit.
    let hits = w.search.search(ada, "tel").await.unwrap();
    assert_eq!(hits[0].item.id, page);
    assert!(hits[0].snippet.as_deref().unwrap().contains("gitar teli"));
}

#[tokio::test]
async fn ar1_turkish_letters_and_suffixes_match() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let books = w.item(ada, "Kitapları okumak", None).await;
    let light = w.item(ada, "Işık tasarımı", None).await;
    let city = w.item(ada, "İstanbul notları", None).await;
    let page = w.page(ada, "Bahçe", "Balkondaki çiçekler açtı.").await;
    let guitar = w.item(ada, "Gitar kursu", None).await;

    // Suffixes: the Turkish stemmer.
    assert_eq!(w.ids(ada, "kitap").await, [books]);
    assert_eq!(w.ids(ada, "kitaplar").await, [books]);
    // Dotted, dotless and folded letters are the same.
    for query in ["ışık", "isik", "IŞIK", "işık"] {
        assert_eq!(w.ids(ada, query).await, [light], "{query}");
    }
    for query in ["istanbul", "İSTANBUL", "Istanbul"] {
        assert_eq!(w.ids(ada, query).await, [city], "{query}");
    }
    assert_eq!(w.ids(ada, "cicek").await, [page]);
    assert_eq!(w.ids(ada, "çiçeği").await, [page]);
    // A typo and the start of a word find a title.
    assert_eq!(w.ids(ada, "gitr kursu").await, [guitar]);
    assert_eq!(w.ids(ada, "kita").await, [books]);
}

#[tokio::test]
async fn ar2_only_what_the_caller_can_view_is_found() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let bob = w.user("bob@example.com").await;
    w.item(bob, "Gizli plan", Some("gizli")).await;
    w.page(bob, "Bob'un notu", "gizli bir not").await;
    let shared = w.item(bob, "Ortak gizli liste", None).await;
    let trashed = w.item(ada, "Gizli çöp", None).await;
    w.items.set_trashed(ada, trashed, true).await.unwrap();
    let archived = w.item(ada, "Gizli arşiv", None).await;
    w.items.set_archived(ada, archived, true).await.unwrap();

    assert_eq!(w.ids(ada, "gizli").await, [archived]);
    w.share(shared, ada).await;
    let mut ids = w.ids(ada, "gizli").await;
    ids.sort();
    let mut expected = vec![archived, shared];
    expected.sort();
    assert_eq!(ids, expected);
}

#[tokio::test]
async fn e2_nobody_elses_personal_tag_is_searched() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let bob = w.user("bob@example.com").await;
    let shared = w.item(ada, "Ortak liste", None).await;
    w.share(shared, bob).await;
    // Bob's own tag on the shared item: only Bob finds it by that name.
    w.tag(bob, shared, "zümrüt").await;
    assert!(w.ids(ada, "zümrüt").await.is_empty());
    assert_eq!(w.found(bob, "zumrut").await, [(shared, SearchField::Tag)]);
}

#[tokio::test]
async fn queries_are_checked() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    for bad in ["", "   ", &"a".repeat(101)] {
        assert!(matches!(
            w.search.search(ada, bad).await,
            Err(AppError::Validation(_))
        ));
    }
    // Operators in the query are only text.
    assert!(
        w.search
            .search(ada, "a & | ! ' \" (")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn index_expressions_survive_maintenance() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    w.item(ada, "Kitap okuma", Some("Haftada iki kitap")).await;
    w.page(ada, "Notlar", "Çiçekler açtı").await;
    // ANALYZE and REINDEX evaluate the expressions with a restricted search_path.
    for statement in [
        "ANALYZE items",
        "ANALYZE page_snapshots",
        "REINDEX INDEX items_search_idx",
        "REINDEX INDEX items_title_trgm_idx",
        "REINDEX INDEX page_snapshots_search_idx",
    ] {
        sqlx::query(statement).execute(w.db.pool()).await.unwrap();
    }
    assert_eq!(w.ids(ada, "kitaplar").await.len(), 1);
}
