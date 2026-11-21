//! `/auth/*` endpoints against a real Postgres 17 (T0.3.2, ARCHITECTURE §15, ADR-0016).
//! Requires Docker (or `TEST_DATABASE_URL`).

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
use identity::IdentityService;
use identity::service::{SESSION_TTL, TOUCH_INTERVAL};
use kernel::FixedClock;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

struct App {
    db: TestDb,
    clock: FixedClock,
    router: Router,
}

impl App {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 21, 12, 0, 0).unwrap());
        let service = IdentityService::new(db.pool().clone(), Arc::new(clock.clone()));
        let router = identity::http::router(service);
        Self { db, clock, router }
    }

    async fn call(
        &self,
        method: Method,
        uri: &str,
        auth: Auth<'_>,
        body: Option<Value>,
    ) -> Response {
        let mut request = Request::builder().method(method).uri(uri);
        match auth {
            Auth::None => {}
            Auth::Bearer(token) => {
                request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            Auth::Cookie(cookie) => request = request.header(header::COOKIE, cookie),
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

    async fn register(&self, email: &str, client: &str) -> Response {
        self.call(
            Method::POST,
            "/auth/register",
            Auth::None,
            Some(json!({
                "email": email,
                "password": "correct horse",
                "display_name": "Ada",
                "client": client,
                "device": "Test device",
            })),
        )
        .await
    }

    async fn login(&self, email: &str, password: &str, client: &str) -> Response {
        self.call(
            Method::POST,
            "/auth/login",
            Auth::None,
            Some(json!({ "email": email, "password": password, "client": client })),
        )
        .await
    }

    /// Registers a native client and returns its bearer token.
    async fn native_token(&self, email: &str) -> String {
        let response = self.register(email, "native").await;
        assert_eq!(response.status(), StatusCode::CREATED);
        json_body(response).await["token"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn sessions(&self, token: &str) -> Response {
        self.call(Method::GET, "/auth/sessions", Auth::Bearer(token), None)
            .await
    }
}

#[derive(Clone, Copy)]
enum Auth<'a> {
    None,
    Bearer(&'a str),
    Cookie(&'a str),
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn set_cookie(response: &Response) -> String {
    response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_owned()
}

/// `sb_session=<token>` from a `Set-Cookie` header.
fn cookie_pair(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap().to_owned()
}

#[tokio::test]
async fn native_register_returns_a_bearer_token_that_authenticates() {
    let app = App::new().await;
    let response = app.register("ada@example.test", "native").await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = json_body(response).await;
    assert_eq!(body["user"]["email"], "ada@example.test");
    assert_eq!(body["user"]["display_name"], "Ada");
    assert_eq!(body["user"]["timezone"], "UTC");
    let token = body["token"].as_str().unwrap();
    assert_eq!(token.len(), 43);

    let sessions = app.sessions(token).await;
    assert_eq!(sessions.status(), StatusCode::OK);
    let list = json_body(sessions).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["current"], true);
    assert_eq!(list[0]["device"], "Test device");
}

#[tokio::test]
async fn web_register_sets_a_secure_cookie_and_hides_the_token() {
    let app = App::new().await;
    let response = app.register("web@example.test", "web").await;

    assert_eq!(response.status(), StatusCode::CREATED);
    let cookie = set_cookie(&response);
    for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(cookie.contains(attr), "{cookie}");
    }
    let body = json_body(response).await;
    assert!(
        body.get("token").is_none(),
        "token must not be in a web response body"
    );

    let pair = cookie_pair(&cookie);
    let sessions = app
        .call(Method::GET, "/auth/sessions", Auth::Cookie(&pair), None)
        .await;
    assert_eq!(sessions.status(), StatusCode::OK);
}

#[tokio::test]
async fn password_is_stored_as_argon2id_and_token_only_as_sha256() {
    let app = App::new().await;
    let token = app.native_token("hash@example.test").await;

    let password_hash: String =
        sqlx::query_scalar("SELECT password_hash FROM auth_identities WHERE provider = 'password'")
            .fetch_one(app.db.pool())
            .await
            .unwrap();
    assert!(password_hash.starts_with("$argon2id$"));
    assert!(!password_hash.contains("correct horse"));

    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_one(app.db.pool())
        .await
        .unwrap();
    assert_eq!(stored, Sha256::digest(token.as_bytes()).to_vec());
}

#[tokio::test]
async fn duplicate_email_is_a_conflict_regardless_of_case() {
    let app = App::new().await;
    assert_eq!(
        app.register("dup@example.test", "web").await.status(),
        StatusCode::CREATED
    );

    let response = app.register("DUP@Example.TEST", "web").await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["error"]["code"], "conflict");
}

#[tokio::test]
async fn invalid_registration_reports_field_errors() {
    let app = App::new().await;
    let response = app
        .call(
            Method::POST,
            "/auth/register",
            Auth::None,
            Some(json!({ "email": "nope", "password": "short", "display_name": " ", "client": "web" })),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let fields: Vec<String> = json_body(response).await["error"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["field"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(fields, ["email", "password", "display_name"]);
}

#[tokio::test]
async fn login_works_with_any_email_case_and_creates_a_new_session() {
    let app = App::new().await;
    let first = app.native_token("login@example.test").await;

    let response = app
        .login("LOGIN@example.test", "correct horse", "native")
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let second = json_body(response).await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(first, second);

    let list = json_body(app.sessions(&second).await).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn wrong_password_and_unknown_email_look_the_same() {
    let app = App::new().await;
    app.native_token("known@example.test").await;

    let wrong = app
        .login("known@example.test", "wrong password", "web")
        .await;
    let unknown = app
        .login("unknown@example.test", "correct horse", "web")
        .await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
    assert!(wrong.headers().get(header::SET_COOKIE).is_none());
    assert_eq!(json_body(wrong).await, json_body(unknown).await);
}

#[tokio::test]
async fn protected_endpoints_reject_missing_malformed_and_unknown_tokens() {
    let app = App::new().await;
    let unknown = "A".repeat(43);
    for auth in [
        Auth::None,
        Auth::Bearer("garbage"),
        Auth::Bearer(&unknown),
        Auth::Cookie("sb_session=garbage"),
    ] {
        let response = app.call(Method::GET, "/auth/sessions", auth, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            json_body(response).await["error"]["code"],
            "unauthenticated"
        );
    }
}

#[tokio::test]
async fn logout_ends_only_the_current_session_and_clears_the_cookie() {
    let app = App::new().await;
    let a = app.native_token("out@example.test").await;
    let response = app
        .login("out@example.test", "correct horse", "native")
        .await;
    let b = json_body(response).await["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = app
        .call(Method::POST, "/auth/logout", Auth::Bearer(&a), None)
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(set_cookie(&response).contains("Max-Age=0"));

    assert_eq!(app.sessions(&a).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(app.sessions(&b).await.status(), StatusCode::OK);

    // Signing out again (or without a session) still succeeds.
    let again = app
        .call(Method::POST, "/auth/logout", Auth::Bearer(&a), None)
        .await;
    assert_eq!(again.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_user_can_revoke_own_sessions_but_not_others() {
    let app = App::new().await;
    let mine = app.native_token("me@example.test").await;
    let response = app
        .login("me@example.test", "correct horse", "native")
        .await;
    let my_other = json_body(response).await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let theirs = app.native_token("them@example.test").await;

    let their_session = json_body(app.sessions(&theirs).await).await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let my_sessions = json_body(app.sessions(&mine).await).await;
    let other_id = my_sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["current"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Someone else's session: 404 (V9), and it stays valid.
    let response = app
        .call(
            Method::DELETE,
            &format!("/auth/sessions/{their_session}"),
            Auth::Bearer(&mine),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(app.sessions(&theirs).await.status(), StatusCode::OK);

    let response = app
        .call(
            Method::DELETE,
            "/auth/sessions/not-a-uuid",
            Auth::Bearer(&mine),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // My other device: revoked.
    let response = app
        .call(
            Method::DELETE,
            &format!("/auth/sessions/{other_id}"),
            Auth::Bearer(&mine),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        app.sessions(&my_other).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(app.sessions(&mine).await.status(), StatusCode::OK);

    // Without a session: 401.
    let response = app
        .call(
            Method::DELETE,
            &format!("/auth/sessions/{other_id}"),
            Auth::None,
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sessions_expire_after_inactivity_and_use_extends_them() {
    let app = App::new().await;
    let token = app.native_token("slide@example.test").await;

    // Used just before expiry: still valid, and the expiry moves forward.
    app.clock.advance(SESSION_TTL - Duration::seconds(1));
    let list = json_body(app.sessions(&token).await).await;
    let expires_at: chrono::DateTime<Utc> =
        serde_json::from_value(list[0]["expires_at"].clone()).unwrap();
    assert_eq!(expires_at, app.clock_now() + SESSION_TTL);

    // A second request within the touch interval does not write again.
    app.clock.advance(TOUCH_INTERVAL - Duration::seconds(1));
    let list = json_body(app.sessions(&token).await).await;
    let unchanged: chrono::DateTime<Utc> =
        serde_json::from_value(list[0]["expires_at"].clone()).unwrap();
    assert_eq!(unchanged, expires_at);

    // Unused for a full TTL: expired.
    app.clock.set(expires_at);
    assert_eq!(
        app.sessions(&token).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

impl App {
    fn clock_now(&self) -> chrono::DateTime<Utc> {
        use kernel::Clock;
        self.clock.now()
    }
}
