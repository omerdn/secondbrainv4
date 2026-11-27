//! Data export (T4.3.2, PRD HS3, V9) against a real Postgres 17. Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation as R, RelationTuple};
use app::exports::{DataExports, ExportFile, ExportStatus, LINK_LIFETIME, Sources};
use chrono::{DateTime, Duration, TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{ItemsAccessContext, ItemsService, NewItem};
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
    exports: DataExports,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let clock = FixedClock::new(start());
        let shared: kernel::SharedClock = Arc::new(clock.clone());
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(pool.clone(), shared.clone(), access.clone());
        let identity = IdentityService::new(
            pool.clone(),
            shared.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        let sources = Sources {
            identity: identity.clone(),
            items: items.clone(),
            tasks: tasks::TasksService::new(
                pool.clone(),
                shared.clone(),
                access.permissions(),
                items.clone(),
            ),
            routines: routines::RoutinesService::new(
                pool.clone(),
                shared.clone(),
                access.permissions(),
                items.clone(),
            ),
            goals: goals::GoalsService::new(
                pool.clone(),
                shared.clone(),
                access.permissions(),
                items.clone(),
            ),
            trackers: tracking::TrackerEntries::new(
                pool.clone(),
                shared.clone(),
                access.permissions(),
                items.clone(),
                identity.clone(),
            ),
            notebooks: notebooks::NotebooksService::new(
                pool.clone(),
                shared.clone(),
                access.clone(),
                items.clone(),
            ),
            sessions: sessions::WorkSessions::new(
                pool.clone(),
                shared.clone(),
                access.permissions(),
                realtime::Notifier::default(),
            ),
        };
        let exports = DataExports::new(pool, shared, sources);
        Self {
            db,
            clock,
            access,
            items,
            identity,
            exports,
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

    /// Requests an export, runs its job and returns both files.
    async fn export(&self, user: Uuid) -> (serde_json::Value, String, String) {
        let requested = self.exports.request(user).await.unwrap();
        assert_eq!(requested.status, ExportStatus::Pending);
        let mut worker = jobs::Worker::new(jobs::WorkerConfig::default());
        let mut scheduler = jobs::Scheduler::new(jobs::SchedulerConfig::default());
        app::exports::register(self.exports.clone(), &mut worker, &mut scheduler).unwrap();
        worker.run_batch(self.db.pool()).await.unwrap();
        let ready = self.exports.latest(user).await.unwrap().unwrap();
        assert_eq!(ready.status, ExportStatus::Ready);
        let path = ready.json_path.unwrap();
        let token = path.split('/').nth(2).unwrap().to_owned();
        let json = self
            .exports
            .download(&token, ExportFile::Json)
            .await
            .unwrap();
        let markdown = self
            .exports
            .download(&token, ExportFile::Markdown)
            .await
            .unwrap();
        (serde_json::from_str(&json).unwrap(), markdown, token)
    }
}

fn titles(export: &serde_json::Value) -> Vec<String> {
    let mut titles: Vec<String> = export["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["item"]["title"].as_str().unwrap().to_owned())
        .collect();
    titles.sort();
    titles
}

fn entry<'a>(export: &'a serde_json::Value, title: &str) -> &'a serde_json::Value {
    export["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["item"]["title"] == title)
        .unwrap()
}

#[tokio::test]
async fn hs3_an_export_has_json_and_markdown_of_what_the_user_owns() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    w.item(ada, "task", "Rapor yaz").await;
    w.item(ada, "routine", "Su iç").await;
    let (json, markdown, _) = w.export(ada).await;
    assert_eq!(json["format"], "routune-export");
    assert_eq!(json["profile"]["email"], "ada@example.com");
    assert_eq!(titles(&json), ["Rapor yaz", "Su iç"]);
    assert!(markdown.starts_with("# Routune verilerin"));
    assert!(markdown.contains("## Görevler\n\n### Rapor yaz"));
    assert!(markdown.contains("## Rutinler\n\n### Su iç"));
    assert!(markdown.contains("- E-posta: ada@example.com"));
}

#[tokio::test]
async fn v9_an_export_never_shows_what_the_user_cannot_see() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    // Ada's notebook is shared with Bob; its page links to Ada's private task.
    let secret = w.item(ada, "task", "Gizli proje").await;
    let notebook = w.item(ada, "notebook", "Ortak defter").await;
    let page = w.item(ada, "page", "Toplantı notu").await;
    w.grant(RelationTuple::new(page, R::InNotebook, notebook))
        .await;
    w.grant(RelationTuple::new(notebook, R::Editor, bob)).await;
    w.items.link(ada, page, secret).await.unwrap();
    w.item(ada, "task", "Ada'nın özel işi").await;

    let (bobs, markdown, _) = w.export(bob).await;
    assert_eq!(titles(&bobs), ["Ortak defter", "Toplantı notu"]);
    assert_eq!(
        entry(&bobs, "Toplantı notu")["links"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "the hidden link is not even mentioned"
    );
    let raw = bobs.to_string();
    for hidden in ["Gizli proje", "özel işi", &secret.to_string()] {
        assert!(!raw.contains(hidden), "{hidden} leaked into the JSON");
        assert!(
            !markdown.contains(hidden),
            "{hidden} leaked into the Markdown"
        );
    }

    // Ada sees the link.
    let (adas, _, _) = w.export(ada).await;
    let links = entry(&adas, "Toplantı notu")["links"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["item"]["title"], "Gizli proje");
}

#[tokio::test]
async fn hs3_the_link_works_for_a_day_and_only_for_its_files() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    w.item(ada, "task", "Rapor").await;
    let (_, _, token) = w.export(ada).await;
    // Bob has no export; Ada's is not his.
    assert!(w.exports.latest(bob).await.unwrap().is_none());
    assert!(matches!(
        w.exports.download("not-a-token", ExportFile::Json).await,
        Err(AppError::NotFound)
    ));
    w.clock.advance(LINK_LIFETIME - Duration::seconds(1));
    assert!(w.exports.download(&token, ExportFile::Json).await.is_ok());
    w.clock.advance(Duration::seconds(1));
    assert!(matches!(
        w.exports.download(&token, ExportFile::Markdown).await,
        Err(AppError::NotFound)
    ));
    assert!(w.exports.latest(ada).await.unwrap().is_none());
    // The daily cleanup removes it, contents and all.
    assert_eq!(w.exports.delete_expired().await.unwrap(), 1);
}

#[tokio::test]
async fn hs3_a_second_request_while_one_is_built_returns_the_same() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let first = w.exports.request(ada).await.unwrap();
    let second = w.exports.request(ada).await.unwrap();
    assert_eq!(first.id, second.id);
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'exports.build'")
        .fetch_one(w.db.pool())
        .await
        .unwrap();
    assert_eq!(queued, 1);
}
