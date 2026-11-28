//! Items CRUD, archive, trash and purge (T1.2.1, PRD N1–N4, V6, V9) against a real Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

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

/// Stand-in for friendships (E3.1): who may share with whom (V1).
#[derive(Clone, Default)]
struct FakeSharing(Arc<Mutex<HashSet<(Uuid, Uuid)>>>);

impl FakeSharing {
    fn befriend(&self, a: Uuid, b: Uuid) {
        let mut pairs = self.0.lock().unwrap();
        pairs.insert((a, b));
        pairs.insert((b, a));
    }

    fn unfriend(&self, a: Uuid, b: Uuid) {
        let mut pairs = self.0.lock().unwrap();
        pairs.remove(&(a, b));
        pairs.remove(&(b, a));
    }
}

impl items::SharingPolicy for FakeSharing {
    fn can_share(&self, from: Uuid, to: Uuid) -> items::SharingFuture<'_> {
        let allowed = self.0.lock().unwrap().contains(&(from, to));
        Box::pin(async move { Ok(allowed) })
    }
}

/// Stand-in for the teams module (T5.1.3): members and whether their role may create items and
/// assign others.
/// (team, user) → (may create items, may assign others and manage tags).
type TeamRights = std::collections::HashMap<(Uuid, Uuid), (bool, bool)>;

#[derive(Clone, Default)]
struct FakeTeams(Arc<Mutex<TeamRights>>);

impl FakeTeams {
    fn join(&self, team: Uuid, user: Uuid, create: bool, assign: bool) {
        self.0
            .lock()
            .unwrap()
            .insert((team, user), (create, assign));
    }
}

impl items::TeamDirectory for FakeTeams {
    fn teams_of(&self, user: Uuid) -> items::TeamsFuture<'_> {
        let teams = self
            .0
            .lock()
            .unwrap()
            .keys()
            .filter(|(_, u)| *u == user)
            .map(|(t, _)| *t)
            .collect();
        Box::pin(async move { Ok(teams) })
    }

    fn is_member(&self, team: Uuid, user: Uuid) -> items::TeamFuture<'_> {
        let member = self.0.lock().unwrap().contains_key(&(team, user));
        Box::pin(async move { Ok(member) })
    }

    fn may(&self, actor: Uuid, team: Uuid, action: items::TeamAction) -> items::TeamFuture<'_> {
        let rights = self.0.lock().unwrap().get(&(team, actor)).copied();
        let allowed = rights.is_some_and(|(create, assign)| match action {
            items::TeamAction::CreateItems | items::TeamAction::CreateNotebooks => create,
            items::TeamAction::Assign | items::TeamAction::ManageTags => assign,
        });
        Box::pin(async move { Ok(allowed) })
    }
}

struct World {
    db: TestDb,
    clock: FixedClock,
    access: AccessEngine<ItemsAccessContext>,
    items: ItemsService,
    sharing: FakeSharing,
    teams: FakeTeams,
    router: Router,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(start());
        let shared = Arc::new(clock.clone());
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let sharing = FakeSharing::default();
        let teams = FakeTeams::default();
        let items = ItemsService::new(db.pool().clone(), shared.clone(), access.clone())
            .with_sharing(Arc::new(sharing.clone()))
            .with_teams(Arc::new(teams.clone()));
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
            sharing,
            teams,
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
                            "accepted_legal": identity::legal::current_acceptances(),
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

// --- filters and saved views (T1.2.7, F1-F3) ----------------------------------------------------

async fn filtered(w: &World, actor: Uuid, filter: items::ItemFilter) -> HashSet<Uuid> {
    let page = w
        .items
        .filter_items(actor, &filter, &first_page())
        .await
        .unwrap();
    page.items.iter().map(|item| item.id).collect()
}

#[tokio::test]
async fn f1_filters_run_only_on_accessible_items() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let mine = w.items.create(ali, World::task("Mine")).await.unwrap();
    w.items.create(ayse, World::task("Hers")).await.unwrap();
    let filter = items::ItemFilter {
        kinds: Some(vec!["task".into()]),
        ..items::ItemFilter::default()
    };
    assert_eq!(filtered(&w, ali, filter).await, HashSet::from([mine.id]));
}

#[tokio::test]
async fn f1_filter_by_tag_uses_only_the_callers_tags() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let shared = w.items.create(ali, World::task("Shared")).await.unwrap();
    let other = w.items.create(ali, World::task("Other")).await.unwrap();
    w.grant(RelationTuple::new(shared.id, R::Watcher, ayse))
        .await;
    let alis = w.items.create_tag(ali, tag("Work")).await.unwrap();
    w.items.attach_tag(ali, shared.id, alis.id).await.unwrap();
    let by_tag = items::ItemFilter {
        tag_ids: Some(vec![alis.id]),
        ..items::ItemFilter::default()
    };
    assert_eq!(
        filtered(&w, ali, by_tag.clone()).await,
        HashSet::from([shared.id])
    );
    assert!(!filtered(&w, ali, by_tag.clone()).await.contains(&other.id));
    // E2: Ayse cannot learn what Ali tagged by filtering with Ali's tag id.
    assert!(filtered(&w, ayse, by_tag).await.is_empty());
}

#[tokio::test]
async fn f1_filter_by_kind_and_status_and_state() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let task = w.items.create(ali, World::task("T")).await.unwrap();
    let goal = w.items.create(ali, of_kind("goal", "G")).await.unwrap();
    let archived = w.items.create(ali, World::task("A")).await.unwrap();
    let trashed = w.items.create(ali, World::task("D")).await.unwrap();
    w.items.set_archived(ali, archived.id, true).await.unwrap();
    w.items.set_trashed(ali, trashed.id, true).await.unwrap();
    sqlx::query("UPDATE items SET status = 'doing' WHERE id = $1")
        .bind(task.id)
        .execute(w.db.pool())
        .await
        .unwrap();

    let kinds = items::ItemFilter {
        kinds: Some(vec!["goal".into()]),
        ..items::ItemFilter::default()
    };
    assert_eq!(filtered(&w, ali, kinds).await, HashSet::from([goal.id]));
    let statuses = items::ItemFilter {
        statuses: Some(vec!["doing".into(), "done".into()]),
        ..items::ItemFilter::default()
    };
    assert_eq!(filtered(&w, ali, statuses).await, HashSet::from([task.id]));
    assert_eq!(
        filtered(&w, ali, items::ItemFilter::default()).await,
        HashSet::from([task.id, goal.id]),
        "active by default"
    );
    for (state, id) in [
        (ListState::Archived, archived.id),
        (ListState::Trash, trashed.id),
    ] {
        let filter = items::ItemFilter {
            state: Some(state),
            ..items::ItemFilter::default()
        };
        assert_eq!(filtered(&w, ali, filter).await, HashSet::from([id]));
    }
    // Dimensions combine with AND.
    let none = items::ItemFilter {
        kinds: Some(vec!["goal".into()]),
        statuses: Some(vec!["doing".into()]),
        ..items::ItemFilter::default()
    };
    assert!(filtered(&w, ali, none).await.is_empty());
}

#[tokio::test]
async fn f1_filter_by_date_is_half_open() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let first = w.items.create(ali, World::task("1")).await.unwrap();
    let boundary = start() + Duration::days(1);
    w.clock.set(boundary);
    let second = w.items.create(ali, World::task("2")).await.unwrap();
    let date = |from, to| items::ItemFilter {
        date: Some(items::DateFilter {
            field: items::DateField::Created,
            from,
            to,
            last_days: None,
        }),
        ..items::ItemFilter::default()
    };
    assert_eq!(
        filtered(&w, ali, date(Some(boundary), None)).await,
        HashSet::from([second.id])
    );
    assert_eq!(
        filtered(&w, ali, date(None, Some(boundary))).await,
        HashSet::from([first.id])
    );

    w.clock.set(start() + Duration::days(5));
    let changes = items::ItemChanges {
        title: Some("Touched".into()),
        description: None,
    };
    w.items.update(ali, first.id, changes).await.unwrap();
    let touched_filter = items::ItemFilter {
        date: Some(items::DateFilter {
            field: items::DateField::LastTouched,
            from: None,
            to: Some(start() + Duration::days(2)),
            last_days: None,
        }),
        ..items::ItemFilter::default()
    };
    assert_eq!(
        filtered(&w, ali, touched_filter).await,
        HashSet::from([second.id])
    );
}

