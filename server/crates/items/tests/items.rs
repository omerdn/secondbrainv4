//! Items CRUD, archive, trash and purge (T1.2.1, PRD N1–N4, V6, V9) against a real Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation as R, RelationTuple};
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use chrono::{DateTime, Duration, TimeZone, Utc};
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::{EmailSettings, IdentityService};
use items::http::{ItemsHttpState, router};
use items::{ItemsAccessContext, ItemsService, ListState, NewItem, TRASH_RETENTION};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap()
}

struct World {
    db: TestDb,
    clock: FixedClock,
    access: AccessEngine<ItemsAccessContext>,
    items: ItemsService,
    router: Router,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(start());
        let shared = Arc::new(clock.clone());
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(db.pool().clone(), shared.clone(), access.clone());
        let identity = IdentityService::new(
            db.pool().clone(),
            shared,
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        let router = identity::http::router(identity.clone()).merge(router(ItemsHttpState {
            items: items.clone(),
            identity,
        }));
        Self {
            db,
            clock,
            access,
            items,
            router,
        }
    }

    fn task(title: &str) -> NewItem {
        NewItem {
            kind: "task".into(),
            title: title.into(),
            description: None,
        }
    }

    async fn grant(&self, tuple: RelationTuple) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.access.grant(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    async fn call(&self, method: Method, uri: &str, token: &str, body: Option<Value>) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        self.router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap()
    }

