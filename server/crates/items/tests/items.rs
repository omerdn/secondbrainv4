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

// --- links (T1.2.3, B1-B3, V3, V4, V8, V9) --------------------------------------------------

fn of_kind(kind: &str, title: &str) -> NewItem {
    NewItem {
        kind: kind.into(),
        title: title.into(),
        description: None,
    }
}

#[tokio::test]
async fn v4_only_the_page_owner_links_and_must_see_the_target() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    let alis_task = w.items.create(ali, World::task("Ali's")).await.unwrap();
    let ayses_task = w.items.create(ayse, World::task("Ayse's")).await.unwrap();
    w.grant(RelationTuple::new(page.id, R::Assignee, ayse))
        .await;

    // An editor of the page may not link it.
    assert!(matches!(
        w.items.link(ayse, page.id, ayses_task.id).await,
        Err(AppError::Forbidden)
    ));
    // The owner may not link to something they cannot see, and learns nothing about it.
    assert!(matches!(
        w.items.link(ali, page.id, ayses_task.id).await,
        Err(AppError::NotFound)
    ));
    let link = w.items.link(ali, page.id, alis_task.id).await.unwrap();
    assert_eq!(link.item.id, alis_task.id);
    // Linking twice returns the same link.
    let again = w.items.link(ali, page.id, alis_task.id).await.unwrap();
    assert_eq!(again.id, link.id);
}

#[tokio::test]
async fn b1_rejects_kinds_that_cannot_be_linked() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let page = w.items.create(ali, of_kind("page", "A")).await.unwrap();
    let other = w.items.create(ali, of_kind("page", "B")).await.unwrap();
    let goal = w.items.create(ali, of_kind("goal", "G")).await.unwrap();
    let task = w.items.create(ali, World::task("T")).await.unwrap();
    for (source, target) in [(page.id, other.id), (goal.id, task.id), (task.id, page.id)] {
        assert!(matches!(
            w.items.link(ali, source, target).await,
            Err(AppError::Validation(_))
        ));
    }
    w.items.link(ali, task.id, goal.id).await.unwrap();
    w.items.link(ali, page.id, goal.id).await.unwrap();
}