#[tokio::test]
async fn t1_5_7_a_relative_date_range_follows_the_clock() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let old = w.items.create(ali, World::task("Old")).await.unwrap();
    w.clock.set(start() + Duration::days(10));
    let recent = w.items.create(ali, World::task("Recent")).await.unwrap();
    let last_week = items::ItemFilter {
        date: Some(items::DateFilter {
            field: items::DateField::Created,
            from: None,
            to: None,
            last_days: Some(7),
        }),
        ..items::ItemFilter::default()
    };
    assert_eq!(
        filtered(&w, ali, last_week.clone()).await,
        HashSet::from([recent.id])
    );
    // A saved view keeps the relative range: a week later, nothing is that recent.
    let view = w
        .items
        .create_view(
            ali,
            items::CreateSavedViewRequest {
                name: "This week".into(),
                filter: last_week,
                pinned: None,
                position: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.filter.date.and_then(|d| d.last_days), Some(7));
    w.clock.set(start() + Duration::days(18));
    let page = kernel::PageRequest::from_params(kernel::PageParams {
        limit: Some(50),
        cursor: None,
    })
    .unwrap();
    let ran = w.items.view_items(ali, view.id, &page).await.unwrap();
    assert!(ran.items.is_empty(), "{:?}", [old.id, recent.id]);
}

#[tokio::test]
async fn f1_f3_filter_by_person_accepts_only_friends() {
    let w = World::new().await;
    let (ali, ayse, stranger) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let with_ayse = w.items.create(ali, World::task("A")).await.unwrap();
    let with_stranger = w.items.create(ali, World::task("B")).await.unwrap();
    w.items.create(ali, World::task("Solo")).await.unwrap();
    // An item only Ayse can see must not come back.
    w.items.create(ayse, World::task("Hers")).await.unwrap();
    w.grant(RelationTuple::new(with_ayse.id, R::Watcher, ayse))
        .await;
    w.grant(RelationTuple::new(with_stranger.id, R::Watcher, stranger))
        .await;
    let people = |list: Vec<Uuid>| items::ItemFilter {
        people: Some(list),
        ..items::ItemFilter::default()
    };
    assert_eq!(
        filtered(&w, ali, people(vec![ayse])).await,
        HashSet::from([with_ayse.id])
    );
    // F3: when a saved view runs, a non-friend is ignored, so nothing can be learned about them
    // (asking for one is refused: `f3_choosing_someone_who_is_not_a_friend_is_refused`).
    assert!(filtered(&w, ali, people(vec![stranger])).await.is_empty());
    assert_eq!(
        filtered(&w, ali, people(vec![ayse, stranger])).await,
        HashSet::from([with_ayse.id])
    );
    w.sharing.unfriend(ali, ayse);
    assert!(filtered(&w, ali, people(vec![ayse])).await.is_empty());
}

#[tokio::test]
async fn f3_choosing_someone_who_is_not_a_friend_is_refused() {
    let w = World::new().await;
    let (ali, ayse, stranger) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let with_ayse = w.items.create(ali, World::task("A")).await.unwrap();
    w.grant(RelationTuple::new(with_ayse.id, R::Watcher, ayse))
        .await;
    let people = |list: Vec<Uuid>| items::ItemFilter {
        people: Some(list),
        ..items::ItemFilter::default()
    };
    // Asked for now: a friend is fine; a stranger, oneself or a mix with a stranger is refused.
    let page = w
        .items
        .filter_checked(ali, &people(vec![ayse]), &first_page())
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    for refused in [vec![stranger], vec![ali], vec![ayse, stranger]] {
        assert!(matches!(
            w.items
                .filter_checked(ali, &people(refused), &first_page())
                .await,
            Err(AppError::Validation(_))
        ));
    }

    // Saving a view checks the same; a saved one keeps working after an unfriending.
    let saved = |filter| items::CreateSavedViewRequest {
        name: "Ayşe ile".into(),
        filter,
        pinned: None,
        position: None,
    };
    assert!(matches!(
        w.items
            .create_view(ali, saved(people(vec![stranger])))
            .await,
        Err(AppError::Validation(_))
    ));
    let view = w
        .items
        .create_view(ali, saved(people(vec![ayse])))
        .await
        .unwrap();
    w.sharing.unfriend(ali, ayse);
    let page = w
        .items
        .view_items(ali, view.id, &first_page())
        .await
        .unwrap();
    assert!(
        page.items.is_empty(),
        "a former friend is ignored, not an error"
    );
    let change = items::UpdateSavedViewRequest {
        filter: Some(people(vec![ayse])),
        ..items::UpdateSavedViewRequest::default()
    };
    assert!(matches!(
        w.items.update_view(ali, view.id, change).await,
        Err(AppError::Validation(_))
    ));
}

#[tokio::test]
async fn f1_filter_by_team_is_empty_until_teams_exist() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    w.items.create(ali, World::task("Personal")).await.unwrap();
    let filter = items::ItemFilter {
        team_ids: Some(vec![Uuid::now_v7()]),
        ..items::ItemFilter::default()
    };
    assert!(filtered(&w, ali, filter).await.is_empty());
}

fn view(name: &str, filter: items::ItemFilter) -> items::CreateSavedViewRequest {
    items::CreateSavedViewRequest {
        name: name.into(),
        filter,
        pinned: None,
        position: None,
    }
}

#[tokio::test]
async fn f2_saved_views_crud_pin_and_privacy() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let task = w.items.create(ali, World::task("T")).await.unwrap();
    w.items.create(ali, of_kind("goal", "G")).await.unwrap();
    let tasks_only = items::ItemFilter {
        kinds: Some(vec!["task".into()]),
        ..items::ItemFilter::default()
    };
    let first = w
        .items
        .create_view(ali, view("Alpha", items::ItemFilter::default()))
        .await
        .unwrap();
    let second = w
        .items
        .create_view(ali, view("Tasks", tasks_only.clone()))
        .await
        .unwrap();
    assert_eq!(second.filter, tasks_only);
    assert!(matches!(
        w.items
            .create_view(ali, view("tasks", items::ItemFilter::default()))
            .await,
        Err(AppError::Conflict(_))
    ));
    // Names are per person.
    w.items
        .create_view(ayse, view("Tasks", items::ItemFilter::default()))
        .await
        .unwrap();

    let page = w
        .items
        .view_items(ali, second.id, &first_page())
        .await
        .unwrap();
    assert_eq!(ids(&page.items), vec![task.id]);

    // Pinned views come first.
    let pin = items::UpdateSavedViewRequest {
        pinned: Some(true),
        ..items::UpdateSavedViewRequest::default()
    };
    let pinned = w
        .items
        .update_view(ali, second.id, pin.clone())
        .await
        .unwrap();
    assert!(pinned.pinned);
    let listed: Vec<Uuid> = w
        .items
        .list_views(ali)
        .await
        .unwrap()
        .iter()
        .map(|v| v.id)
        .collect();
    assert_eq!(listed, vec![second.id, first.id]);

    // Someone else's view behaves as if it did not exist.
    assert!(matches!(
        w.items.update_view(ayse, second.id, pin).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.items.delete_view(ayse, second.id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.items.view_items(ayse, second.id, &first_page()).await,
        Err(AppError::NotFound)
    ));

    w.items.delete_view(ali, second.id).await.unwrap();
    assert_eq!(w.items.list_views(ali).await.unwrap().len(), 1);
}

