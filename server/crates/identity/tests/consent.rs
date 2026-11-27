//! Legal consent (T4.3.3, PRD HS5, HS1) against a real Postgres 17. Requires Docker (or
//! `TEST_DATABASE_URL`). Provider sign-ins through consent are in `google.rs` and `apple.rs`.

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use chrono::{Duration, TimeZone, Utc};
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::legal::{TICKET_TTL, current_acceptances};
use identity::{EmailSettings, IdentityService};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use serde_json::{Value, json};
use tower::ServiceExt;

struct App {
    db: TestDb,
    clock: FixedClock,
    router: Router,
}

impl App {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 27, 12, 0, 0).unwrap());
        let service = IdentityService::new(
            db.pool().clone(),
            Arc::new(clock.clone()),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        Self {
            db,
            clock,
            router: identity::http::router(service),
        }
    }

    async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let response: Response = self
            .router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn register(&self, accepted: Value) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            "/auth/register",
            Some(json!({
                "email": "ada@example.com", "password": "correct horse", "display_name": "Ada",
                "client": "native", "accepted_legal": accepted
            })),
        )
        .await
    }

    async fn login(&self) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            "/auth/login",
            Some(json!({ "email": "ada@example.com", "password": "correct horse", "client": "native" })),
        )
        .await
    }

    async fn consent(&self, ticket: &str, accepted: Value) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            "/auth/consent",
            Some(json!({ "ticket": ticket, "accepted": accepted, "client": "native" })),
        )
        .await
    }

    async fn count(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }
}

fn all() -> Value {
    serde_json::to_value(current_acceptances()).unwrap()
}

#[tokio::test]
async fn hs5_no_account_without_accepting_every_text() {
    let app = App::new().await;
    for accepted in [
        json!([]),
        json!([{ "kind": "privacy", "version": 1 }, { "kind": "kvkk", "version": 1 }]),
    ] {
        let (status, body) = app.register(accepted).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["fields"][0]["field"], "accepted_legal");
    }
    // An old client without the field at all.
    let (status, _) = app
        .call(
            Method::POST,
            "/auth/register",
            Some(json!({
                "email": "ada@example.com", "password": "correct horse", "display_name": "Ada",
                "client": "native"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.count("SELECT count(*) FROM users").await, 0);

    let (status, _) = app.register(all()).await;
    assert_eq!(status, StatusCode::CREATED);
    // What was accepted, which version, when.
    let rows: Vec<(String, i32, chrono::DateTime<Utc>)> = sqlx::query_as(
        "SELECT document, version, accepted_at FROM legal_consents ORDER BY document",
    )
    .fetch_all(app.db.pool())
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            ("kvkk".to_owned(), 1, app.clock_now()),
            ("privacy".to_owned(), 1, app.clock_now()),
            ("terms".to_owned(), 1, app.clock_now()),
        ]
    );
    let (status, body) = app.login().await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["token"].as_str().is_some());
}

impl App {
    fn clock_now(&self) -> chrono::DateTime<Utc> {
        use kernel::Clock;
        self.clock.now()
    }
}

#[tokio::test]
async fn hs5_a_new_version_holds_the_next_sign_in_until_it_is_accepted() {
    let app = App::new().await;
    assert_eq!(app.register(all()).await.0, StatusCode::CREATED);
    // The terms got a version this user has not accepted.
    sqlx::query("DELETE FROM legal_consents WHERE document = 'terms'")
        .execute(app.db.pool())
        .await
        .unwrap();
    let sessions_before = app.count("SELECT count(*) FROM sessions").await;

    let (status, body) = app.login().await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "consent_required");
    assert!(body.get("token").is_none() && body["user"].is_null());
    let ticket = body["error"]["consent_ticket"].as_str().unwrap().to_owned();
    assert_eq!(
        app.count("SELECT count(*) FROM sessions").await,
        sessions_before,
        "no session before consent"
    );

    // Not everything accepted: refused, and the ticket still works.
    let (status, _) = app
        .consent(&ticket, json!([{ "kind": "privacy", "version": 1 }]))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) = app.consent(&ticket, all()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["token"].as_str().is_some());
    assert_eq!(body["user"]["email"], "ada@example.com");
    assert_eq!(
        app.count("SELECT count(*) FROM legal_consents WHERE document = 'terms'")
            .await,
        1
    );
    // Single use; and the next sign-in goes straight through.
    assert_eq!(
        app.consent(&ticket, all()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(app.login().await.0, StatusCode::OK);
}

#[tokio::test]
async fn hs5_a_ticket_expires_and_a_wrong_password_gets_none() {
    let app = App::new().await;
    assert_eq!(app.register(all()).await.0, StatusCode::CREATED);
    sqlx::query("DELETE FROM legal_consents")
        .execute(app.db.pool())
        .await
        .unwrap();
    let (status, body) = app
        .call(
            Method::POST,
            "/auth/login",
            Some(json!({ "email": "ada@example.com", "password": "wrong horse!", "client": "native" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body["error"].get("consent_ticket").is_none());
    assert_eq!(app.count("SELECT count(*) FROM consent_tickets").await, 0);

    let (_, body) = app.login().await;
    let ticket = body["error"]["consent_ticket"].as_str().unwrap().to_owned();
    app.clock.advance(TICKET_TTL + Duration::seconds(1));
    assert_eq!(
        app.consent(&ticket, all()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.consent("made-up", all()).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn hs5_the_texts_are_served_with_their_versions() {
    let app = App::new().await;
    let (status, list) = app.call(Method::GET, "/legal", None).await;
    assert_eq!(status, StatusCode::OK);
    let kinds: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["privacy", "kvkk", "terms"]);
    let (status, doc) = app.call(Method::GET, "/legal/kvkk", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(doc["version"], 1);
    assert!(doc["body"].as_str().unwrap().starts_with("# KVKK"));
    assert_eq!(
        app.call(Method::GET, "/legal/cookies", None).await.0,
        StatusCode::NOT_FOUND
    );
}
