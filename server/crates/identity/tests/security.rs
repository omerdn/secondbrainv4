//! Rate limits and CSRF protection (T0.3.6) against a real Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::rate_limit::{CREDENTIALS, EMAIL, RateLimits};
use identity::{EmailSettings, IdentityService};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn app(trust_proxy_headers: bool) -> (TestDb, Router) {
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
    let router = identity::http::rate_limited_router(
        service,
        RateLimits {
            trust_proxy_headers,
        },
    );
    (db, router)
}

/// Request builder with a peer address (as the real server provides).
struct Call<'a> {
    method: Method,
    uri: &'a str,
    peer: &'a str,
    headers: Vec<(header::HeaderName, String)>,
    body: Option<Value>,
}

impl<'a> Call<'a> {
    fn new(method: Method, uri: &'a str) -> Self {
        Self {
            method,
            uri,
            peer: "198.51.100.1:5000",
            headers: Vec::new(),
            body: None,
        }
    }
    fn peer(mut self, peer: &'a str) -> Self {
        self.peer = peer;
        self
    }
    fn header(mut self, name: impl Into<header::HeaderName>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
    fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }
    async fn send(self, router: &Router) -> Response {
        let mut request = Request::builder().method(self.method).uri(self.uri);
        for (name, value) in self.headers {
            request = request.header(name, value);
        }
        let body = match self.body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let mut request = request.body(body).unwrap();
        let peer: SocketAddr = self.peer.parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(peer));
        router.clone().oneshot(request).await.unwrap()
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn login_body() -> Value {
    json!({ "email": "nobody@example.test", "password": "wrong password", "client": "web" })
}

#[tokio::test]
async fn login_is_rate_limited_per_client_ip() {
    let (_db, router) = app(false).await;
    for _ in 0..CREDENTIALS.burst {
        let response = Call::new(Method::POST, "/auth/login")
            .json(login_body())
            .send(&router)
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let limited = Call::new(Method::POST, "/auth/login")
        .json(login_body())
        .send(&router)
        .await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = limited.headers()[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=CREDENTIALS.refill.as_secs()).contains(&retry_after));
    assert_eq!(json_body(limited).await["error"]["code"], "rate_limited");

    // Another client is not affected; neither are unlimited endpoints for the first client.
    let other = Call::new(Method::POST, "/auth/login")
        .peer("203.0.113.9:4000")
        .json(login_body())
        .send(&router)
        .await;
    assert_eq!(other.status(), StatusCode::UNAUTHORIZED);
    let sessions = Call::new(Method::GET, "/auth/sessions").send(&router).await;
    assert_eq!(sessions.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn register_shares_the_credentials_bucket() {
    let (_db, router) = app(false).await;
    for i in 0..CREDENTIALS.burst {
        let body = json!({
            "email": format!("u{i}@example.test"), "password": "correct horse",
            "display_name": "U", "client": "native"
        });
        let response = Call::new(Method::POST, "/auth/register")
            .json(body)
            .send(&router)
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    let limited = Call::new(Method::POST, "/auth/login")
        .json(login_body())
        .send(&router)
        .await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn password_forgot_has_a_tighter_limit() {
    let (_db, router) = app(false).await;
    let body = json!({ "email": "someone@example.test" });
    for _ in 0..EMAIL.burst {
        let response = Call::new(Method::POST, "/auth/password/forgot")
            .json(body.clone())
            .send(&router)
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }
    let limited = Call::new(Method::POST, "/auth/password/forgot")
        .json(body)
        .send(&router)
        .await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn proxy_headers_select_the_client_only_when_trusted() {
    // Trusted: each forwarded address has its own bucket even behind one proxy peer.
    let (_db, router) = app(true).await;
    for _ in 0..CREDENTIALS.burst {
        Call::new(Method::POST, "/auth/login")
            .header(
                header::HeaderName::from_static("x-forwarded-for"),
                "192.0.2.1",
            )
            .json(login_body())
            .send(&router)
            .await;
    }
    let first = Call::new(Method::POST, "/auth/login")
        .header(
            header::HeaderName::from_static("x-forwarded-for"),
            "192.0.2.1",
        )
        .json(login_body())
        .send(&router)
        .await;
    assert_eq!(first.status(), StatusCode::TOO_MANY_REQUESTS);
    let second = Call::new(Method::POST, "/auth/login")
        .header(
            header::HeaderName::from_static("x-forwarded-for"),
            "192.0.2.2",
        )
        .json(login_body())
        .send(&router)
        .await;
    assert_eq!(second.status(), StatusCode::UNAUTHORIZED);

    // Not trusted: a spoofed header does not give a fresh bucket.
    let (_db, router) = app(false).await;
    for i in 0..=CREDENTIALS.burst {
        let response = Call::new(Method::POST, "/auth/login")
            .header(
                header::HeaderName::from_static("x-forwarded-for"),
                format!("192.0.2.{i}"),
            )
            .json(login_body())
            .send(&router)
            .await;
        if i == CREDENTIALS.burst {
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }
    }
}

/// Registers a web client; returns (`sb_session=…` cookie pair, CSRF token).
async fn web_session(router: &Router, email: &str) -> (String, String) {
    let response = Call::new(Method::POST, "/auth/register")
        .peer("192.0.2.200:1")
        .json(json!({ "email": email, "password": "correct horse", "display_name": "W", "client": "web" }))
        .send(router)
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body = json_body(response).await;
    (cookie, body["csrf_token"].as_str().unwrap().to_owned())
}

async fn session_ids(router: &Router, cookie: &str) -> Vec<String> {
    let response = Call::new(Method::GET, "/auth/sessions")
        .header(header::COOKIE, cookie)
        .send(router)
        .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "reads need no CSRF token"
    );
    json_body(response)
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn cookie_mutations_require_the_csrf_token() {
    let (_db, router) = app(false).await;
    let (cookie, csrf) = web_session(&router, "web@example.test").await;
    let id = session_ids(&router, &cookie).await.remove(0);
    let uri = format!("/auth/sessions/{id}");

    let missing = Call::new(Method::DELETE, &uri)
        .header(header::COOKIE, cookie.clone())
        .send(&router)
        .await;
    assert_eq!(missing.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(missing).await["error"]["code"], "forbidden");

    let wrong = Call::new(Method::DELETE, &uri)
        .header(header::COOKIE, cookie.clone())
        .header(
            header::HeaderName::from_static("x-csrf-token"),
            "A".repeat(43),
        )
        .send(&router)
        .await;
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        session_ids(&router, &cookie).await.len(),
        1,
        "nothing was revoked"
    );

    let ok = Call::new(Method::DELETE, &uri)
        .header(header::COOKIE, cookie.clone())
        .header(header::HeaderName::from_static("x-csrf-token"), csrf)
        .send(&router)
        .await;
    assert_eq!(ok.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_csrf_token_only_works_for_its_own_session() {
    let (_db, router) = app(false).await;
    let (cookie_a, _) = web_session(&router, "a@example.test").await;
    let (_, csrf_b) = web_session(&router, "b@example.test").await;
    let response = Call::new(Method::POST, "/auth/email/verification")
        .header(header::COOKIE, cookie_a)
        .header(header::HeaderName::from_static("x-csrf-token"), csrf_b)
        .send(&router)
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn bearer_requests_need_no_csrf_token() {
    let (_db, router) = app(false).await;
    let response = Call::new(Method::POST, "/auth/register")
        .json(json!({ "email": "n@example.test", "password": "correct horse", "display_name": "N", "client": "native" }))
        .send(&router)
        .await;
    let body = json_body(response).await;
    assert!(
        body.get("csrf_token").is_none(),
        "native clients get no CSRF token"
    );
    let token = body["token"].as_str().unwrap();
    let response = Call::new(Method::POST, "/auth/email/verification")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .send(&router)
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn csrf_endpoint_returns_the_token_of_the_cookie_session() {
    let (_db, router) = app(false).await;
    let (cookie, csrf) = web_session(&router, "reload@example.test").await;
    let response = Call::new(Method::GET, "/auth/csrf")
        .header(header::COOKIE, cookie)
        .send(&router)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(json_body(response).await["csrf_token"], csrf);

    let anonymous = Call::new(Method::GET, "/auth/csrf").send(&router).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cookie_logout_revokes_the_session_only_with_the_csrf_token() {
    let (_db, router) = app(false).await;
    let (cookie, csrf) = web_session(&router, "out@example.test").await;

    let forged = Call::new(Method::POST, "/auth/logout")
        .header(header::COOKIE, cookie.clone())
        .send(&router)
        .await;
    assert_eq!(forged.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        session_ids(&router, &cookie).await.len(),
        1,
        "a forged logout revokes nothing"
    );

    let real = Call::new(Method::POST, "/auth/logout")
        .header(header::COOKIE, cookie.clone())
        .header(header::HeaderName::from_static("x-csrf-token"), csrf)
        .send(&router)
        .await;
    assert_eq!(real.status(), StatusCode::NO_CONTENT);
    let after = Call::new(Method::GET, "/auth/sessions")
        .header(header::COOKIE, cookie)
        .send(&router)
        .await;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn json_endpoints_reject_form_posts() {
    // Cross-site HTML forms cannot send application/json, so login CSRF is not possible.
    let (_db, router) = app(false).await;
    let response = Call::new(Method::POST, "/auth/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .send(&router)
        .await;
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}
