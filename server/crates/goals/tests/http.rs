//! Goal endpoints (T2.3.1) over HTTP, against Postgres 17: status codes, bodies and errors that
//! reveal nothing (V9). Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use goals::GoalsService;
use goals::http::{GoalsHttpState, router};
use http_body_util::BodyExt;
use identity::{EmailSettings, IdentityService};
use items::{ItemsAccessContext, ItemsService, NewItem};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

struct World {
    _db: TestDb,
    items: ItemsService,
    router: Router,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock: kernel::SharedClock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap(),
        ));
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(db.pool().clone(), clock.clone(), access.clone());
        let identity = IdentityService::new(
            db.pool().clone(),
            clock.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        let goals = GoalsService::new(
            db.pool().clone(),
            clock,
            access.permissions(),
            items.clone(),
        );
        let router = identity::http::router(identity.clone())
            .merge(router(GoalsHttpState { goals, identity }));
        Self {
            _db: db,
            items,
            router,
        }
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    async fn user(&self, email: &str) -> (Uuid, String) {
        let (status, body) = self
            .send(
                Method::POST,
                "/auth/register",
                None,
                Some(json!({
                    "email": email,
                    "password": "correct horse",
                    "display_name": "Test",
                    "client": "native",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        (
            body["user"]["id"].as_str().unwrap().parse().unwrap(),
            body["token"].as_str().unwrap().to_owned(),
        )
    }
}

#[tokio::test]
async fn goals_are_created_read_moved_and_linked_over_http() {
    let w = World::new().await;
    let (ali, token) = w.user("ali@example.com").await;
    let (status, goal) = w
        .send(
            Method::POST,
            "/goals",
            Some(&token),
            Some(json!({"title": "Kitap", "start_date": "2026-12-01", "end_date": "2026-12-31"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(goal["start_date"], "2026-12-01");
    assert_eq!(goal["links"], json!([]));
    let id = goal["item"]["id"].as_str().unwrap().to_owned();

    let (status, read) = w
        .send(Method::GET, &format!("/goals/{id}"), Some(&token), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read, goal);

    let (status, moved) = w
        .send(
            Method::PATCH,
            &format!("/goals/{id}"),
            Some(&token),
            Some(json!({"end_date": "2026-12-15"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["end_date"], "2026-12-15");

    let task = w
        .items
        .create(
            ali,
            NewItem {
                kind: "task".into(),
                title: "Oku".into(),
                description: None,
            },
        )
        .await
        .unwrap()
        .id;
    let link = format!("/goals/{id}/links/{task}");
    let (status, linked) = w
        .send(
            Method::PUT,
            &link,
            Some(&token),
            Some(json!({"weight": "2.5"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(linked["links"][0]["weight"], "2.5");
    assert_eq!(linked["links"][0]["contributes"], true);
    let (status, unlinked) = w.send(Method::DELETE, &link, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unlinked["links"], json!([]));
}

#[tokio::test]
async fn d1_a_missing_date_is_a_field_error() {
    let w = World::new().await;
    let (_, token) = w.user("ali@example.com").await;
    let (status, body) = w
        .send(
            Method::POST,
            "/goals",
            Some(&token),
            Some(json!({"title": "Kitap", "start_date": "2026-12-01"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["fields"][0]["field"], "end_date");
}

#[tokio::test]
async fn v9_errors_reveal_nothing_to_others() {
    let w = World::new().await;
    let (_, ali) = w.user("ali@example.com").await;
    let (_, ayse) = w.user("ayse@example.com").await;
    let (_, goal) = w
        .send(
            Method::POST,
            "/goals",
            Some(&ali),
            Some(json!({"title": "Gizli hedef", "start_date": "2026-12-01", "end_date": "2026-12-31"})),
        )
        .await;
    let id = goal["item"]["id"].as_str().unwrap().to_owned();
    let missing = Uuid::now_v7();
    for (method, uri, request) in [
        (Method::GET, format!("/goals/{id}"), None),
        (
            Method::PATCH,
            format!("/goals/{id}"),
            Some(json!({"end_date": "2026-12-20"})),
        ),
        (
            Method::PUT,
            format!("/goals/{id}/links/{missing}"),
            Some(json!({})),
        ),
        (Method::DELETE, format!("/goals/{id}/links/{missing}"), None),
    ] {
        let (status, body) = w
            .send(method.clone(), &uri, Some(&ayse), request.clone())
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert!(!body.to_string().contains("Gizli"));
        // The same answer as for a goal that does not exist at all.
        let unknown = uri.replace(&id, &Uuid::now_v7().to_string());
        let (other, _) = w.send(method, &unknown, Some(&ayse), request).await;
        assert_eq!(other, StatusCode::NOT_FOUND);
    }
    let (status, _) = w
        .send(Method::GET, &format!("/goals/{id}"), None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