    async fn user(&self, email: &str) -> (Uuid, String) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "email": email,
                            "password": "correct horse",
                            "display_name": "Test",
                            "client": "native",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = json_body(response).await;
        (
            body["user"]["id"].as_str().unwrap().parse().unwrap(),
            body["token"].as_str().unwrap().to_owned(),
        )
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn ids(items: &[items::Item]) -> Vec<Uuid> {
    items.iter().map(|i| i.id).collect()
}

// --- service --------------------------------------------------------------------------------

#[tokio::test]
async fn create_makes_the_creator_the_owner() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let item = w.items.create(ali, World::task("Buy milk")).await.unwrap();
    assert_eq!(item.owner, items::Owner::User { user_id: ali });
    assert_eq!(item.created_at, start());
    assert_eq!(item.last_touched_at, start());
    assert_eq!(w.items.get(ali, item.id).await.unwrap(), item);
    // N1 + access: the owner has every permission; nobody else sees it (V9: 404).
    assert!(matches!(
        w.items.get(Uuid::now_v7(), item.id).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn editing_needs_edit_and_deleting_is_the_owners_alone() {
    let w = World::new().await;
    let (ali, ayse, can) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let item = w.items.create(ali, World::task("Plan")).await.unwrap();
    w.grant(RelationTuple::new(item.id, R::Assignee, ayse))
        .await;
    w.grant(RelationTuple::new(item.id, R::Watcher, can)).await;

    w.clock.set(start() + Duration::hours(1));
    let changes = items::ItemChanges {
        title: Some("Plan the trip".into()),
        description: Some(Some("Book tickets".into())),
    };
    let updated = w.items.update(ayse, item.id, changes).await.unwrap();
    assert_eq!(updated.title, "Plan the trip");
    assert_eq!(updated.description.as_deref(), Some("Book tickets"));
    assert_eq!(updated.updated_at, start() + Duration::hours(1));

    // Watcher: view only (403). V6: the assignee may edit but not delete or archive.
    let no_title = items::ItemChanges {
        title: Some("x".into()),
        description: None,
    };
    assert!(matches!(
        w.items.update(can, item.id, no_title).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        w.items.set_trashed(ayse, item.id, true).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        w.items.set_archived(ayse, item.id, true).await,
        Err(AppError::Forbidden)
    ));
    // Clearing the description.
    let cleared = w
        .items
        .update(
            ali,
            item.id,
            items::ItemChanges {
                title: None,
                description: Some(None),
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.description, None);
    assert_eq!(cleared.title, "Plan the trip");
}

#[tokio::test]
async fn archive_is_a_reversible_state_separate_from_the_trash() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let a = w.items.create(ali, World::task("A")).await.unwrap();
    w.clock.set(start() + Duration::minutes(1));
    let b = w.items.create(ali, World::task("B")).await.unwrap();

    assert_eq!(
        ids(&w.items.list(ali, ListState::Active).await.unwrap()),
        [b.id, a.id]
    );
    let archived = w.items.set_archived(ali, a.id, true).await.unwrap();
    assert!(archived.archived_at.is_some());
    assert_eq!(
        ids(&w.items.list(ali, ListState::Active).await.unwrap()),
        [b.id]
    );
    assert_eq!(
        ids(&w.items.list(ali, ListState::Archived).await.unwrap()),
        [a.id]
    );
    let back = w.items.set_archived(ali, a.id, false).await.unwrap();
    assert_eq!(back.archived_at, None);
    assert_eq!(w.items.list(ali, ListState::Active).await.unwrap().len(), 2);
}

#[tokio::test]
async fn trash_restore_and_purge_after_30_days() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let item = w.items.create(ali, World::task("Old")).await.unwrap();
    let page = w
        .items
        .create(
            ali,
            NewItem {
                kind: "page".into(),
                title: "Notes".into(),
                description: None,
            },
        )
        .await
        .unwrap();
    w.grant(RelationTuple::new(item.id, R::Watcher, ayse)).await;
    w.grant(RelationTuple::new(page.id, R::LinkedTo, item.id))
        .await;

    let trashed = w.items.set_trashed(ali, item.id, true).await.unwrap();
    assert_eq!(trashed.deleted_at, Some(start()));
    assert!(
        w.items
            .list(ali, ListState::Active)
            .await
            .unwrap()
            .iter()
            .all(|i| i.id != item.id)
    );
    assert_eq!(
        ids(&w.items.list(ali, ListState::Trash).await.unwrap()),
        [item.id]
    );
    // Only the owner sees it in the trash; others cannot restore it.
    assert!(
        w.items
            .list(ayse, ListState::Trash)
            .await
            .unwrap()
            .is_empty()
    );
    // Trashed items cannot be edited or archived until restored.
    let edit = items::ItemChanges {
        title: Some("x".into()),
        description: None,
    };
    assert!(matches!(
        w.items.update(ali, item.id, edit).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        w.items.set_archived(ali, item.id, true).await,
        Err(AppError::Conflict(_))
    ));

    // Trashing again keeps the first time; restore clears it.
    w.clock.set(start() + Duration::days(5));
    assert_eq!(
        w.items
            .set_trashed(ali, item.id, true)
            .await
            .unwrap()
            .deleted_at,
        Some(start())
    );
    assert_eq!(
        w.items
            .set_trashed(ali, item.id, false)
            .await
            .unwrap()
            .deleted_at,
        None
    );
    w.items.set_trashed(ali, item.id, true).await.unwrap();
    let trashed_at = start() + Duration::days(5);

    // N4: not purged before 30 days, purged after (Clock injected).
    assert_eq!(
        w.items
            .purge_expired(trashed_at + TRASH_RETENTION - Duration::seconds(1))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        w.items
            .purge_expired(trashed_at + TRASH_RETENTION + Duration::seconds(1))
            .await
            .unwrap(),
        1
    );
    assert!(matches!(
        w.items.get(ali, item.id).await,
        Err(AppError::NotFound)
    ));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE id = $1")
        .bind(item.id)
        .fetch_one(w.db.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0);
    // Access went with it: the page linked to it no longer lends Ayşe access.
    let permissions = w.access.permissions();
    assert!(
        !permissions
            .check(w.db.pool(), ayse, page.id, access::Perms::VIEW)
            .await
            .unwrap()
    );
    assert!(
        permissions
            .check(w.db.pool(), ali, page.id, access::Perms::DELETE)
            .await
            .unwrap()
    );
}

// --- endpoints ------------------------------------------------------------------------------

#[tokio::test]
async fn endpoints() {
    let w = World::new().await;
    let (_, ali) = w.user("ali@example.test").await;
    let (_, can) = w.user("can@example.test").await;

    let response = w
        .call(
            Method::POST,
            "/items",
            &ali,
            Some(json!({ "kind": "task", "title": "Buy milk" })),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let item = json_body(response).await;
    let id = item["id"].as_str().unwrap().to_owned();
    assert_eq!(item["owner"]["type"], "user");
    assert_eq!(item["archived_at"], Value::Null);

    let bad = w
        .call(
            Method::POST,
            "/items",
            &ali,
            Some(json!({ "kind": "spaceship", "title": "" })),
        )
        .await;
    assert_eq!(bad.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let uri = format!("/items/{id}");
    assert_eq!(
        w.call(Method::GET, &uri, &ali, None).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        w.call(Method::GET, &uri, &can, None).await.status(),
        StatusCode::NOT_FOUND
    );
    let patched = w
        .call(
            Method::PATCH,
            &uri,
            &ali,
            Some(json!({ "title": "Buy oat milk" })),
        )
        .await;
    assert_eq!(json_body(patched).await["title"], "Buy oat milk");
    assert_eq!(
        w.call(Method::PATCH, &uri, &can, Some(json!({ "title": "x" })))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    let archived = w
        .call(Method::POST, &format!("{uri}/archive"), &ali, None)
        .await;
    assert_ne!(json_body(archived).await["archived_at"], Value::Null);
    let list = w
        .call(Method::GET, "/items?state=archived", &ali, None)
        .await;
    assert_eq!(json_body(list).await.as_array().unwrap().len(), 1);
    w.call(Method::POST, &format!("{uri}/unarchive"), &ali, None)
        .await;

    assert_eq!(
        w.call(Method::DELETE, &uri, &can, None).await.status(),
        StatusCode::NOT_FOUND
    );
    let trashed = w.call(Method::DELETE, &uri, &ali, None).await;
    assert_eq!(trashed.status(), StatusCode::OK);
    let trash = json_body(w.call(Method::GET, "/items?state=trash", &ali, None).await).await;
    assert_eq!(trash.as_array().unwrap().len(), 1);
    let active = json_body(w.call(Method::GET, "/items", &ali, None).await).await;
    assert!(active.as_array().unwrap().is_empty());
    let restored = w
        .call(Method::POST, &format!("{uri}/restore"), &ali, None)
        .await;
    assert_eq!(json_body(restored).await["deleted_at"], Value::Null);
}
