//! `GET /me` and `PATCH /me` (T0.3.7, ARCHITECTURE §5.1) against a real Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::{EmailSettings, IdentityService};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use serde_json::{Value, json};
use tower::ServiceExt;

struct App {
    _db: TestDb,
    router: Router,
}

impl App {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 21, 12, 0, 0).unwrap());
        let service = IdentityService::new(
            db.pool().clone(),
            Arc::new(clock),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        let router =
            identity::http::router(service.clone()).merge(identity::http::profile_router(service));
        Self { _db: db, router }
    }

    async fn send(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    async fn call(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Response {
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
        self.send(request.body(body).unwrap()).await
    }

    /// Registers a native client and returns its bearer token.
    async fn token(&self, email: &str) -> String {
        let response = self
            .call(
                Method::POST,
                "/auth/register",
                None,
                Some(json!({
                    "email": email,
                    "password": "correct horse",
                    "display_name": "Ada",
                    "client": "native",
                })),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        json_body(response).await["token"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn me(&self, token: &str) -> Value {
        let response = self.call(Method::GET, "/me", Some(token), None).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    }

    async fn patch(&self, token: &str, body: Value) -> Response {
        self.call(Method::PATCH, "/me", Some(token), Some(body))
            .await
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn new_account_has_default_profile() {
    let app = App::new().await;
    let token = app.token("ada@example.test").await;
    let me = app.me(&token).await;
    assert_eq!(me["email"], "ada@example.test");
    assert_eq!(me["display_name"], "Ada");
    assert_eq!(me["avatar_url"], Value::Null);
    assert_eq!(me["timezone"], "UTC");
    assert_eq!(me["locale"], "tr");
}

#[tokio::test]
async fn patch_updates_all_four_fields_and_persists() {
    let app = App::new().await;
    let token = app.token("ada@example.test").await;
    let response = app
        .patch(
            &token,
            json!({
                "display_name": "  Ada Lovelace ",
                "avatar_url": "https://cdn.example.test/ada.png",
                "timezone": "Europe/Istanbul",
                "locale": "en-us",
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["display_name"], "Ada Lovelace");
    assert_eq!(body["avatar_url"], "https://cdn.example.test/ada.png");
    assert_eq!(body["timezone"], "Europe/Istanbul");
    assert_eq!(body["locale"], "en-US");
    assert_eq!(app.me(&token).await, body);
}

#[tokio::test]
async fn absent_fields_are_unchanged_and_null_clears_the_avatar() {
    let app = App::new().await;
    let token = app.token("ada@example.test").await;
    let set = app
        .patch(
            &token,
            json!({ "avatar_url": "https://x.test/a.png", "timezone": "Asia/Tokyo" }),
        )
        .await;
    assert_eq!(set.status(), StatusCode::OK);

    // Only the name changes; the avatar and time zone stay.
    let renamed = json_body(app.patch(&token, json!({ "display_name": "Grace" })).await).await;
    assert_eq!(renamed["display_name"], "Grace");
    assert_eq!(renamed["avatar_url"], "https://x.test/a.png");
    assert_eq!(renamed["timezone"], "Asia/Tokyo");

    let cleared = json_body(app.patch(&token, json!({ "avatar_url": null })).await).await;
    assert_eq!(cleared["avatar_url"], Value::Null);
    assert_eq!(cleared["display_name"], "Grace");

    // An empty body is a no-op read.
    let same = json_body(app.patch(&token, json!({})).await).await;
    assert_eq!(same, cleared);
}

#[tokio::test]
async fn invalid_fields_are_rejected_with_field_errors_and_nothing_changes() {
    let app = App::new().await;
    let token = app.token("ada@example.test").await;
    let before = app.me(&token).await;
    let response = app
        .patch(
            &token,
            json!({
                "display_name": "Valid Name",
                "avatar_url": "javascript:alert(1)",
                "timezone": "Mars/Olympus",
                "locale": "klingon",
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    let fields: Vec<&str> = body["error"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["avatar_url", "timezone", "locale"]);
    // Validation is all-or-nothing: the valid name was not applied either.
    assert_eq!(app.me(&token).await, before);
}

#[tokio::test]
async fn profile_requires_a_session() {
    let app = App::new().await;
    let get = app.call(Method::GET, "/me", None, None).await;
    assert_eq!(get.status(), StatusCode::UNAUTHORIZED);
    let patch = app
        .call(
            Method::PATCH,
            "/me",
            Some("not-a-token"),
            Some(json!({ "display_name": "X" })),
        )
        .await;
    assert_eq!(patch.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_user_only_changes_their_own_profile() {
    let app = App::new().await;
    let ada = app.token("ada@example.test").await;
    let grace = app.token("grace@example.test").await;
    let response = app.patch(&ada, json!({ "display_name": "Ada L." })).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(app.me(&grace).await["display_name"], "Ada");
    assert_eq!(app.me(&grace).await["email"], "grace@example.test");
}

#[tokio::test]
async fn cookie_patch_needs_the_csrf_token() {
    let app = App::new().await;
    let response = app
        .call(
            Method::POST,
            "/auth/register",
            None,
            Some(json!({
                "email": "ada@example.test",
                "password": "correct horse",
                "display_name": "Ada",
                "client": "web",
            })),
        )
        .await;
    let csrf = csrf_of(&response);
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let patch = |csrf: Option<String>| {
        let mut request = Request::builder()
            .method(Method::PATCH)
            .uri("/me")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(csrf) = csrf {
            request = request.header("x-csrf-token", csrf);
        }
        request
            .body(Body::from(json!({ "display_name": "Ada L." }).to_string()))
            .unwrap()
    };
    assert_eq!(app.send(patch(None)).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.send(patch(Some(csrf))).await.status(), StatusCode::OK);
}

/// The CSRF token for the session cookie of a web sign-in response (derived the same way the
/// server derives it, so the body need not be read).
fn csrf_of(response: &Response) -> String {
    let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    let token = set_cookie
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("sb_session=");
    identity::token::csrf_for(token)
}
