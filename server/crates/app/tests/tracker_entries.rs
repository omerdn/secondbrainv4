//! Tracker entries API (T1.3.4, PRD T2) through the real idempotency layer, against Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation as R, RelationTuple};
use app::idempotency::{self, Idempotency, KEY_HEADER, REPLAYED_HEADER};
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use chrono::{DateTime, Duration, TimeZone, Utc};
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::{EmailSettings, IdentityService};
use items::{ItemsAccessContext, ItemsService, NewItem};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tower::ServiceExt;
use tracking::http::{TrackingHttpState, router};
use tracking::{Aggregation, Direction, NewTracker, OwnerKind, Period, TrackerEntries, repo};
use uuid::Uuid;

fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap()
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
        let entries = TrackerEntries::new(
            db.pool().clone(),
            shared.clone(),
            access.permissions(),
            items.clone(),
            identity.clone(),
        );
        let routes = router(TrackingHttpState {
            entries,
            identity: identity.clone(),
        });
        let protected = idempotency::protect(
            routes,
            Idempotency::new(db.pool().clone(), identity.clone(), shared),
        );
        let router = identity::http::router(identity).merge(protected);
        Self {
            db,
            clock,
            access,
            items,
            router,
        }
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

    async fn tracker(&self, owner: Uuid) -> Uuid {
        let new = NewItem {
            kind: "tracker".into(),
            title: "Reading".into(),
            description: None,
        };
        let item_id = self.items.create(owner, new).await.unwrap().id;
        let definition = NewTracker {
            item_id,
            unit: "sayfa".into(),
            target_value: Decimal::from(300),
            period: Period::Total,
            aggregation: Aggregation::Sum,
            direction: Direction::Increase,
            owner_kind: OwnerKind::Standalone,
        };
        repo::insert_tracker(self.db.pool(), &definition, start())
            .await
            .unwrap();
        item_id
    }

    async fn grant(&self, tuple: RelationTuple) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.access.grant(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    async fn call(
        &self,
        method: Method,
        uri: &str,
        token: &str,
        key: Option<&str>,
        body: Option<Value>,
    ) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(key) = key {
            request = request.header(KEY_HEADER, key);
        }
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

    async fn post_entry(&self, tracker: Uuid, token: &str, body: Value) -> Response {
        let key = Uuid::now_v7().to_string();
        self.call(
            Method::POST,
            &format!("/trackers/{tracker}/entries"),
            token,
            Some(&key),
            Some(body),
        )
        .await
    }

    async fn delete_entry(&self, tracker: Uuid, entry: &str, token: &str) -> StatusCode {
        let key = Uuid::now_v7().to_string();
        self.call(
            Method::DELETE,
            &format!("/trackers/{tracker}/entries/{entry}"),
            token,
            Some(&key),
            None,
        )
        .await
        .status()
    }

    async fn list(&self, tracker: Uuid, token: &str, query: &str) -> Value {
        let uri = format!("/trackers/{tracker}/entries{query}");
        let response = self.call(Method::GET, &uri, token, None, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn the_same_idempotency_key_creates_one_entry() {
    let w = World::new().await;
    let (user, token) = w.user("idem@example.com").await;
    let tracker = w.tracker(user).await;
    let uri = format!("/trackers/{tracker}/entries");
    let body = json!({"value": "12.50", "note": "  chapter 3 "});

    let first = w
        .call(
            Method::POST,
            &uri,
            &token,
            Some("key-1"),
            Some(body.clone()),
        )
        .await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first = json_body(first).await;
    assert_eq!(first["value"], "12.5");
    assert_eq!(first["note"], "chapter 3");
    assert_eq!(first["user_id"], user.to_string());

    let replay = w
        .call(Method::POST, &uri, &token, Some("key-1"), Some(body))
        .await;
    assert_eq!(replay.status(), StatusCode::CREATED);
    assert!(replay.headers().contains_key(REPLAYED_HEADER));
    assert_eq!(json_body(replay).await["id"], first["id"]);
    let listed = w.list(tracker, &token, "").await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);

    // A mutation without a key is refused.
    let response = w
        .call(
            Method::POST,
            &uri,
            &token,
            None,
            Some(json!({"value": "1"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn permissions_come_from_the_access_engine() {
    let w = World::new().await;
    let (owner, owner_token) = w.user("owner@example.com").await;
    let (editor, editor_token) = w.user("editor@example.com").await;
    let (watcher, watcher_token) = w.user("watcher@example.com").await;
    let (_, stranger_token) = w.user("stranger@example.com").await;
    let tracker = w.tracker(owner).await;
    w.grant(RelationTuple::new(tracker, R::Assignee, editor))
        .await;
    w.grant(RelationTuple::new(tracker, R::Watcher, watcher))
        .await;
    let body = json!({"value": "1"});

    // Strangers learn nothing (V9).
    let response = w.post_entry(tracker, &stranger_token, body.clone()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let uri = format!("/trackers/{tracker}/entries");
    let response = w.call(Method::GET, &uri, &stranger_token, None, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // A watcher can read but not write.
    let response = w.post_entry(tracker, &watcher_token, body.clone()).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = w.post_entry(tracker, &editor_token, body.clone()).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let editors = json_body(response).await["id"].as_str().unwrap().to_owned();
    let response = w.post_entry(tracker, &owner_token, body).await;
    let owners = json_body(response).await["id"].as_str().unwrap().to_owned();
    let listed = w.list(tracker, &watcher_token, "").await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);

    // Deleting: a watcher cannot, an editor only their own, the owner anyone's.
    assert_eq!(
        w.delete_entry(tracker, &editors, &watcher_token).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        w.delete_entry(tracker, &owners, &editor_token).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        w.delete_entry(tracker, &editors, &editor_token).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        w.delete_entry(tracker, &editors, &editor_token).await,
        StatusCode::NOT_FOUND,
        "already deleted"
    );
    let response = w
        .post_entry(tracker, &editor_token, json!({"value": "2"}))
        .await;
    let another = json_body(response).await["id"].as_str().unwrap().to_owned();
    assert_eq!(
        w.delete_entry(tracker, &another, &owner_token).await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn local_date_follows_the_users_time_zone() {
    let w = World::new().await;
    let (user, token) = w.user("tz@example.com").await;
    sqlx::query("UPDATE users SET timezone = 'America/New_York' WHERE id = $1")
        .bind(user)
        .execute(w.db.pool())
        .await
        .unwrap();
    let tracker = w.tracker(user).await;
    // 22:00 on May 1 in New York, already May 2 in UTC.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 5, 2, 2, 0, 0).unwrap());
    let response = w.post_entry(tracker, &token, json!({"value": "5"})).await;
    assert_eq!(json_body(response).await["local_date"], "2026-05-01");

    // A past moment counts for its own day.
    let response = w
        .post_entry(
            tracker,
            &token,
            json!({"value": "1", "occurred_at": "2026-04-20T12:00:00Z"}),
        )
        .await;
    assert_eq!(json_body(response).await["local_date"], "2026-04-20");
}

#[tokio::test]
async fn invalid_entries_are_refused() {
    let w = World::new().await;
    let (user, token) = w.user("bad@example.com").await;
    let tracker = w.tracker(user).await;
    for body in [
        json!({"value": "1.23456"}),
        json!({"value": "10000000000000000"}),
        json!({"value": "1", "occurred_at": "2026-05-02T12:00:00Z"}),
        json!({"value": "1", "note": "x".repeat(501)}),
    ] {
        let response = w.post_entry(tracker, &token, body.clone()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
    }
    // A JSON number would pass through a float.
    let response = w.post_entry(tracker, &token, json!({"value": 1.5})).await;
    assert!(response.status().is_client_error());
    assert!(
        w.list(tracker, &token, "").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn items_without_a_tracker_or_in_the_trash() {
    let w = World::new().await;
    let (user, token) = w.user("trash@example.com").await;
    let plain = w
        .items
        .create(
            user,
            NewItem {
                kind: "tracker".into(),
                title: "No definition".into(),
                description: None,
            },
        )
        .await
        .unwrap();
    let response = w.post_entry(plain.id, &token, json!({"value": "1"})).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let uri = format!("/trackers/{}/entries", plain.id);
    let response = w.call(Method::GET, &uri, &token, None, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let tracker = w.tracker(user).await;
    let response = w.post_entry(tracker, &token, json!({"value": "1"})).await;
    let entry = json_body(response).await["id"].as_str().unwrap().to_owned();
    w.items.set_trashed(user, tracker, true).await.unwrap();
    let response = w.post_entry(tracker, &token, json!({"value": "1"})).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        w.delete_entry(tracker, &entry, &token).await,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn an_entry_touches_the_item_but_a_refused_one_does_not() {
    let w = World::new().await;
    let (user, token) = w.user("touch@example.com").await;
    let tracker = w.tracker(user).await;
    let later = start() + Duration::days(2);
    w.clock.set(later);
    let response = w
        .post_entry(tracker, &token, json!({"value": "1.23456"}))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let item = w.items.get(user, tracker).await.unwrap();
    assert_eq!(item.last_touched_at, start());

    w.post_entry(tracker, &token, json!({"value": "1"})).await;
    let item = w.items.get(user, tracker).await.unwrap();
    assert_eq!(item.last_touched_at, later);
}

#[tokio::test]
async fn entries_are_listed_newest_first_by_page_and_day() {
    let w = World::new().await;
    let (user, token) = w.user("list@example.com").await;
    let tracker = w.tracker(user).await;
    for day in 1..=3 {
        w.clock
            .set(Utc.with_ymd_and_hms(2026, 5, day, 12, 0, 0).unwrap());
        w.post_entry(tracker, &token, json!({"value": day.to_string()}))
            .await;
    }
    let first = w.list(tracker, &token, "?limit=2").await;
    assert_eq!(first["items"][0]["value"], "3");
    assert_eq!(first["items"][1]["value"], "2");
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = w
        .list(tracker, &token, &format!("?limit=2&cursor={cursor}"))
        .await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["items"][0]["value"], "1");
    assert!(second["next_cursor"].is_null());

    let ranged = w
        .list(tracker, &token, "?from=2026-05-02&to=2026-05-02")
        .await;
    assert_eq!(ranged["items"].as_array().unwrap().len(), 1);
    assert_eq!(ranged["items"][0]["value"], "2");
}

// --- definitions (T1.3.5, T1, K1) ----------------------------------------------------------------

fn definition() -> Value {
    json!({
        "title": "  Kitap ",
        "unit": " sayfa ",
        "target_value": "300.50",
        "period": {"type": "range", "start": "2026-05-01", "end": "2026-05-31"},
        "aggregation": "sum",
        "direction": "increase",
    })
}

async fn send(w: &World, method: Method, uri: &str, token: &str, body: Value) -> Response {
    let key = Uuid::now_v7().to_string();
    w.call(method, uri, token, Some(&key), Some(body)).await
}

#[tokio::test]
async fn a_tracker_is_created_with_its_item_and_takes_entries() {
    let w = World::new().await;
    let (user, token) = w.user("define@example.com").await;
    let response = send(&w, Method::POST, "/trackers", &token, definition()).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    assert_eq!(created["item"]["kind"], "tracker");
    assert_eq!(created["item"]["title"], "Kitap");
    assert_eq!(created["unit"], "sayfa");
    assert_eq!(created["target_value"], "300.5");
    assert_eq!(created["period"]["type"], "range");
    assert_eq!(created["period"]["end"], "2026-05-31");
    assert_eq!(created["aggregation"], "sum");
    assert_eq!(created["direction"], "increase");
    assert_eq!(created["owner_kind"], "standalone");
    let id: Uuid = created["item"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(w.items.get(user, id).await.unwrap().id, id);

    let uri = format!("/trackers/{id}");
    let read = json_body(w.call(Method::GET, &uri, &token, None, None).await).await;
    assert_eq!(read, created);
    let response = w.post_entry(id, &token, json!({"value": "10"})).await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn a_failed_definition_leaves_no_item_behind() {
    let w = World::new().await;
    let (_, token) = w.user("atomic@example.com").await;
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM items")
            .fetch_one(w.db.pool())
            .await
            .unwrap()
    };
    for (field, value) in [
        ("unit", json!("  ")),
        ("target_value", json!("0")),
        ("target_value", json!("1.23456")),
        ("title", json!("  ")),
        (
            "period",
            json!({"type": "range", "start": "2026-05-02", "end": "2026-05-01"}),
        ),
    ] {
        let mut body = definition();
        body[field] = value;
        let response = send(&w, Method::POST, "/trackers", &token, body).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{field}"
        );
    }
    assert_eq!(count().await, 0);

    // The database refuses the definition after the item was written: both roll back.
    sqlx::query("ALTER TABLE trackers ADD CONSTRAINT test_refuse CHECK (unit <> 'boom')")
        .execute(w.db.pool())
        .await
        .unwrap();
    let mut body = definition();
    body["unit"] = json!("boom");
    let response = send(&w, Method::POST, "/trackers", &token, body).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(count().await, 0);
}

#[tokio::test]
async fn reading_needs_view_and_changing_needs_edit() {
    let w = World::new().await;
    let (owner, _) = w.user("perm-owner@example.com").await;
    let (watcher, watcher_token) = w.user("perm-watcher@example.com").await;
    let (editor, editor_token) = w.user("perm-editor@example.com").await;
    let (_, stranger_token) = w.user("perm-stranger@example.com").await;
    let id = w.tracker(owner).await;
    w.grant(RelationTuple::new(id, R::Watcher, watcher)).await;
    w.grant(RelationTuple::new(id, R::Assignee, editor)).await;
    let uri = format!("/trackers/{id}");
    let change = json!({"unit": "dakika"});

    let response = w.call(Method::GET, &uri, &stranger_token, None, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(&w, Method::PATCH, &uri, &stranger_token, change.clone()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = w.call(Method::GET, &uri, &watcher_token, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = send(&w, Method::PATCH, &uri, &watcher_token, change.clone()).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = send(&w, Method::PATCH, &uri, &editor_token, change).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["unit"], "dakika");
}

#[tokio::test]
async fn changing_a_definition_keeps_other_fields_and_touches_the_item() {
    let w = World::new().await;
    let (user, token) = w.user("change@example.com").await;
    let response = send(&w, Method::POST, "/trackers", &token, definition()).await;
    let id: Uuid = json_body(response).await["item"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let uri = format!("/trackers/{id}");
    let later = start() + Duration::hours(3);
    w.clock.set(later);

    let response = send(
        &w,
        Method::PATCH,
        &uri,
        &token,
        json!({"target_value": "2000", "direction": "limit"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let changed = json_body(response).await;
    assert_eq!(changed["target_value"], "2000");
    assert_eq!(changed["direction"], "limit");
    assert_eq!(changed["unit"], "sayfa");
    assert_eq!(changed["period"]["type"], "range");
    assert_eq!(w.items.get(user, id).await.unwrap().last_touched_at, later);

    // Leaving a range clears its dates.
    let response = send(
        &w,
        Method::PATCH,
        &uri,
        &token,
        json!({"period": {"type": "week"}}),
    )
    .await;
    assert_eq!(json_body(response).await["period"], json!({"type": "week"}));
    let stored = repo::get_tracker(w.db.pool(), id).await.unwrap().unwrap();
    assert_eq!(stored.period, Period::Week);

    let response = send(&w, Method::PATCH, &uri, &token, json!({})).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    w.items.set_trashed(user, id, true).await.unwrap();
    let response = send(&w, Method::PATCH, &uri, &token, json!({"unit": "x"})).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_item_without_a_definition_is_not_a_tracker() {
    let w = World::new().await;
    let (user, token) = w.user("plain@example.com").await;
    let task = w
        .items
        .create(
            user,
            NewItem {
                kind: "task".into(),
                title: "Task".into(),
                description: None,
            },
        )
        .await
        .unwrap();
    let uri = format!("/trackers/{}", task.id);
    let response = w.call(Method::GET, &uri, &token, None, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(&w, Method::PATCH, &uri, &token, json!({"unit": "x"})).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

async fn limit_tracker(w: &World, token: &str, period: Value) -> String {
    let response = send(
        w,
        Method::POST,
        "/trackers",
        token,
        json!({
            "title": "Budget",
            "unit": "TL",
            "target_value": "100",
            "period": period,
            "aggregation": "sum",
            "direction": "limit",
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["item"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn k2_the_summary_shows_use_and_what_is_left() {
    let w = World::new().await;
    let (_, token) = w.user("summary@example.com").await;
    let id = limit_tracker(&w, &token, json!({"type": "month"})).await;
    let tracker: Uuid = id.parse().unwrap();
    w.post_entry(tracker, &token, json!({"value": "30.5"}))
        .await;
    // Last month does not count.
    w.post_entry(
        tracker,
        &token,
        json!({"value": "99", "occurred_at": "2026-04-20T12:00:00Z"}),
    )
    .await;

    let uri = format!("/trackers/{id}/summary");
    let response = w.call(Method::GET, &uri, &token, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["period_start"], "2026-05-01");
    assert_eq!(body["period_end"], "2026-05-31");
    assert_eq!(body["value"], "30.5");
    assert_eq!(body["remaining"], "69.5");
    assert_eq!(body["exceeded_by"], "0");
    assert_eq!(body["over_limit"], false);

    w.post_entry(tracker, &token, json!({"value": "80"})).await;
    let response = w.call(Method::GET, &uri, &token, None, None).await;
    let body = json_body(response).await;
    assert_eq!(body["remaining"], "0");
    assert_eq!(body["exceeded_by"], "10.5");
    assert_eq!(body["over_limit"], true);
}

#[tokio::test]
async fn the_summary_period_follows_the_users_time_zone() {
    let w = World::new().await;
    let (user, token) = w.user("summary-tz@example.com").await;
    sqlx::query("UPDATE users SET timezone = 'America/New_York' WHERE id = $1")
        .bind(user)
        .execute(w.db.pool())
        .await
        .unwrap();
    let id = limit_tracker(&w, &token, json!({"type": "day"})).await;
    // 22:00 on May 1 in New York, already May 2 in UTC.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 5, 2, 2, 0, 0).unwrap());
    let uri = format!("/trackers/{id}/summary");
    let body = json_body(w.call(Method::GET, &uri, &token, None, None).await).await;
    assert_eq!(body["today"], "2026-05-01");
    assert_eq!(body["period_start"], "2026-05-01");
}

#[tokio::test]
async fn v9_a_stranger_cannot_read_the_summary() {
    let w = World::new().await;
    let (_, owner) = w.user("summary-owner@example.com").await;
    let (_, stranger) = w.user("summary-stranger@example.com").await;
    let id = limit_tracker(&w, &owner, json!({"type": "day"})).await;
    let uri = format!("/trackers/{id}/summary");
    let response = w.call(Method::GET, &uri, &stranger, None, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn r7_a_routines_tracker_cannot_be_written_directly() {
    let w = World::new().await;
    let (user, token) = w.user("routine-tracker@example.com").await;
    let new = NewItem {
        kind: "routine".into(),
        title: "Read".into(),
        description: None,
    };
    let item_id = w.items.create(user, new).await.unwrap().id;
    let definition = NewTracker {
        item_id,
        unit: "count".into(),
        target_value: Decimal::ONE,
        period: Period::Day,
        aggregation: Aggregation::Sum,
        direction: Direction::Increase,
        owner_kind: OwnerKind::Routine,
    };
    repo::insert_tracker(w.db.pool(), &definition, start())
        .await
        .unwrap();
    // Far outside the 3-day limit a routine allows.
    let response = w
        .post_entry(
            item_id,
            &token,
            json!({"value": "1", "occurred_at": "2026-01-01T12:00:00Z"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let uri = format!("/trackers/{item_id}");
    let patched = send(&w, Method::PATCH, &uri, &token, json!({"unit": "x"})).await;
    assert_eq!(patched.status(), StatusCode::CONFLICT);
}