#[tokio::test]
async fn filter_and_view_endpoints() {
    let w = World::new().await;
    let (_, token) = w.user("filters@example.com").await;
    for minute in 0..3 {
        w.clock.set(start() + Duration::minutes(minute));
        w.call(
            Method::POST,
            "/items",
            &token,
            Some(json!({"kind": "task", "title": "T"})),
        )
        .await;
    }
    w.call(
        Method::POST,
        "/items",
        &token,
        Some(json!({"kind": "goal", "title": "G"})),
    )
    .await;
    // {"kinds":["task"]}
    let encoded = "%7B%22kinds%22%3A%5B%22task%22%5D%7D";
    let uri = format!("/items/filter?filter={encoded}&limit=2");
    let body = json_body(w.call(Method::GET, &uri, &token, None).await).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    let cursor = body["next_cursor"].as_str().unwrap();
    let uri = format!("/items/filter?filter={encoded}&limit=2&cursor={cursor}");
    let body = json_body(w.call(Method::GET, &uri, &token, None).await).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert!(body["next_cursor"].is_null());

    let response = w
        .call(Method::GET, "/items/filter?filter=nope", &token, None)
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let response = w
        .call(
            Method::POST,
            "/saved-views",
            &token,
            Some(json!({"name": "Tasks", "filter": {"kinds": ["task"]}, "pinned": true})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    assert_eq!(created["pinned"], true);
    let id = created["id"].as_str().unwrap();
    let uri = format!("/saved-views/{id}/items");
    let body = json_body(w.call(Method::GET, &uri, &token, None).await).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 3);
    let uri = format!("/saved-views/{id}");
    let response = w
        .call(
            Method::PATCH,
            &uri,
            &token,
            Some(json!({"pinned": false, "position": 2})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["position"], 2);
    let response = w.call(Method::DELETE, &uri, &token, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let body = json_body(w.call(Method::GET, "/saved-views", &token, None).await).await;
    assert_eq!(body, json!([]));
}

// --- last_touched_at (T1.2.6, H4) ---------------------------------------------------------------

async fn touched(w: &World, actor: Uuid, item: Uuid) -> DateTime<Utc> {
    w.items.get(actor, item).await.unwrap().last_touched_at
}

#[tokio::test]
async fn h4_reading_and_private_actions_do_not_touch() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("Task")).await.unwrap();
    let goal = w.items.create(ali, of_kind("goal", "Goal")).await.unwrap();
    w.clock.set(start() + Duration::days(1));

    w.items.get(ali, task.id).await.unwrap();
    w.items.list(ali, ListState::Active).await.unwrap();
    let label = w.items.create_tag(ali, tag("Private")).await.unwrap();
    w.items.attach_tag(ali, task.id, label.id).await.unwrap();
    w.items.link(ali, task.id, goal.id).await.unwrap();
    w.items.unlink(ali, task.id, goal.id).await.unwrap();
    let invitation = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    w.items
        .decline_invitation(ayse, invitation.id)
        .await
        .unwrap();
    w.items.set_archived(ali, task.id, true).await.unwrap();
    w.items.set_archived(ali, task.id, true).await.unwrap();

    assert_eq!(touched(&w, ali, task.id).await, start());
    assert_eq!(touched(&w, ali, goal.id).await, start());
}

#[tokio::test]
async fn h4_meaningful_changes_touch() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("Task")).await.unwrap();

    let edited = start() + Duration::days(1);
    w.clock.set(edited);
    let changes = items::ItemChanges {
        title: Some("Renamed".into()),
        description: None,
    };
    let updated = w.items.update(ali, task.id, changes).await.unwrap();
    assert_eq!(updated.last_touched_at, edited, "returned row is fresh");

    let joined = start() + Duration::days(2);
    w.clock.set(joined);
    let invitation = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    assert_eq!(touched(&w, ali, task.id).await, edited);
    w.items
        .accept_invitation(ayse, invitation.id)
        .await
        .unwrap();
    assert_eq!(touched(&w, ali, task.id).await, joined);

    let completed = start() + Duration::days(3);
    w.clock.set(completed);
    let mut tx = w.db.pool().begin().await.unwrap();
    w.items
        .record_completed(&mut tx, ayse, task.id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(touched(&w, ali, task.id).await, completed);

    w.items.set_archived(ali, task.id, true).await.unwrap();
    let unarchived = start() + Duration::days(4);
    w.clock.set(unarchived);
    let item = w.items.set_archived(ali, task.id, false).await.unwrap();
    assert_eq!(item.last_touched_at, unarchived);
    // Unarchiving again is not a change.
    w.clock.set(start() + Duration::days(5));
    let item = w.items.set_archived(ali, task.id, false).await.unwrap();
    assert_eq!(item.last_touched_at, unarchived);

    w.items.set_trashed(ali, task.id, true).await.unwrap();
    let restored = start() + Duration::days(6);
    w.clock.set(restored);
    let item = w.items.set_trashed(ali, task.id, false).await.unwrap();
    assert_eq!(item.last_touched_at, restored);
    w.clock.set(start() + Duration::days(7));
    let item = w.items.set_trashed(ali, task.id, false).await.unwrap();
    assert_eq!(
        item.last_touched_at, restored,
        "restoring again is no touch"
    );
}

