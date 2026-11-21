//! Idempotency keys (T0.2.8) against a real Postgres 17. Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use app::idempotency::{self, Idempotency, KEY_HEADER, LEASE, REPLAYED_HEADER, RETENTION};
use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{Duration, TimeZone, Utc};
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::IdentityService;
use identity::http::Authenticated;
use identity::model::NewAccount;
use kernel::{Clock, FixedClock};
use serde_json::{Value, json};
use tokio::sync::Notify;
use tower::ServiceExt;

#[derive(Clone)]
struct TestState {
    identity: IdentityService,
    calls: Arc<AtomicUsize>,
    release: Arc<Notify>,
}

impl axum::extract::FromRef<TestState> for IdentityService {
    fn from_ref(state: &TestState) -> Self {
        state.identity.clone()
    }
}

async fn create(
    State(state): State<TestState>,
    Authenticated(_): Authenticated,
    body: String,
) -> Response {
    let n = state.calls.fetch_add(1, Ordering::SeqCst) + 1;
    (StatusCode::CREATED, Json(json!({ "n": n, "echo": body }))).into_response()
}

async fn fail(State(state): State<TestState>, Authenticated(_): Authenticated) -> StatusCode {
    state.calls.fetch_add(1, Ordering::SeqCst);
    StatusCode::INTERNAL_SERVER_ERROR
}

async fn slow(State(state): State<TestState>, Authenticated(_): Authenticated) -> StatusCode {
    state.calls.fetch_add(1, Ordering::SeqCst);
    state.release.notified().await;
    StatusCode::NO_CONTENT
}

async fn reject(State(state): State<TestState>, Authenticated(_): Authenticated) -> StatusCode {
    state.calls.fetch_add(1, Ordering::SeqCst);
    StatusCode::UNPROCESSABLE_ENTITY
}

struct App {
    db: TestDb,
    clock: FixedClock,
    identity: IdentityService,
    state: TestState,
    router: Router,
}

impl App {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 21, 12, 0, 0).unwrap());
        let shared: kernel::SharedClock = Arc::new(clock.clone());
        let identity = IdentityService::new(db.pool().clone(), shared.clone(), email_settings());
        let state = TestState {
            identity: identity.clone(),
            calls: Arc::new(AtomicUsize::new(0)),
            release: Arc::new(Notify::new()),
        };
        let routes = Router::new()
            .route("/things", post(create).get(|| async { "list" }))
            .route("/fail", post(fail))
            .route("/slow", post(slow))
            .route("/reject", post(reject))
            .with_state(state.clone());
        let router = idempotency::protect(
            routes,
            Idempotency::new(db.pool().clone(), identity.clone(), shared),
        );
        Self {
            db,
            clock,
            identity,
            state,
            router,
        }
    }

    async fn token(&self, email: &str) -> String {
        let (_, session) = self
            .identity
            .register(NewAccount {
                email: email.into(),
                password: "correct horse".into(),
                display_name: "T".into(),
                device: None,
            })
            .await
            .unwrap();
        session.token.expose().to_owned()
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        key: Option<&str>,
        body: &str,
    ) -> Response {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(key) = key {
            request = request.header(KEY_HEADER, key);
        }
        self.router
            .clone()
            .oneshot(request.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap()
    }

    fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }

    async fn rows(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM idempotency_keys")
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn repeated_key_replays_the_first_response_without_executing_again() {
    let app = App::new().await;
    let token = app.token("a@x.test").await;

    let first = app
        .send(Method::POST, "/things", Some(&token), Some("k1"), "hello")
        .await;
    assert_eq!(first.status(), StatusCode::CREATED);
    assert!(first.headers().get(REPLAYED_HEADER).is_none());
    let first_body = json_body(first).await;

    let second = app
        .send(Method::POST, "/things", Some(&token), Some("k1"), "hello")
        .await;
    assert_eq!(second.status(), StatusCode::CREATED);
    assert_eq!(second.headers()[REPLAYED_HEADER], "true");
    assert_eq!(second.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(json_body(second).await, first_body);
    assert_eq!(app.calls(), 1);

    // A new key is a new mutation.
    let third = app
        .send(Method::POST, "/things", Some(&token), Some("k2"), "hello")
        .await;
    assert_eq!(json_body(third).await["n"], 2);
}

#[tokio::test]
async fn mutations_without_a_key_are_rejected_and_reads_pass() {
    let app = App::new().await;
    let token = app.token("b@x.test").await;

    let response = app
        .send(Method::POST, "/things", Some(&token), None, "x")
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["error"]["fields"][0]["field"], "Idempotency-Key");
    assert_eq!(body["error"]["fields"][0]["code"], "required");

    let response = app
        .send(Method::POST, "/things", Some(&token), Some("bad key"), "x")
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let read = app
        .send(Method::GET, "/things", Some(&token), None, "")
        .await;
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(app.calls(), 0);
}

#[tokio::test]
async fn same_key_with_a_different_request_is_rejected() {
    let app = App::new().await;
    let token = app.token("c@x.test").await;
    app.send(Method::POST, "/things", Some(&token), Some("k"), "one")
        .await;

    let response = app
        .send(Method::POST, "/things", Some(&token), Some("k"), "two")
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json_body(response).await["error"]["fields"][0]["code"],
        "invalid"
    );
    let other_path = app
        .send(Method::POST, "/reject", Some(&token), Some("k"), "one")
        .await;
    assert_eq!(other_path.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.calls(), 1);
}

