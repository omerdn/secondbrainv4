//! `PermissionService` and the access endpoints (T1.1.5, PRD V7, V9, V11) against a real
//! Postgres 17. Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::http::{AccessHttpState, router};
use access::{
    AccessConfig, AccessEngine, DefaultAccessContext, Permission, Perms, PreviewItem, Reason,
    Relation as R, RelationTuple,
};
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use chrono::{TimeZone, Utc};
use db::PgPool;
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::{EmailSettings, IdentityService};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

fn id() -> Uuid {
    Uuid::now_v7()
}

fn t(object: Uuid, relation: R, subject: Uuid) -> RelationTuple {
    RelationTuple::new(object, relation, subject)
}

fn engine() -> AccessEngine<DefaultAccessContext> {
    AccessEngine::new(DefaultAccessContext, AccessConfig::default())
}

async fn setup(pool: &PgPool, tuples: &[RelationTuple]) {
    let engine = engine();
    let mut tx = pool.begin().await.unwrap();
    for tuple in tuples {
        engine.grant(&mut tx, *tuple).await.unwrap();
    }
    tx.commit().await.unwrap();
}

// --- service --------------------------------------------------------------------------------

#[tokio::test]
async fn check_require_and_list() {
    let db = TestDb::new().await;
    let (ali, ayse, can, task, task2) = (id(), id(), id(), id(), id());
    setup(
        db.pool(),
        &[
            t(task, R::Owner, ali),
            t(task, R::Watcher, ayse),
            t(task2, R::Owner, ayse),
        ],
    )
    .await;
    let perms = engine().permissions();
    let pool = db.pool();

    assert!(perms.check(pool, ali, task, Perms::DELETE).await.unwrap());
    assert!(perms.check(pool, ayse, task, Perms::VIEW).await.unwrap());
    assert!(!perms.check(pool, ayse, task, Perms::EDIT).await.unwrap());
    assert!(!perms.check(pool, can, task, Perms::VIEW).await.unwrap());

    perms.require(pool, ali, task, Perms::SHARE).await.unwrap();
    // Visible but not allowed: 403. Not visible at all: 404, existence not revealed (V9).
    assert!(matches!(
        perms.require(pool, ayse, task, Perms::EDIT).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        perms.require(pool, can, task, Perms::VIEW).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        perms.require(pool, can, id(), Perms::VIEW).await,
        Err(AppError::NotFound)
    ));

    let mut both = vec![task, task2];
    both.sort();
    assert_eq!(
        perms
            .list_accessible(pool, ayse, Perms::VIEW)
            .await
            .unwrap(),
        both
    );
    assert_eq!(
        perms
            .list_accessible(pool, ayse, Perms::DELETE)
            .await
            .unwrap(),
        [task2]
    );
    assert!(
        perms
            .list_accessible(pool, can, Perms::VIEW)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn explain_lists_every_access_path() {
    // V11: Ayşe sees the page as a notebook member and through a link; Ali owns it.
    let db = TestDb::new().await;
    let (ali, ayse, notebook, page, task, team) = (id(), id(), id(), id(), id(), id());
    setup(
        db.pool(),
        &[
            t(notebook, R::Owner, ali),
            t(notebook, R::Editor, ayse),
            t(page, R::Owner, ali),
            t(page, R::InNotebook, notebook),
            t(team, R::TeamMember, ayse),
            t(task, R::OwnedBy, team),
            t(page, R::LinkedTo, task),
        ],
    )
    .await;
    let perms = engine().permissions();

    let ayse_view = perms.explain(db.pool(), ayse, page).await.unwrap().unwrap();
    assert_eq!(ayse_view.permissions, [Permission::View, Permission::Edit]);
    assert_eq!(
        ayse_view.reasons,
        [
            Reason::Notebook {
                notebook_id: notebook
            },
            Reason::Linked { item_id: task },
        ]
    );
    let team_view = perms.explain(db.pool(), ayse, task).await.unwrap().unwrap();
    assert!(matches!(team_view.reasons[..], [Reason::TeamRole { team_id, .. }] if team_id == team));
    let ali_view = perms.explain(db.pool(), ali, page).await.unwrap().unwrap();
    assert_eq!(ali_view.reasons[0], Reason::Owner);
    assert_eq!(ali_view.permissions.len(), 6);
    assert!(
        perms
            .explain(db.pool(), id(), page)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn preview_lists_new_viewers_without_writing() {
    // V7: "If you link this page, Ayşe and Mehmet will see it too."
    let db = TestDb::new().await;
    let (ali, ayse, mehmet, can, page, task, page2) = (id(), id(), id(), id(), id(), id(), id());
    setup(
        db.pool(),
        &[
            t(page, R::Owner, ali),
            t(page, R::Watcher, can),
            t(task, R::Owner, ali),
            t(task, R::Assignee, ayse),
            t(task, R::Watcher, mehmet),
            t(task, R::Watcher, can),
            t(page2, R::Owner, ali),
            t(page2, R::LinkedTo, page),
        ],
    )
    .await;
    let perms = engine().permissions();
    let mut conn = db.pool().acquire().await.unwrap();

    let link = t(page, R::LinkedTo, task);
    let preview = perms.preview(&mut conn, link).await.unwrap();
    let mut expected = vec![ayse, mehmet];
    expected.sort();
    // Can already sees the page; page2 (linked to page) gains the same people.
    let mut items: Vec<PreviewItem> = vec![
        PreviewItem {
            item_id: page,
            new_viewers: expected.clone(),
        },
        PreviewItem {
            item_id: page2,
            new_viewers: expected,
        },
    ];
    items.sort_by_key(|i| i.item_id);
    assert_eq!(preview, items);
    // Nothing was written.
    assert!(
        !perms
            .check(db.pool(), ayse, page, Perms::VIEW)
            .await
            .unwrap()
    );

    // "Adding Deniz to the task also shares its 1 linked page": only after the link exists.
    setup(db.pool(), &[link]).await;
    let deniz = id();
    let preview = perms
        .preview(&mut conn, t(task, R::Watcher, deniz))
        .await
        .unwrap();
    let gained: Vec<Uuid> = preview.iter().map(|i| i.item_id).collect();
    let mut expected = vec![task, page, page2];
    expected.sort();
    assert_eq!(gained, expected);
    assert!(preview.iter().all(|i| i.new_viewers == [deniz]));

    // An action that changes nothing previews nothing.
    assert!(perms.preview(&mut conn, link).await.unwrap().is_empty());
    assert!(
        perms
            .preview(&mut conn, t(task, R::Watcher, ayse))
            .await
            .unwrap()
            .is_empty()
    );
}

// --- endpoints ------------------------------------------------------------------------------

struct App {
    db: TestDb,
    router: Router,
}

impl App {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap());
        let identity = IdentityService::new(
            db.pool().clone(),
            Arc::new(clock),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        let router = identity::http::router(identity.clone()).merge(router(AccessHttpState {
            permissions: engine().permissions(),
            identity,
            pool: db.pool().clone(),
        }));
        Self { db, router }
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

    /// Registers a native client; returns its user id and bearer token.
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
        let user_id = body["user"]["id"].as_str().unwrap().parse().unwrap();
        (user_id, body["token"].as_str().unwrap().to_owned())
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn explanation_endpoint() {
    let app = App::new().await;
    let (ali, ali_token) = app.user("ali@example.test").await;
    let (_, can_token) = app.user("can@example.test").await;
    let task = id();
    setup(app.db.pool(), &[t(task, R::Owner, ali)]).await;

    let uri = format!("/access/items/{task}/explanation");
    let response = app.call(Method::GET, &uri, &ali_token, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "permissions": ["view", "edit", "share", "link", "manage", "delete"],
            "reasons": [{ "kind": "owner" }],
        })
    );
    // Someone without access learns nothing (V9).
    let response = app.call(Method::GET, &uri, &can_token, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    // Unauthenticated.
    let response = app.call(Method::GET, &uri, "nope", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn preview_endpoint_checks_the_actors_rights() {
    let app = App::new().await;
    let (ali, ali_token) = app.user("ali@example.test").await;
    let (ayse, ayse_token) = app.user("ayse@example.test").await;
    let (page, task, secret) = (id(), id(), id());
    setup(
        app.db.pool(),
        &[
            t(page, R::Owner, ali),
            t(task, R::Owner, ali),
            t(task, R::Assignee, ayse),
            t(secret, R::Owner, ayse),
        ],
    )
    .await;

    let link = json!({ "action": "link", "page_id": page, "item_id": task });
    let response = app
        .call(Method::POST, "/access/preview", &ali_token, Some(link))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(
        body["items"],
        json!([{ "item_id": page, "new_viewers": [ayse] }])
    );
    // V7: the dialog can name the person.
    assert_eq!(body["people"].as_array().unwrap().len(), 1);
    assert_eq!(body["people"][0]["id"], json!(ayse));
    assert!(body["people"][0]["display_name"].is_string());

    // V4: Ali cannot link to an item he cannot see (404, not revealed).
    let hidden = json!({ "action": "link", "page_id": page, "item_id": secret });
    let response = app
        .call(Method::POST, "/access/preview", &ali_token, Some(hidden))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // V5: Ayşe sees the task but may not add members (403).
    let add =
        json!({ "action": "add_member", "item_id": task, "user_id": id(), "role": "watcher" });
    let response = app
        .call(
            Method::POST,
            "/access/preview",
            &ayse_token,
            Some(add.clone()),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .call(Method::POST, "/access/preview", &ali_token, Some(add))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    // An id without an account has no name to show.
    assert!(body["people"][0]["display_name"].is_null());

    // A real stranger's name cannot be looked up by adding them to a preview (V1).
    let (stranger, _) = app.user("stranger@example.test").await;
    let add =
        json!({ "action": "add_member", "item_id": task, "user_id": stranger, "role": "watcher" });
    let response = app
        .call(Method::POST, "/access/preview", &ali_token, Some(add))
        .await;
    let body = json_body(response).await;
    assert_eq!(body["people"][0]["id"], json!(stranger));
    assert!(body["people"][0]["display_name"].is_null());
}