#[tokio::test]
async fn h4_a_refused_edit_does_not_touch() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let task = w.items.create(ali, World::task("Task")).await.unwrap();
    w.items.set_trashed(ali, task.id, true).await.unwrap();
    w.clock.set(start() + Duration::days(1));
    let changes = items::ItemChanges {
        title: Some("Nope".into()),
        description: None,
    };
    assert!(matches!(
        w.items.update(ali, task.id, changes).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        w.items.set_archived(ali, task.id, false).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(touched(&w, ali, task.id).await, start());
}

// --- activity (T1.2.5, A1, A2) ------------------------------------------------------------------

fn first_page() -> kernel::PageRequest {
    kernel::PageRequest::from_params(kernel::PageParams::default()).unwrap()
}

async fn kinds(w: &World, actor: Uuid, item: Uuid) -> Vec<items::ActivityKind> {
    let page = w
        .items
        .item_activity(actor, item, &first_page())
        .await
        .unwrap();
    page.items.iter().rev().map(|event| event.kind).collect()
}

#[tokio::test]
async fn a1_meaningful_changes_are_recorded_with_who_and_when() {
    use items::ActivityKind as K;
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("Plan")).await.unwrap();

    // A pending or declined invitation and a personal tag leave no trace (G5, E2).
    let declined = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    w.items.decline_invitation(ayse, declined.id).await.unwrap();
    let label = w.items.create_tag(ali, tag("Private")).await.unwrap();
    w.items.attach_tag(ali, task.id, label.id).await.unwrap();
    assert_eq!(kinds(&w, ali, task.id).await, [K::Created]);

    w.clock.set(start() + Duration::minutes(1));
    let invitation = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    w.items
        .accept_invitation(ayse, invitation.id)
        .await
        .unwrap();
    let page = w
        .items
        .item_activity(ayse, task.id, &first_page())
        .await
        .unwrap();
    let assigned = &page.items[0];
    assert_eq!(assigned.kind, K::Assigned);
    assert_eq!(assigned.actor_id, Some(ali));
    assert_eq!(assigned.member_user_id, Some(ayse));
    assert_eq!(assigned.member_role, Some(items::InvitationRole::Assignee));
    assert_eq!(assigned.created_at, start() + Duration::minutes(1));

    w.clock.set(start() + Duration::minutes(2));
    let mut tx = w.db.pool().begin().await.unwrap();
    w.items
        .record_completed(&mut tx, ayse, task.id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    w.clock.set(start() + Duration::minutes(3));
    let page_item = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    w.clock.set(start() + Duration::minutes(4));
    w.items.link(ali, page_item.id, task.id).await.unwrap();
    // Linking again records nothing.
    w.items.link(ali, page_item.id, task.id).await.unwrap();
    w.clock.set(start() + Duration::minutes(5));
    w.items.unlink(ali, page_item.id, task.id).await.unwrap();
    assert_eq!(
        kinds(&w, ali, task.id).await,
        [
            K::Created,
            K::Assigned,
            K::Completed,
            K::Linked,
            K::Unlinked
        ]
    );
    assert_eq!(
        kinds(&w, ali, page_item.id).await,
        [K::Created, K::Linked, K::Unlinked]
    );
}

#[tokio::test]
async fn a2_activity_is_limited_to_visible_items_and_link_ends() {
    use items::ActivityKind as K;
    let w = World::new().await;
    let (ali, can) = (Uuid::now_v7(), Uuid::now_v7());
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    let hidden = w.items.create(ali, World::task("Secret")).await.unwrap();
    let own = w.items.create(can, World::task("Own")).await.unwrap();
    // Events in the same millisecond have no defined order.
    w.clock.set(start() + Duration::minutes(1));
    w.items.link(ali, page.id, hidden.id).await.unwrap();
    w.grant(RelationTuple::new(page.id, R::Watcher, can)).await;

    // V9: the link to an item Can cannot see does not appear, not even as an event.
    assert_eq!(kinds(&w, can, page.id).await, [K::Created]);
    assert_eq!(
        kinds(&w, ali, page.id).await,
        [K::Created, K::Linked],
        "the owner sees both ends"
    );
    assert!(matches!(
        w.items.item_activity(can, hidden.id, &first_page()).await,
        Err(AppError::NotFound)
    ));
    let feed = w.items.activity_feed(can, &first_page()).await.unwrap();
    let items: HashSet<Uuid> = feed.items.iter().map(|event| event.item_id).collect();
    assert_eq!(items, HashSet::from([page.id, own.id]));
    assert!(feed.items.iter().all(|e| e.related_item_id.is_none()));

    // Losing access removes the history from the feed too.
    revoke(&w, RelationTuple::new(page.id, R::Watcher, can)).await;
    let feed = w.items.activity_feed(can, &first_page()).await.unwrap();
    assert!(feed.items.iter().all(|event| event.item_id == own.id));

    // The trash leaves the feed.
    w.items.set_trashed(can, own.id, true).await.unwrap();
    let feed = w.items.activity_feed(can, &first_page()).await.unwrap();
    assert!(feed.items.is_empty());
}

#[tokio::test]
async fn activity_of_a_system_unlink_has_no_actor_and_purge_removes_history() {
    use items::ActivityKind as K;
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    let task = w.items.create(ayse, World::task("Task")).await.unwrap();
    let membership = RelationTuple::new(task.id, R::Watcher, ali);
    w.grant(membership).await;
    w.clock.set(start() + Duration::minutes(1));
    w.items.link(ali, page.id, task.id).await.unwrap();
    w.clock.set(start() + Duration::minutes(2));
    revoke(&w, membership).await;
    deliver(&w).await;

    let history = w
        .items
        .item_activity(ayse, task.id, &first_page())
        .await
        .unwrap();
    // Ayse can no longer see the page, so neither link event is shown to them.
    assert_eq!(history.items.len(), 1);
    assert_eq!(history.items[0].kind, K::Created);
    let stored: Option<Uuid> = sqlx::query_scalar(
        "SELECT actor_id FROM activity_events WHERE type = 'unlinked' AND item_id = $1",
    )
    .bind(page.id)
    .fetch_one(w.db.pool())
    .await
    .unwrap();
    assert_eq!(stored, None);

    w.items.set_trashed(ali, page.id, true).await.unwrap();
    let later = start() + TRASH_RETENTION + Duration::hours(1);
    w.items.purge_expired(later).await.unwrap();
    let left: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_events WHERE item_id = $1 OR related_item_id = $1",
    )
    .bind(page.id)
    .fetch_one(w.db.pool())
    .await
    .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn activity_endpoints_page_through_results() {
    let w = World::new().await;
    let (_, token) = w.user("activity@example.com").await;
    let (_, other) = w.user("activity2@example.com").await;
    let mut last = String::new();
    for minute in 0..3 {
        w.clock.set(start() + Duration::minutes(minute));
        let response = w
            .call(
                Method::POST,
                "/items",
                &token,
                Some(json!({"kind": "task", "title": "T"})),
            )
            .await;
        last = json_body(response).await["id"].as_str().unwrap().to_owned();
    }
    let body = json_body(w.call(Method::GET, "/activity?limit=2", &token, None).await).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert_eq!(body["items"][0]["item_id"], last.as_str());
    assert_eq!(body["items"][0]["kind"], "created");
    let cursor = body["next_cursor"].as_str().unwrap();
    let uri = format!("/activity?limit=2&cursor={cursor}");
    let body = json_body(w.call(Method::GET, &uri, &token, None).await).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert!(body["next_cursor"].is_null());

    let uri = format!("/items/{last}/activity");
    let response = w.call(Method::GET, &uri, &token, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = w.call(Method::GET, &uri, &other, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = w
        .call(Method::GET, "/activity?cursor=%21bad", &token, None)
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// --- invitations (T1.2.4, G5, V1, V5) -----------------------------------------------------------

fn invite_as(to_user: Uuid, role: items::InvitationRole) -> items::CreateInvitationRequest {
    items::CreateInvitationRequest { to_user, role }
}

fn assignee(to_user: Uuid) -> items::CreateInvitationRequest {
    invite_as(to_user, items::InvitationRole::Assignee)
}

/// Sends `count` invitations from `from` to new friends.
async fn send_invitations(w: &World, from: Uuid, item: Uuid, count: i64) {
    for _ in 0..count {
        let to = Uuid::now_v7();
        w.sharing.befriend(from, to);
        w.items.invite(from, item, assignee(to)).await.unwrap();
    }
}

#[tokio::test]
async fn invitations_are_rate_limited_per_sender() {
    let w = World::new().await;
    let (ali, ayse, friend) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, friend);
    w.sharing.befriend(ayse, friend);
    let alis = w.items.create(ali, World::task("A")).await.unwrap();
    let ayses = w.items.create(ayse, World::task("B")).await.unwrap();
    send_invitations(&w, ali, alis.id, items::INVITE_LIMIT).await;

    w.clock.set(start() + Duration::minutes(10));
    assert!(matches!(
        w.items.invite(ali, alis.id, assignee(friend)).await,
        Err(AppError::RateLimited {
            retry_after_secs: Some(3000)
        })
    ));
    // Another sender is not affected.
    w.items
        .invite(ayse, ayses.id, assignee(friend))
        .await
        .unwrap();

    // The window slides: exactly one hour later the old invitations no longer count.
    w.clock.set(start() + items::INVITE_WINDOW);
    w.items
        .invite(ali, alis.id, assignee(friend))
        .await
        .unwrap();
}

#[tokio::test]
async fn answered_invitations_still_count_toward_the_limit() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("A")).await.unwrap();
    send_invitations(&w, ali, task.id, items::INVITE_LIMIT - 1).await;
    let last = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    w.items.decline_invitation(ayse, last.id).await.unwrap();
    assert!(matches!(
        w.items.invite(ali, task.id, assignee(ayse)).await,
        Err(AppError::RateLimited { .. })
    ));
}

#[tokio::test]
async fn parallel_invitations_cannot_pass_the_limit() {
    let w = World::new().await;
    let ali = Uuid::now_v7();
    let task = w.items.create(ali, World::task("A")).await.unwrap();
    send_invitations(&w, ali, task.id, items::INVITE_LIMIT - 2).await;
    let mut calls = Vec::new();
    for _ in 0..6 {
        let to = Uuid::now_v7();
        w.sharing.befriend(ali, to);
        let service = w.items.clone();
        let id = task.id;
        calls.push(tokio::spawn(async move {
            service.invite(ali, id, assignee(to)).await
        }));
    }
    let mut created = 0;
    for call in calls {
        match call.await.unwrap() {
            Ok(_) => created += 1,
            Err(AppError::RateLimited { .. }) => {}
            Err(other) => panic!("unexpected error: {other}"),
        }
    }
    assert_eq!(created, 2);
}

#[tokio::test]
async fn invitation_endpoint_answers_429_with_retry_after() {
    let w = World::new().await;
    let (ali, token) = w.user("limited@example.com").await;
    let (ayse, _) = w.user("limited2@example.com").await;
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("A")).await.unwrap();
    send_invitations(&w, ali, task.id, items::INVITE_LIMIT).await;
    let response = w
        .call(
            Method::POST,
            &format!("/items/{}/invitations", task.id),
            &token,
            Some(json!({"to_user": ayse, "role": "assignee"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "3600");
}

#[tokio::test]
async fn g5_accepting_adds_the_member_and_publishes_an_event() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("Plan")).await.unwrap();

    let invitation = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    assert_eq!(invitation.status, items::InvitationStatus::Pending);
    assert_eq!(invitation.item_title, "Plan");
    // No access until accepted.
    assert!(matches!(
        w.items.get(ayse, task.id).await,
        Err(AppError::NotFound)
    ));
    let events: Vec<Value> =
        sqlx::query_scalar("SELECT payload FROM outbox WHERE event_type = 'invitation.created'")
            .fetch_all(w.db.pool())
            .await
            .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["to_user"], ayse.to_string());

    let inbox = w.items.pending_invitations(ayse).await.unwrap();
    assert_eq!(inbox, vec![invitation.clone()]);
    w.clock.set(start() + Duration::hours(1));
    let accepted = w
        .items
        .accept_invitation(ayse, invitation.id)
        .await
        .unwrap();
    assert_eq!(accepted.status, items::InvitationStatus::Accepted);
    assert_eq!(accepted.responded_at, Some(start() + Duration::hours(1)));
    assert_eq!(w.items.get(ayse, task.id).await.unwrap().id, task.id);
    assert!(w.items.pending_invitations(ayse).await.unwrap().is_empty());
    assert!(matches!(
        w.items.accept_invitation(ayse, invitation.id).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn g5_declining_changes_nothing() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("Plan")).await.unwrap();
    let invitation = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();

    let declined = w
        .items
        .decline_invitation(ayse, invitation.id)
        .await
        .unwrap();
    assert_eq!(declined.status, items::InvitationStatus::Declined);
    assert!(matches!(
        w.items.get(ayse, task.id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.items.accept_invitation(ayse, invitation.id).await,
        Err(AppError::Conflict(_))
    ));
    // A new invitation is possible afterwards.
    w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
}

#[tokio::test]
async fn v1_only_friends_can_be_invited_and_the_default_refuses_everyone() {
    let w = World::new().await;
    let (ali, stranger) = (Uuid::now_v7(), Uuid::now_v7());
    let task = w.items.create(ali, World::task("Plan")).await.unwrap();
    assert!(matches!(
        w.items.invite(ali, task.id, assignee(stranger)).await,
        Err(AppError::Forbidden)
    ));

    // Production wiring until E3.1: no policy, so nobody can be invited.
    w.sharing.befriend(ali, stranger);
    let shared: Arc<FixedClock> = Arc::new(w.clock.clone());
    let default = ItemsService::new(w.db.pool().clone(), shared, w.access.clone());
    assert!(matches!(
        default.invite(ali, task.id, assignee(stranger)).await,
        Err(AppError::Forbidden)
    ));
}

#[tokio::test]
async fn v5_inviting_needs_share_and_only_the_invitee_can_answer() {
    let w = World::new().await;
    let (ali, ayse, can) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    w.sharing.befriend(ayse, can);
    let task = w.items.create(ali, World::task("Plan")).await.unwrap();
    // Cannot see the item.
    assert!(matches!(
        w.items.invite(can, task.id, assignee(ayse)).await,
        Err(AppError::NotFound)
    ));
    w.grant(RelationTuple::new(task.id, R::Assignee, can)).await;
    // A member without SHARE.
    assert!(matches!(
        w.items.invite(can, task.id, assignee(ayse)).await,
        Err(AppError::Forbidden)
    ));

    let invitation = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    for other in [ali, can] {
        assert!(matches!(
            w.items.accept_invitation(other, invitation.id).await,
            Err(AppError::NotFound)
        ));
        assert!(matches!(
            w.items.decline_invitation(other, invitation.id).await,
            Err(AppError::NotFound)
        ));
    }
    assert!(w.items.pending_invitations(can).await.unwrap().is_empty());
}

#[tokio::test]
async fn invitations_reject_bad_targets_and_duplicates() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("Plan")).await.unwrap();
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    assert!(matches!(
        w.items.invite(ali, task.id, assignee(ali)).await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        w.items.invite(ali, page.id, assignee(ayse)).await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        w.items
            .invite(ali, task.id, invite_as(ayse, items::InvitationRole::Editor))
            .await,
        Err(AppError::Validation(_))
    ));

    let invitation = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    assert!(matches!(
        w.items.invite(ali, task.id, assignee(ayse)).await,
        Err(AppError::Conflict(_))
    ));
    w.items
        .accept_invitation(ayse, invitation.id)
        .await
        .unwrap();
    assert!(
        matches!(
            w.items.invite(ali, task.id, assignee(ayse)).await,
            Err(AppError::Conflict(_))
        ),
        "already a member"
    );
}