#[tokio::test]
async fn concurrent_requests_with_one_key_execute_once() {
    let app = Arc::new(App::new().await);
    let token = app.token("d@x.test").await;

    let first = {
        let app = Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move {
            app.send(Method::POST, "/slow", Some(&token), Some("k"), "")
                .await
                .status()
        })
    };
    // Wait until the first request is inside the handler.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while app.calls() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first request never reached the handler");
    let second = app
        .send(Method::POST, "/slow", Some(&token), Some("k"), "")
        .await;
    assert_eq!(second.status(), StatusCode::CONFLICT);

    app.state.release.notify_one();
    assert_eq!(first.await.unwrap(), StatusCode::NO_CONTENT);
    let replay = app
        .send(Method::POST, "/slow", Some(&token), Some("k"), "")
        .await;
    assert_eq!(replay.status(), StatusCode::NO_CONTENT);
    assert_eq!(replay.headers()[REPLAYED_HEADER], "true");
    assert_eq!(app.calls(), 1);
}

#[tokio::test]
async fn server_errors_are_not_stored_so_the_client_can_retry() {
    let app = App::new().await;
    let token = app.token("e@x.test").await;
    for _ in 0..2 {
        let response = app
            .send(Method::POST, "/fail", Some(&token), Some("k"), "")
            .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
    assert_eq!(app.calls(), 2);
    assert_eq!(app.rows().await, 0);
}

#[tokio::test]
async fn client_errors_are_stored_and_replayed() {
    let app = App::new().await;
    let token = app.token("f@x.test").await;
    for _ in 0..2 {
        let response = app
            .send(Method::POST, "/reject", Some(&token), Some("k"), "")
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert_eq!(app.calls(), 1);
}

#[tokio::test]
async fn keys_are_scoped_per_user() {
    let app = App::new().await;
    let alice = app.token("alice@x.test").await;
    let bob = app.token("bob@x.test").await;

    app.send(Method::POST, "/things", Some(&alice), Some("same"), "x")
        .await;
    let response = app
        .send(Method::POST, "/things", Some(&bob), Some("same"), "x")
        .await;
    assert!(response.headers().get(REPLAYED_HEADER).is_none());
    assert_eq!(app.calls(), 2);
}

#[tokio::test]
async fn unauthenticated_requests_pass_through_unrecorded() {
    let app = App::new().await;
    let response = app
        .send(Method::POST, "/things", None, Some("k"), "x")
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .send(
            Method::POST,
            "/things",
            Some(&"A".repeat(43)),
            Some("k"),
            "x",
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(app.rows().await, 0);
}

#[tokio::test]
async fn an_abandoned_attempt_can_be_taken_over_after_the_lease() {
    let app = App::new().await;
    let token = app.token("g@x.test").await;

    // Simulate a request that claimed the key and then died.
    let user: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(app.db.pool())
        .await
        .unwrap();
    let hash = idempotency::request_hash(&Method::POST, "/things", b"x");
    sqlx::query(
        "INSERT INTO idempotency_keys (user_id, key, request_hash, created_at) VALUES ($1, 'k', $2, $3)",
    )
    .bind(user)
    .bind(hash.as_slice())
    .bind(app.clock.now())
    .execute(app.db.pool())
    .await
    .unwrap();

    let blocked = app
        .send(Method::POST, "/things", Some(&token), Some("k"), "x")
        .await;
    assert_eq!(blocked.status(), StatusCode::CONFLICT);

    app.clock.advance(LEASE);
    let taken = app
        .send(Method::POST, "/things", Some(&token), Some("k"), "x")
        .await;
    assert_eq!(taken.status(), StatusCode::CREATED);
    assert_eq!(app.calls(), 1);
}

#[tokio::test]
async fn cleanup_deletes_only_expired_keys() {
    let app = App::new().await;
    let token = app.token("h@x.test").await;
    app.send(Method::POST, "/things", Some(&token), Some("old"), "x")
        .await;
    app.clock.advance(RETENTION);
    app.send(Method::POST, "/things", Some(&token), Some("new"), "x")
        .await;

    let deleted = idempotency::delete_expired(
        app.db.pool(),
        app.clock.now() - RETENTION + Duration::seconds(1),
    )
    .await
    .unwrap();
    assert_eq!(deleted, 1);
    let left: Vec<String> = sqlx::query_scalar("SELECT key FROM idempotency_keys")
        .fetch_all(app.db.pool())
        .await
        .unwrap();
    assert_eq!(left, ["new"]);
}

fn email_settings() -> identity::EmailSettings {
    identity::EmailSettings {
        sender: Arc::new(kernel::email::RecordingEmailSender::default()),
        web_url: "https://app.test".into(),
    }
}