#[tokio::test]
async fn v8_linking_grants_and_unlinking_drops_page_access() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    let task = w.items.create(ali, World::task("Task")).await.unwrap();
    w.grant(RelationTuple::new(task.id, R::Watcher, ayse)).await;
    assert!(matches!(
        w.items.get(ayse, page.id).await,
        Err(AppError::NotFound)
    ));

    w.items.link(ali, page.id, task.id).await.unwrap();
    assert_eq!(w.items.get(ayse, page.id).await.unwrap().id, page.id);
    // V3: inherited access cannot link or delete.
    assert!(matches!(
        w.items.unlink(ayse, page.id, task.id).await,
        Err(AppError::Forbidden)
    ));

    w.items.unlink(ali, page.id, task.id).await.unwrap();
    assert!(matches!(
        w.items.get(ayse, page.id).await,
        Err(AppError::NotFound)
    ));
    assert!(w.items.links(ali, page.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn v9_hidden_links_do_not_leak_in_any_form() {
    let w = World::new().await;
    let (ali, can) = (Uuid::now_v7(), Uuid::now_v7());
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    let hidden = w
        .items
        .create(ali, World::task("Secret title"))
        .await
        .unwrap();
    let open = w.items.create(ali, World::task("Open")).await.unwrap();
    w.items.link(ali, page.id, hidden.id).await.unwrap();
    w.items.link(ali, page.id, open.id).await.unwrap();
    w.grant(RelationTuple::new(page.id, R::Watcher, can)).await;
    w.grant(RelationTuple::new(open.id, R::Watcher, can)).await;

    let seen = w.items.links(can, page.id).await.unwrap();
    assert_eq!(seen.len(), 1, "the hidden link is not even counted");
    assert_eq!(seen[0].item.id, open.id);
    assert!(!serde_json::to_string(&seen).unwrap().contains("Secret"));
    assert_eq!(w.items.links(ali, page.id).await.unwrap().len(), 2);
    // The hidden end answers exactly like an item that does not exist.
    assert!(matches!(
        w.items.links(can, hidden.id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.items.unlink(can, page.id, hidden.id).await,
        Err(AppError::Forbidden)
    ));
}

#[tokio::test]
async fn b3_a_text_mention_is_not_a_link() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let task = w.items.create(ali, World::task("Task")).await.unwrap();
    w.grant(RelationTuple::new(task.id, R::Watcher, ayse)).await;
    let page = w
        .items
        .create(
            ali,
            NewItem {
                kind: "page".into(),
                title: "Notes".into(),
                description: Some(format!("See @{} and @Task", task.id)),
            },
        )
        .await
        .unwrap();
    assert!(w.items.links(ali, page.id).await.unwrap().is_empty());
    assert!(matches!(
        w.items.get(ayse, page.id).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn a_task_to_goal_link_grants_no_access() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let task = w.items.create(ali, World::task("Task")).await.unwrap();
    let goal = w.items.create(ali, of_kind("goal", "Goal")).await.unwrap();
    w.grant(RelationTuple::new(goal.id, R::Watcher, ayse)).await;
    w.items.link(ali, task.id, goal.id).await.unwrap();
    assert!(matches!(
        w.items.get(ayse, task.id).await,
        Err(AppError::NotFound)
    ));
    assert!(w.items.links(ayse, goal.id).await.unwrap().is_empty());
    let incoming = w.items.links(ali, goal.id).await.unwrap();
    assert_eq!(incoming[0].direction, items::LinkDirection::Incoming);
}

#[tokio::test]
async fn trashed_items_cannot_be_linked_and_drop_out_of_link_lists() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    let task = w.items.create(ali, World::task("Task")).await.unwrap();
    let other = w.items.create(ali, World::task("Other")).await.unwrap();
    w.items.link(ali, page.id, task.id).await.unwrap();
    w.items.set_trashed(ali, task.id, true).await.unwrap();
    w.items.set_trashed(ali, other.id, true).await.unwrap();
    assert!(w.items.links(ali, page.id).await.unwrap().is_empty());
    assert!(matches!(
        w.items.link(ali, page.id, other.id).await,
        Err(AppError::Conflict(_))
    ));
    w.items.set_trashed(ali, task.id, false).await.unwrap();
    assert_eq!(w.items.links(ali, page.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn link_endpoints() {
    let w = World::new().await;
    let (_, token) = w.user("links@example.com").await;
    let (_, other) = w.user("links2@example.com").await;
    let mut ids_by_kind = Vec::new();
    for kind in ["page", "task"] {
        let response = w
            .call(
                Method::POST,
                "/items",
                &token,
                Some(json!({"kind": kind, "title": kind})),
            )
            .await;
        let id = json_body(response).await["id"].as_str().unwrap().to_owned();
        ids_by_kind.push(id);
    }
    let (page, task) = (&ids_by_kind[0], &ids_by_kind[1]);
    let uri = format!("/items/{page}/links/{task}");
    let response = w.call(Method::PUT, &uri, &other, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = w.call(Method::PUT, &uri, &token, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["direction"], "outgoing");
    assert_eq!(body["item"]["id"], task.as_str());

    let list = format!("/items/{task}/links");
    let body = json_body(w.call(Method::GET, &list, &token, None).await).await;
    assert_eq!(body[0]["direction"], "incoming");
    assert_eq!(body[0]["item"]["id"], page.as_str());

    let response = w.call(Method::DELETE, &uri, &token, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let body = json_body(w.call(Method::GET, &list, &token, None).await).await;
    assert_eq!(body, json!([]));
}

// --- tags (T1.2.2, E1, E2, E4) -----------------------------------------------------------------

fn tag(name: &str) -> items::NewTag {
    items::NewTag {
        name: name.into(),
        color: None,
    }
}

#[tokio::test]
async fn personal_tag_on_shared_item_is_invisible_to_other_members() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let item = w.items.create(ali, World::task("Shared")).await.unwrap();
    w.grant(RelationTuple::new(item.id, R::Assignee, ayse))
        .await;
    let alis = w.items.create_tag(ali, tag("Private")).await.unwrap();
    let ayses = w.items.create_tag(ayse, tag("Mine")).await.unwrap();
    w.items.attach_tag(ali, item.id, alis.id).await.unwrap();
    w.items.attach_tag(ayse, item.id, ayses.id).await.unwrap();

    // E2: each member sees only their own tag on the same item.
    let seen = w.items.item_tags(ali, item.id).await.unwrap();
    assert_eq!(seen, vec![alis.clone()]);
    let seen = w.items.item_tags(ayse, item.id).await.unwrap();
    assert_eq!(seen, vec![ayses.clone()]);

    // E4: the tag list holds only the caller's tags.
    assert_eq!(w.items.list_tags(ayse).await.unwrap(), vec![ayses.clone()]);

    // E2: someone else's tag behaves as if it did not exist.
    let changes = items::TagChanges {
        name: Some("Stolen".into()),
        color: None,
    };
    for result in [
        w.items.attach_tag(ayse, item.id, alis.id).await,
        w.items.detach_tag(ayse, item.id, alis.id).await,
        w.items.delete_tag(ayse, alis.id).await,
        w.items.update_tag(ayse, alis.id, changes).await.map(|_| ()),
        w.items
            .items_by_tag(ayse, alis.id, ListState::Active)
            .await
            .map(|_| ()),
    ] {
        assert!(matches!(result, Err(AppError::NotFound)));
    }
    assert_eq!(w.items.item_tags(ali, item.id).await.unwrap(), vec![alis]);
}

#[tokio::test]
async fn items_by_tag_returns_only_accessible_items() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let shared = w.items.create(ali, World::task("Shared")).await.unwrap();
    let own = w.items.create(ayse, World::task("Own")).await.unwrap();
    let untagged = w.items.create(ayse, World::task("Untagged")).await.unwrap();
    let tuple = RelationTuple::new(shared.id, R::Assignee, ayse);
    w.grant(tuple).await;
    let label = w.items.create_tag(ayse, tag("Label")).await.unwrap();
    w.items.attach_tag(ayse, shared.id, label.id).await.unwrap();
    w.items.attach_tag(ayse, own.id, label.id).await.unwrap();
    // Attaching twice is a no-op.
    w.items.attach_tag(ayse, own.id, label.id).await.unwrap();

    let found = w
        .items
        .items_by_tag(ayse, label.id, ListState::Active)
        .await
        .unwrap();
    assert_eq!(found.len(), 2);
    assert!(!ids(&found).contains(&untagged.id));

    // Access revoked: the tag stays, but the item no longer appears and cannot be re-tagged.
    let mut tx = w.db.pool().begin().await.unwrap();
    w.access.revoke(&mut tx, tuple).await.unwrap();
    tx.commit().await.unwrap();
    let found = w
        .items
        .items_by_tag(ayse, label.id, ListState::Active)
        .await
        .unwrap();
    assert_eq!(ids(&found), vec![own.id]);
    assert!(matches!(
        w.items.attach_tag(ayse, shared.id, label.id).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn tag_names_are_unique_per_owner_and_trash_blocks_attaching() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let first = w.items.create_tag(ali, tag("Work")).await.unwrap();
    assert!(matches!(
        w.items.create_tag(ali, tag("work")).await,
        Err(AppError::Conflict(_))
    ));
    w.items.create_tag(ayse, tag("Work")).await.unwrap();

    let item = w.items.create(ali, World::task("Task")).await.unwrap();
    w.items.set_trashed(ali, item.id, true).await.unwrap();
    assert!(matches!(
        w.items.attach_tag(ali, item.id, first.id).await,
        Err(AppError::Conflict(_))
    ));
    w.items.set_trashed(ali, item.id, false).await.unwrap();
    w.items.attach_tag(ali, item.id, first.id).await.unwrap();

    // Deleting the tag detaches it.
    w.items.delete_tag(ali, first.id).await.unwrap();
    assert!(w.items.item_tags(ali, item.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn tag_endpoints() {
    let w = World::new().await;
    let (_, token) = w.user("tags@example.com").await;
    let (_, other) = w.user("other@example.com").await;
    let response = w
        .call(
            Method::POST,
            "/tags",
            &token,
            Some(json!({"name": " Work ", "color": "#FF0000"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    assert_eq!(created["name"], "Work");
    assert_eq!(created["color"], "#ff0000");
    let tag_id = created["id"].as_str().unwrap().to_owned();

    let response = w
        .call(
            Method::POST,
            "/items",
            &token,
            Some(json!({"kind": "task", "title": "Tagged"})),
        )
        .await;
    let item_id = json_body(response).await["id"].as_str().unwrap().to_owned();
    let attach = format!("/items/{item_id}/tags/{tag_id}");
    let response = w.call(Method::PUT, &attach, &token, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = w.call(Method::PUT, &attach, &other, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let uri = format!("/tags/{tag_id}/items");
    let body = json_body(w.call(Method::GET, &uri, &token, None).await).await;
    assert_eq!(body[0]["id"], item_id.as_str());
    let response = w.call(Method::GET, &uri, &other, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = w.call(Method::DELETE, &attach, &token, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let uri = format!("/items/{item_id}/tags");
    let body = json_body(w.call(Method::GET, &uri, &token, None).await).await;
    assert_eq!(body, json!([]));
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
