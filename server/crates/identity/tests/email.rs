//! E-mail verification and password reset (T0.3.3) against a real Postgres 17.
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
use identity::email_flow::{RESET_TTL, VERIFY_TTL};
use identity::{EmailSettings, IdentityService};
use kernel::email::RecordingEmailSender;
use kernel::{EmailKind, FixedClock};
use serde_json::{Value, json};
use tower::ServiceExt;

const PASSWORD: &str = "correct horse";

struct App {
    db: TestDb,
    clock: FixedClock,
    mail: RecordingEmailSender,
    router: Router,
}

impl App {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 21, 12, 0, 0).unwrap());
        let mail = RecordingEmailSender::default();
        let service = IdentityService::new(
            db.pool().clone(),
            Arc::new(clock.clone()),
            EmailSettings {
                sender: Arc::new(mail.clone()),
                web_url: "https://app.test/".into(),
            },
        );
        Self {
            db,
            clock,
            mail,
            router: identity::http::router(service),
        }
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
        self.router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap()
    }

    /// Registers a native client; returns (bearer token, response body).
    async fn register(&self, email: &str) -> (String, Value) {
        let response = self
            .call(
                Method::POST,
                "/auth/register",
                None,
                Some(json!({ "email": email, "password": PASSWORD, "display_name": "Ada", "client": "native", "accepted_legal": identity::legal::current_acceptances() })),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = json_body(response).await;
        (body["token"].as_str().unwrap().to_owned(), body)
    }

    async fn login(&self, email: &str, password: &str) -> StatusCode {
        self.call(
            Method::POST,
            "/auth/login",
            None,
            Some(json!({ "email": email, "password": password, "client": "native" })),
        )
        .await
        .status()
    }

    async fn verify(&self, token: &str) -> Response {
        self.call(
            Method::POST,
            "/auth/email/verify",
            None,
            Some(json!({ "token": token })),
        )
        .await
    }

    async fn forgot(&self, email: &str) -> StatusCode {
        self.call(
            Method::POST,
            "/auth/password/forgot",
            None,
            Some(json!({ "email": email })),
        )
        .await
        .status()
    }

    async fn reset(&self, token: &str, password: &str) -> Response {
        self.call(
            Method::POST,
            "/auth/password/reset",
            None,
            Some(json!({ "token": token, "password": password })),
        )
        .await
    }

    async fn verified(&self, email: &str) -> bool {
        sqlx::query_scalar(
            "SELECT email_verified_at IS NOT NULL FROM users WHERE lower(email) = lower($1)",
        )
        .bind(email)
        .fetch_one(self.db.pool())
        .await
        .unwrap()
    }

    /// Token from the newest e-mail of `kind` (waits for background delivery).
    async fn last_token(&self, kind: EmailKind) -> String {
        for _ in 0..200 {
            if let Some(message) = self.mail.sent().into_iter().rev().find(|m| m.kind == kind) {
                return message
                    .text
                    .split("token=")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_owned();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("no {kind:?} e-mail was sent");
    }

    fn count(&self, kind: EmailKind) -> usize {
        self.mail.sent().iter().filter(|m| m.kind == kind).count()
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn assert_invalid_token(response: Response) {
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["error"]["fields"][0]["field"], "token");
    assert_eq!(body["error"]["fields"][0]["code"], "invalid");
}

#[tokio::test]
async fn registration_sends_a_verification_link_that_verifies_once() {
    let app = App::new().await;
    let (_, body) = app.register("ada@example.test").await;
    assert_eq!(body["user"]["email_verified"], false);

    let sent = app.mail.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].kind, EmailKind::VerifyEmail);
    assert_eq!(sent[0].to, "ada@example.test");
    assert!(
        sent[0]
            .text
            .contains("https://app.test/verify-email?token="),
        "{}",
        sent[0].text
    );
    assert!(
        !sent[0].text.contains("Ada"),
        "no personal data beyond the address"
    );

    let token = app.last_token(EmailKind::VerifyEmail).await;
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM email_tokens")
        .fetch_one(app.db.pool())
        .await
        .unwrap();
    assert_ne!(stored, token.as_bytes(), "only the digest is stored");

    assert_eq!(app.verify(&token).await.status(), StatusCode::NO_CONTENT);
    assert!(app.verified("ada@example.test").await);
    assert_invalid_token(app.verify(&token).await).await;
}

#[tokio::test]
async fn verification_links_expire() {
    let app = App::new().await;
    app.register("late@example.test").await;
    let token = app.last_token(EmailKind::VerifyEmail).await;

    app.clock.advance(VERIFY_TTL);
    assert_invalid_token(app.verify(&token).await).await;
    assert!(!app.verified("late@example.test").await);
}

#[tokio::test]
async fn malformed_and_unknown_tokens_are_rejected() {
    let app = App::new().await;
    assert_invalid_token(app.verify("garbage").await).await;
    assert_invalid_token(app.verify(&"A".repeat(43)).await).await;
    assert_invalid_token(app.reset("garbage", "new password 1").await).await;
}

#[tokio::test]
async fn resend_invalidates_the_previous_link_and_stops_once_verified() {
    let app = App::new().await;
    let (session, _) = app.register("again@example.test").await;
    let first = app.last_token(EmailKind::VerifyEmail).await;

    let unauthenticated = app
        .call(Method::POST, "/auth/email/verification", None, None)
        .await;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .call(
            Method::POST,
            "/auth/email/verification",
            Some(&session),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(app.count(EmailKind::VerifyEmail), 2);
    let second = app.last_token(EmailKind::VerifyEmail).await;

    assert_invalid_token(app.verify(&first).await).await;
    assert_eq!(app.verify(&second).await.status(), StatusCode::NO_CONTENT);

    app.call(
        Method::POST,
        "/auth/email/verification",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(
        app.count(EmailKind::VerifyEmail),
        2,
        "already verified: nothing sent"
    );
}

#[tokio::test]
async fn a_link_for_a_previous_address_no_longer_verifies() {
    let app = App::new().await;
    app.register("old@example.test").await;
    let token = app.last_token(EmailKind::VerifyEmail).await;

    // Address change (profile API will do this through the service).
    sqlx::query("UPDATE users SET email = 'new@example.test'")
        .execute(app.db.pool())
        .await
        .unwrap();
    assert_invalid_token(app.verify(&token).await).await;
    assert!(!app.verified("new@example.test").await);
}

#[tokio::test]
async fn forgot_password_answers_the_same_for_unknown_addresses() {
    let app = App::new().await;
    app.register("known@example.test").await;

    assert_eq!(
        app.forgot("unknown@example.test").await,
        StatusCode::ACCEPTED
    );
    assert_eq!(app.forgot("KNOWN@example.test").await, StatusCode::ACCEPTED);

    let token = app.last_token(EmailKind::ResetPassword).await;
    assert_eq!(token.len(), 43);
    let resets: Vec<String> = app
        .mail
        .sent()
        .into_iter()
        .filter(|m| m.kind == EmailKind::ResetPassword)
        .map(|m| m.to)
        .collect();
    assert_eq!(resets, ["known@example.test"]);
}

#[tokio::test]
async fn reset_sets_the_password_signs_out_everywhere_and_verifies() {
    let app = App::new().await;
    let (session, _) = app.register("reset@example.test").await;
    app.forgot("reset@example.test").await;
    let token = app.last_token(EmailKind::ResetPassword).await;

    let response = app.reset(&token, "brand new password").await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );

    assert_eq!(
        app.login("reset@example.test", PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.login("reset@example.test", "brand new password").await,
        StatusCode::OK
    );
    let old_session = app
        .call(Method::GET, "/auth/sessions", Some(&session), None)
        .await;
    assert_eq!(old_session.status(), StatusCode::UNAUTHORIZED);
    assert!(app.verified("reset@example.test").await);

    // T0.5.6: every session of the user ended; their WebSocket connections close.
    let ended: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM outbox WHERE event_type = 'identity.sessions_ended'",
    )
    .fetch_all(app.db.pool())
    .await
    .unwrap();
    assert_eq!(ended.len(), 1);
    assert!(ended[0]["user_id"].is_string());
    assert!(ended[0]["session_id"].is_null());

    assert_invalid_token(app.reset(&token, "another password").await).await;
}

#[tokio::test]
async fn reset_links_expire_and_only_the_newest_works() {
    let app = App::new().await;
    app.register("twice@example.test").await;
    app.forgot("twice@example.test").await;
    let first = app.last_token(EmailKind::ResetPassword).await;
    app.forgot("twice@example.test").await;
    while app.count(EmailKind::ResetPassword) < 2 {
        tokio::task::yield_now().await;
    }
    let second = app.last_token(EmailKind::ResetPassword).await;
    assert_ne!(first, second);

    assert_invalid_token(app.reset(&first, "new password 1").await).await;
    app.clock.advance(RESET_TTL - Duration::seconds(1));
    let weak = app.reset(&second, "short").await;
    assert_eq!(weak.status(), StatusCode::UNPROCESSABLE_ENTITY);
    app.clock.advance(Duration::seconds(1));
    assert_invalid_token(app.reset(&second, "new password 1").await).await;
}

#[tokio::test]
async fn tokens_only_work_for_their_purpose() {
    let app = App::new().await;
    app.register("purpose@example.test").await;
    let verify = app.last_token(EmailKind::VerifyEmail).await;
    app.forgot("purpose@example.test").await;
    let reset = app.last_token(EmailKind::ResetPassword).await;

    assert_invalid_token(app.reset(&verify, "new password 1").await).await;
    assert_invalid_token(app.verify(&reset).await).await;
}