#[tokio::test]
async fn accepting_expires_an_invitation_that_is_no_longer_valid() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);

    // V1 stopped holding.
    let unfriended = w.items.create(ali, World::task("A")).await.unwrap();
    let first = w
        .items
        .invite(ali, unfriended.id, assignee(ayse))
        .await
        .unwrap();
    w.sharing.unfriend(ali, ayse);
    assert!(matches!(
        w.items.accept_invitation(ayse, first.id).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        w.items.get(ayse, unfriended.id).await,
        Err(AppError::NotFound)
    ));
    assert!(w.items.pending_invitations(ayse).await.unwrap().is_empty());

    // The item went to the trash: hidden from the inbox, and it cannot be accepted.
    w.sharing.befriend(ali, ayse);
    let trashed = w.items.create(ali, World::task("B")).await.unwrap();
    let second = w
        .items
        .invite(ali, trashed.id, assignee(ayse))
        .await
        .unwrap();
    w.items.set_trashed(ali, trashed.id, true).await.unwrap();
    assert!(w.items.pending_invitations(ayse).await.unwrap().is_empty());
    assert!(matches!(
        w.items.accept_invitation(ayse, second.id).await,
        Err(AppError::Conflict(_))
    ));
    w.items.set_trashed(ali, trashed.id, false).await.unwrap();
    assert!(matches!(
        w.items.get(ayse, trashed.id).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn purging_an_item_deletes_its_invitations() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("Plan")).await.unwrap();
    w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    w.items.set_trashed(ali, task.id, true).await.unwrap();
    let later = start() + TRASH_RETENTION + Duration::hours(1);
    assert_eq!(w.items.purge_expired(later).await.unwrap(), 1);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM invitations")
        .fetch_one(w.db.pool())
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn invitation_endpoints() {
    let w = World::new().await;
    let (ali, ali_token) = w.user("inviter@example.com").await;
    let (ayse, ayse_token) = w.user("invitee@example.com").await;
    w.sharing.befriend(ali, ayse);
    let response = w
        .call(
            Method::POST,
            "/items",
            &ali_token,
            Some(json!({"kind": "task", "title": "Shared"})),
        )
        .await;
    let task = json_body(response).await["id"].as_str().unwrap().to_owned();

    let response = w
        .call(
            Method::POST,
            &format!("/items/{task}/invitations"),
            &ali_token,
            Some(json!({"to_user": ayse, "role": "assignee"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = json_body(response).await["id"].as_str().unwrap().to_owned();

    let inbox = json_body(w.call(Method::GET, "/invitations", &ayse_token, None).await).await;
    assert_eq!(inbox[0]["item_title"], "Shared");
    assert_eq!(inbox[0]["status"], "pending");

    let uri = format!("/invitations/{id}/accept");
    let response = w.call(Method::POST, &uri, &ali_token, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = w.call(Method::POST, &uri, &ayse_token, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["status"], "accepted");
    let response = w
        .call(Method::GET, &format!("/items/{task}"), &ayse_token, None)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
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

// --- V13: links whose page owner lost the target (T1.2.8) ---------------------------------------

async fn revoke(w: &World, tuple: RelationTuple) {
    let mut tx = w.db.pool().begin().await.unwrap();
    w.access.revoke(&mut tx, tuple).await.unwrap();
    tx.commit().await.unwrap();
}

/// Delivers everything in the outbox, including events published by the handlers.
async fn deliver(w: &World) {
    let mut dispatcher = db::outbox::Dispatcher::new(db::outbox::DispatchConfig::default());
    items::register_link_cleanup(&mut dispatcher, w.items.clone());
    for _ in 0..3 {
        dispatcher.dispatch_batch(w.db.pool()).await.unwrap();
    }
}

#[tokio::test]
async fn v13_owner_losing_the_target_drops_the_link_and_inherited_access() {
    let w = World::new().await;
    let (ali, ayse, can) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let page = w
        .items
        .create(ali, of_kind("page", "Private"))
        .await
        .unwrap();
    let task = w.items.create(ayse, World::task("Ayse's")).await.unwrap();
    let membership = RelationTuple::new(task.id, R::Watcher, ali);
    w.grant(membership).await;
    w.grant(RelationTuple::new(task.id, R::Watcher, can)).await;
    w.items.link(ali, page.id, task.id).await.unwrap();
    assert_eq!(w.items.get(can, page.id).await.unwrap().id, page.id);

    revoke(&w, membership).await;
    deliver(&w).await;

    for viewer in [ayse, can] {
        assert!(
            matches!(w.items.get(viewer, page.id).await, Err(AppError::NotFound)),
            "the page closes to everyone who saw it through the link"
        );
    }
    assert_eq!(w.items.get(ali, page.id).await.unwrap().id, page.id);
    assert!(w.items.links(ayse, task.id).await.unwrap().is_empty());
    // A repeated delivery changes nothing.
    let again = w
        .items
        .drop_links_of_lost_owners(task.id, &[ali])
        .await
        .unwrap();
    assert_eq!(again, 0);
}

#[tokio::test]
async fn v13_link_stays_while_the_owner_still_sees_the_target() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    let task = w.items.create(ayse, World::task("Task")).await.unwrap();
    let watcher = RelationTuple::new(task.id, R::Watcher, ali);
    w.grant(watcher).await;
    w.grant(RelationTuple::new(task.id, R::Assignee, ali)).await;
    w.items.link(ali, page.id, task.id).await.unwrap();

    // One path is gone, another remains; even a stale event must not drop the link.
    revoke(&w, watcher).await;
    let dropped = w
        .items
        .drop_links_of_lost_owners(task.id, &[ali])
        .await
        .unwrap();
    assert_eq!(dropped, 0);
    assert_eq!(w.items.links(ali, page.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn v13_someone_else_losing_the_target_keeps_the_link() {
    let w = World::new().await;
    let (ali, can) = (Uuid::now_v7(), Uuid::now_v7());
    let page = w.items.create(ali, of_kind("page", "Notes")).await.unwrap();
    let task = w.items.create(ali, World::task("Task")).await.unwrap();
    let watcher = RelationTuple::new(task.id, R::Watcher, can);
    w.grant(watcher).await;
    w.items.link(ali, page.id, task.id).await.unwrap();

    revoke(&w, watcher).await;
    deliver(&w).await;

    assert_eq!(w.items.links(ali, page.id).await.unwrap().len(), 1);
    assert!(matches!(
        w.items.get(can, page.id).await,
        Err(AppError::NotFound)
    ));
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

// --- AK4: emoji reactions to friends' completions (T3.5.2) -------------------------------------

/// Ali's task shared with Ayşe as an assignee; Ayşe completes it. The completion event's id.
async fn shared_completion(w: &World, ali: Uuid, ayse: Uuid) -> (Uuid, Uuid) {
    w.sharing.befriend(ali, ayse);
    let task = w.items.create(ali, World::task("Ortak")).await.unwrap();
    let invitation = w.items.invite(ali, task.id, assignee(ayse)).await.unwrap();
    w.items
        .accept_invitation(ayse, invitation.id)
        .await
        .unwrap();
    let mut tx = w.db.pool().begin().await.unwrap();
    w.items
        .record_completed(&mut tx, ayse, task.id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let page = kernel::PageRequest::from_params(kernel::PageParams {
        limit: Some(50),
        cursor: None,
    })
    .unwrap();
    let completion = w
        .items
        .item_activity(ali, task.id, &page)
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|a| a.kind == items::ActivityKind::Completed)
        .unwrap()
        .id;
    (task.id, completion)
}

fn emoji(e: &str) -> items::ReactRequest {
    items::ReactRequest { emoji: e.into() }
}

#[tokio::test]
async fn ak4_a_friend_reacts_to_a_completion_once_and_can_change_it() {
    let w = World::new().await;
    let (ali, ayse) = (Uuid::now_v7(), Uuid::now_v7());
    let (task, completion) = shared_completion(&w, ali, ayse).await;

    let counts = w.items.react(ali, completion, emoji("🎉")).await.unwrap();
    assert_eq!(
        counts,
        [items::ReactionCount {
            emoji: "🎉".into(),
            count: 1,
            mine: true
        }]
    );
    // Reacting again replaces the reaction: still one per person.
    let counts = w.items.react(ali, completion, emoji("🔥")).await.unwrap();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].emoji, "🔥");
    // Ayşe sees the count in the activity, but not as hers and not who reacted.
    let page = kernel::PageRequest::from_params(kernel::PageParams {
        limit: Some(50),
        cursor: None,
    })
    .unwrap();
    let theirs = w.items.item_activity(ayse, task, &page).await.unwrap();
    let event = theirs.items.iter().find(|a| a.id == completion).unwrap();
    assert_eq!(
        event.reactions,
        [items::ReactionCount {
            emoji: "🔥".into(),
            count: 1,
            mine: false
        }]
    );
    let counts = w.items.unreact(ali, completion).await.unwrap();
    assert!(counts.is_empty());
}

#[tokio::test]
async fn ak4_only_visible_completions_of_friends_get_reactions() {
    let w = World::new().await;
    let (ali, ayse, stranger) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (_, completion) = shared_completion(&w, ali, ayse).await;

    // Someone who cannot see the item learns nothing, not even that the event exists (V9).
    assert!(matches!(
        w.items.react(stranger, completion, emoji("👏")).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.items.react(ali, Uuid::now_v7(), emoji("👏")).await,
        Err(AppError::NotFound)
    ));
    // One's own completion, another kind of event and an emoji not offered are refused.
    assert!(matches!(
        w.items.react(ayse, completion, emoji("👏")).await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        w.items.react(ali, completion, emoji("💩")).await,
        Err(AppError::Validation(_))
    ));
    let own = w.items.create(ali, World::task("Kendi")).await.unwrap();
    let page = kernel::PageRequest::from_params(kernel::PageParams {
        limit: Some(50),
        cursor: None,
    })
    .unwrap();
    let created = w
        .items
        .item_activity(ali, own.id, &page)
        .await
        .unwrap()
        .items[0]
        .id;
    assert!(matches!(
        w.items.react(ali, created, emoji("👏")).await,
        Err(AppError::Validation(_))
    ));
    // Still a member after the friendship ends, but no longer a friend: no reaction.
    w.sharing.unfriend(ali, ayse);
    assert!(matches!(
        w.items.react(ali, completion, emoji("👏")).await,
        Err(AppError::Forbidden)
    ));
}

// --- team items (T5.1.3, V12, TK4, V8, V9) -------------------------------------------------------

#[tokio::test]
async fn v12_team_items_over_http_leak_nothing() {
    let w = World::new().await;
    let (ali, ali_token) = w.user("ali@example.com").await;
    let (ayse, ayse_token) = w.user("ayse@example.com").await;
    let (_, outsider_token) = w.user("dis@example.com").await;
    let team = Uuid::now_v7();
    w.teams.join(team, ali, true, true);
    w.teams.join(team, ayse, true, false);
    w.grant(RelationTuple::new(team, R::TeamAdmin, ali)).await;
    w.grant(RelationTuple::new(team, R::TeamMember, ayse)).await;
    let body = json!({ "kind": "task", "title": "Rapor", "visibility": "assignees" });

    // Outsiders get the same answer for a real team and a made-up one.
    let real = w
        .call(
            Method::POST,
            &format!("/teams/{team}/items"),
            &outsider_token,
            Some(body.clone()),
        )
        .await;
    let fake = w
        .call(
            Method::POST,
            &format!("/teams/{}/items", Uuid::now_v7()),
            &outsider_token,
            Some(body.clone()),
        )
        .await;
    assert_eq!(real.status(), StatusCode::NOT_FOUND);
    assert_eq!(fake.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(real).await, json_body(fake).await);

    // A member without the assign right may not choose someone else.
    let assigning = json!({ "kind": "task", "title": "X", "assignees": [ali] });
    let response = w
        .call(
            Method::POST,
            &format!("/teams/{team}/items"),
            &ayse_token,
            Some(assigning),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = w
        .call(
            Method::POST,
            &format!("/teams/{team}/items"),
            &ali_token,
            Some(body),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let item = json_body(response).await;
    assert_eq!(item["owner"], json!({ "type": "team", "team_id": team }));
    assert_eq!(item["team_visibility"], "assignees");
    let id = item["id"].as_str().unwrap();

    // Ayşe is a member but not involved: the item does not exist for her, whatever she asks.
    let missing = Uuid::now_v7();
    for uri in [format!("/items/{id}"), format!("/items/{id}/visibility")] {
        let method = if uri.ends_with("visibility") {
            Method::PUT
        } else {
            Method::GET
        };
        let hidden = w
            .call(
                method.clone(),
                &uri,
                &ayse_token,
                Some(json!({ "visibility": "all" })),
            )
            .await;
        let absent = w
            .call(
                method,
                &uri.replace(id, &missing.to_string()),
                &ayse_token,
                Some(json!({ "visibility": "all" })),
            )
            .await;
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(json_body(hidden).await, json_body(absent).await, "{uri}");
    }
    let listed = json_body(w.call(Method::GET, "/items", &ayse_token, None).await).await;
    assert!(listed.as_array().unwrap().iter().all(|i| i["id"] != id));

    // The admin opens it to the whole team (V8): Ayşe sees it at once.
    let response = w
        .call(
            Method::PUT,
            &format!("/items/{id}/visibility"),
            &ali_token,
            Some(json!({ "visibility": "all" })),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["team_visibility"], "all");
    let seen = w
        .call(Method::GET, &format!("/items/{id}"), &ayse_token, None)
        .await;
    assert_eq!(seen.status(), StatusCode::OK);

    // A personal item has no mode; personal items say so in their DTO.
    let personal = w.items.create(ali, World::task("Kişisel")).await.unwrap();
    let response = w
        .call(
            Method::PUT,
            &format!("/items/{}/visibility", personal.id),
            &ali_token,
            Some(json!({ "visibility": "all" })),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(personal.team_visibility, None);
}

// --- team tags (T5.1.4, E3) ---------------------------------------------------------------------

#[tokio::test]
async fn e3_team_tags_are_visible_to_team_members_only() {
    let w = World::new().await;
    let (lead, member, outsider) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let team = Uuid::now_v7();
    w.teams.join(team, lead, true, true);
    w.teams.join(team, member, true, false);
    w.grant(RelationTuple::new(team, R::TeamAdmin, lead)).await;
    w.grant(RelationTuple::new(team, R::TeamMember, member))
        .await;

    // Only roles with "manage tags" create them; outsiders cannot tell the team exists.
    assert!(matches!(
        w.items.create_team_tag(member, team, tag("Sprint")).await,
        Err(AppError::Forbidden)
    ));
    for team_id in [team, Uuid::now_v7()] {
        assert!(matches!(
            w.items
                .create_team_tag(outsider, team_id, tag("Sprint"))
                .await,
            Err(AppError::NotFound)
        ));
    }
    let sprint = w
        .items
        .create_team_tag(lead, team, tag("Sprint"))
        .await
        .unwrap();
    assert_eq!(sprint.team_id, Some(team));
    // The same name is fine for a person and for the team, but not twice in the team.
    let personal = w.items.create_tag(member, tag("Sprint")).await.unwrap();
    assert_eq!(personal.team_id, None);
    assert!(matches!(
        w.items.create_team_tag(lead, team, tag("sprint")).await,
        Err(AppError::Conflict(_))
    ));

    // E4: one list holds personal and team tags; outsiders see none of the team's.
    let tag_ids = |tags: Vec<items::Tag>| tags.into_iter().map(|t| t.id).collect::<HashSet<_>>();
    assert_eq!(
        tag_ids(w.items.list_tags(member).await.unwrap()),
        HashSet::from([sprint.id, personal.id])
    );
    assert!(w.items.list_tags(outsider).await.unwrap().is_empty());

    // A team item tagged by a member with EDIT; every teammate who sees it sees the tag.
    let new = items::CreateTeamItemRequest {
        kind: "task".into(),
        title: "Rapor".into(),
        description: None,
        visibility: None,
        assignees: None,
        watchers: None,
    }
    .validate()
    .unwrap();
    let item = w.items.create_team_item(lead, team, new).await.unwrap();
    w.items
        .attach_tag(member, item.id, sprint.id)
        .await
        .unwrap();
    w.items
        .attach_tag(member, item.id, personal.id)
        .await
        .unwrap();
    assert_eq!(
        w.items.item_tags(lead, item.id).await.unwrap(),
        vec![sprint.clone()]
    );
    assert_eq!(
        tag_ids(w.items.item_tags(member, item.id).await.unwrap()),
        HashSet::from([sprint.id, personal.id])
    );
    let tagged = w
        .items
        .items_by_tag(lead, sprint.id, ListState::Active)
        .await
        .unwrap();
    assert_eq!(ids(&tagged), vec![item.id]);

    // An outsider who sees the item (a watcher) still does not see the team tag, and the tag
    // behaves as if it did not exist for them.
    w.grant(RelationTuple::new(item.id, R::Watcher, outsider))
        .await;
    assert!(
        w.items
            .item_tags(outsider, item.id)
            .await
            .unwrap()
            .is_empty()
    );
    let changes = items::TagChanges {
        name: Some("X".into()),
        color: None,
    };
    for result in [
        w.items.attach_tag(outsider, item.id, sprint.id).await,
        w.items.detach_tag(outsider, item.id, sprint.id).await,
        w.items.delete_tag(outsider, sprint.id).await,
        w.items
            .update_tag(outsider, sprint.id, changes.clone())
            .await
            .map(|_| ()),
        w.items
            .items_by_tag(outsider, sprint.id, ListState::Active)
            .await
            .map(|_| ()),
    ] {
        assert!(matches!(result, Err(AppError::NotFound)));
    }

    // Members without "manage tags" may use the tag but not change it.
    assert!(matches!(
        w.items.update_tag(member, sprint.id, changes.clone()).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        w.items.delete_tag(member, sprint.id).await,
        Err(AppError::Forbidden)
    ));
    let renamed = w.items.update_tag(lead, sprint.id, changes).await.unwrap();
    assert_eq!(renamed.name, "X");
    w.items.delete_tag(lead, sprint.id).await.unwrap();
    assert_eq!(
        tag_ids(w.items.item_tags(member, item.id).await.unwrap()),
        HashSet::from([personal.id])
    );
}

/// A team tag changes what teammates see on an item, so it needs EDIT there; a viewer may only
/// use their personal tags.
#[tokio::test]
async fn e3_attaching_a_team_tag_needs_edit() {
    let w = World::new().await;
    let (lead, member) = (Uuid::now_v7(), Uuid::now_v7());
    let team = Uuid::now_v7();
    w.teams.join(team, lead, true, true);
    w.teams.join(team, member, true, false);
    let sprint = w
        .items
        .create_team_tag(lead, team, tag("Sprint"))
        .await
        .unwrap();
    let item = w.items.create(lead, World::task("Kişisel")).await.unwrap();
    w.grant(RelationTuple::new(item.id, R::Watcher, member))
        .await;
    assert!(matches!(
        w.items.attach_tag(member, item.id, sprint.id).await,
        Err(AppError::Forbidden)
    ));
    let mine = w.items.create_tag(member, tag("Benim")).await.unwrap();
    w.items.attach_tag(member, item.id, mine.id).await.unwrap();
    w.items.attach_tag(lead, item.id, sprint.id).await.unwrap();
    assert!(matches!(
        w.items.detach_tag(member, item.id, sprint.id).await,
        Err(AppError::Forbidden)
    ));
    // Team tags filter and are searched for members (E3, AR1).
    let filter = items::ItemFilter {
        tag_ids: Some(vec![sprint.id]),
        ..items::ItemFilter::default()
    };
    assert_eq!(filtered(&w, member, filter).await, HashSet::from([item.id]));
}

#[tokio::test]
async fn e3_team_tag_endpoints() {
    let w = World::new().await;
    let (lead, lead_token) = w.user("lead@example.com").await;
    let (member, member_token) = w.user("member@example.com").await;
    let (_, outsider_token) = w.user("outsider@example.com").await;
    let team = Uuid::now_v7();
    w.teams.join(team, lead, true, true);
    w.teams.join(team, member, true, false);
    let body = json!({ "name": "Sprint", "color": "#22C55E" });

    let real = w
        .call(
            Method::POST,
            &format!("/teams/{team}/tags"),
            &outsider_token,
            Some(body.clone()),
        )
        .await;
    let fake = w
        .call(
            Method::POST,
            &format!("/teams/{}/tags", Uuid::now_v7()),
            &outsider_token,
            Some(body.clone()),
        )
        .await;
    assert_eq!(real.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(real).await, json_body(fake).await);
    let response = w
        .call(
            Method::POST,
            &format!("/teams/{team}/tags"),
            &member_token,
            Some(body.clone()),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = w
        .call(
            Method::POST,
            &format!("/teams/{team}/tags"),
            &lead_token,
            Some(body),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    assert_eq!(created["team_id"], json!(team));
    assert_eq!(created["color"], "#22c55e");
    let id = created["id"].as_str().unwrap();

    let listed = json_body(w.call(Method::GET, "/tags", &member_token, None).await).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], id);
    let listed = json_body(w.call(Method::GET, "/tags", &outsider_token, None).await).await;
    assert!(listed.as_array().unwrap().is_empty());

    // Outsiders get the same 404 for the team's tag as for a missing one.
    let hidden = w
        .call(
            Method::PATCH,
            &format!("/tags/{id}"),
            &outsider_token,
            Some(json!({ "name": "X" })),
        )
        .await;
    let absent = w
        .call(
            Method::PATCH,
            &format!("/tags/{}", Uuid::now_v7()),
            &outsider_token,
            Some(json!({ "name": "X" })),
        )
        .await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(hidden).await, json_body(absent).await);
}

// --- activity filters (T5.2.7, A2, TK6) ----------------------------------------------------------

#[tokio::test]
async fn a2_activity_filters_only_narrow_what_the_reader_sees() {
    use items::{ActivityFilter, ActivityKind as K};
    let w = World::new().await;
    let (ali, ayse, can) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let team = Uuid::now_v7();
    for (user, relation) in [(ali, R::TeamAdmin), (ayse, R::TeamMember)] {
        w.teams.join(team, user, true, true);
        w.grant(RelationTuple::new(team, relation, user)).await;
    }
    let new = |title: &str, mode| {
        items::CreateTeamItemRequest {
            kind: "task".into(),
            title: title.into(),
            description: None,
            visibility: Some(mode),
            assignees: None,
            watchers: None,
        }
        .validate()
        .unwrap()
    };
    let open = w
        .items
        .create_team_item(ali, team, new("Açık", items::TeamVisibilityMode::All))
        .await
        .unwrap();
    // Only managers and above (and the creator) see this one.
    w.clock.set(start() + Duration::minutes(1));
    let closed = w
        .items
        .create_team_item(
            ali,
            team,
            new("Kapalı", items::TeamVisibilityMode::Assignees),
        )
        .await
        .unwrap();
    w.clock.set(start() + Duration::minutes(2));
    let personal = w.items.create(ayse, World::task("Kişisel")).await.unwrap();
    w.clock.set(start() + Duration::minutes(3));
    w.items
        .add_team_member(ali, open.id, ayse, items::InvitationRole::Watcher)
        .await
        .unwrap();

    let feed = |actor, filter: ActivityFilter| {
        let w = &w;
        async move {
            w.items
                .filtered_activity(actor, &filter, &first_page())
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|e| (e.item_id, e.kind))
                .collect::<Vec<_>>()
        }
    };
    let by_team = ActivityFilter {
        team_ids: Some(vec![team]),
        ..ActivityFilter::default()
    };
    // Ayşe: the team's events she can see; not the closed item, not her personal one.
    assert_eq!(
        feed(ayse, by_team.clone()).await,
        vec![(open.id, K::Shared), (open.id, K::Created)]
    );
    assert_eq!(
        feed(ali, by_team.clone()).await.len(),
        3,
        "Ali sees the closed one too"
    );
    assert!(feed(can, by_team).await.is_empty(), "outsiders see nothing");
    // People, kinds and time narrow further.
    assert_eq!(
        feed(
            ayse,
            ActivityFilter {
                people: Some(vec![ayse]),
                ..ActivityFilter::default()
            }
        )
        .await,
        vec![(personal.id, K::Created)]
    );
    assert_eq!(
        feed(
            ali,
            ActivityFilter {
                kinds: Some(vec![K::Created]),
                since: Some(start() + Duration::minutes(1)),
                until: Some(start() + Duration::minutes(2)),
                ..ActivityFilter::default()
            }
        )
        .await,
        vec![(closed.id, K::Created)]
    );
    // Empty lists filter nothing; bad filters are refused.
    assert_eq!(
        feed(
            ayse,
            ActivityFilter {
                kinds: Some(vec![]),
                ..ActivityFilter::default()
            }
        )
        .await
        .len(),
        3
    );
    for bad in [
        ActivityFilter {
            people: Some(
                (0..=items::ACTIVITY_FILTER_MAX_VALUES)
                    .map(|_| Uuid::now_v7())
                    .collect(),
            ),
            ..ActivityFilter::default()
        },
        ActivityFilter {
            since: Some(start()),
            until: Some(start() - Duration::minutes(1)),
            ..ActivityFilter::default()
        },
    ] {
        assert!(matches!(
            w.items.filtered_activity(ali, &bad, &first_page()).await,
            Err(AppError::Validation(_))
        ));
    }
}

#[tokio::test]
async fn a2_activity_filter_over_http() {
    let w = World::new().await;
    let (ali, token) = w.user("ali@example.com").await;
    let team = Uuid::now_v7();
    w.teams.join(team, ali, true, true);
    w.grant(RelationTuple::new(team, R::TeamAdmin, ali)).await;
    let new = items::CreateTeamItemRequest {
        kind: "task".into(),
        title: "Takım".into(),
        description: None,
        visibility: None,
        assignees: None,
        watchers: None,
    }
    .validate()
    .unwrap();
    let item = w.items.create_team_item(ali, team, new).await.unwrap();
    w.items.create(ali, World::task("Kişisel")).await.unwrap();
    let filter = serde_json::json!({ "team_ids": [team], "kinds": ["created"] }).to_string();
    let uri = format!("/activity?filter={}", urlencode(&filter));
    let response = w.call(Method::GET, &uri, &token, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let events = body["items"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["item_id"], json!(item.id));
    for bad in ["{\"nope\":1}", "{\"kinds\":[\"exploded\"]}", "not json"] {
        let uri = format!("/activity?filter={}", urlencode(bad));
        let response = w.call(Method::GET, &uri, &token, None).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
}

fn urlencode(raw: &str) -> String {
    raw.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
