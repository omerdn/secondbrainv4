//! Request logging, request ids and the no-personal-data rule (T0.2.4, ARCHITECTURE §15).
//!
//! Logs are captured from the real JSON formatter and checked line by line.

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io;
use std::sync::{Arc, Mutex};

use app::http::{REQUEST_ID_HEADER, with_observability};
use app::telemetry::{LogFormat, fmt_layer};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use axum::routing::get;
use kernel::AppError;
use serde_json::Value;
use tower::ServiceExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;

/// Personal data planted in requests; none of it may appear in any log line.
const EMAIL: &str = "ali.yilmaz@example.com";
const TOKEN: &str = "s3cr3t-token-value";
const IP: &str = "203.0.113.7";
const COOKIE: &str = "session=abc123def";

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    fn lines(&self) -> Vec<Value> {
        self.text()
            .lines()
            .map(|line| serde_json::from_str(line).expect("every log line is JSON"))
            .collect()
    }

    fn event(&self, message: &str) -> Value {
        self.lines()
            .into_iter()
            .find(|line| line["message"] == message)
            .unwrap_or_else(|| panic!("no {message:?} event in:\n{}", self.text()))
    }
}

impl io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Runs one request through `router` with JSON logging captured for this thread.
async fn call_logged(router: Router, request: Request<Body>) -> (Response, Captured) {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new("info"))
        .with(fmt_layer(LogFormat::Json, captured.clone()));
    let _guard = tracing::subscriber::set_default(subscriber);

    let response = router.oneshot(request).await.unwrap();
    (response, captured)
}

fn request_with_personal_data(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("cookie", COOKIE)
        .header("x-forwarded-for", IP)
        .header("x-real-ip", IP)
        .header("user-agent", format!("test ({EMAIL})"))
        .body(Body::empty())
        .unwrap()
}

fn assert_no_personal_data(captured: &Captured) {
    let text = captured.text();
    for secret in [EMAIL, TOKEN, IP, COOKIE, "abc123def"] {
        assert!(!text.contains(secret), "log contains {secret:?}:\n{text}");
    }
}

#[tokio::test]
async fn request_log_has_route_status_latency_and_request_id_only() {
    let request = Request::builder()
        .uri(format!("/health?email={EMAIL}&token={TOKEN}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("cookie", COOKIE)
        .header("x-forwarded-for", IP)
        .header(REQUEST_ID_HEADER, "req-123")
        .body(Body::empty())
        .unwrap();

    let (response, captured) = call_logged(app::http::router(), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[REQUEST_ID_HEADER], "req-123");

    let event = captured.event("request completed");
    assert_eq!(event["level"], "INFO");
    assert_eq!(event["status"], 200);
    assert!(event["latency_ms"].is_u64());
    assert_eq!(event["span"]["method"], "GET");
    assert_eq!(event["span"]["route"], "/health");
    assert_eq!(event["span"]["request_id"], "req-123");

    assert_no_personal_data(&captured);
    assert!(!captured.text().contains('?'), "query string leaked");
}

#[tokio::test]
async fn unmatched_paths_are_not_logged_verbatim() {
    let (response, captured) = call_logged(
        app::http::router(),
        request_with_personal_data(&format!("/users/{EMAIL}/profile")),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let event = captured.event("request completed");
    assert_eq!(event["span"]["route"], "<unmatched>");
    assert_eq!(event["status"], 404);
    assert_no_personal_data(&captured);
}

#[tokio::test]
async fn route_templates_hide_path_parameters() {
    let router = with_observability(Router::new().route("/items/{id}", get(|| async { "ok" })));
    let (_, captured) = call_logged(
        router,
        request_with_personal_data(&format!("/items/{EMAIL}")),
    )
    .await;

    assert_eq!(
        captured.event("request completed")["span"]["route"],
        "/items/{id}"
    );
    assert_no_personal_data(&captured);
}

#[tokio::test]
async fn generates_request_id_when_missing() {
    let (response, captured) = call_logged(
        app::http::router(),
        Request::get("/health").body(Body::empty()).unwrap(),
    )
    .await;

    let id = response.headers()[REQUEST_ID_HEADER]
        .to_str()
        .unwrap()
        .to_owned();
    let uuid = uuid::Uuid::parse_str(&id).expect("generated id is a UUID");
    assert_eq!(uuid.get_version_num(), 7);
    assert_eq!(
        captured.event("request completed")["span"]["request_id"],
        id
    );
}

#[tokio::test]
async fn replaces_malformed_client_request_ids() {
    let request = Request::get("/health")
        .header(REQUEST_ID_HEADER, EMAIL)
        .body(Body::empty())
        .unwrap();
    let (response, captured) = call_logged(app::http::router(), request).await;

    let id = response.headers()[REQUEST_ID_HEADER].to_str().unwrap();
    assert_ne!(id, EMAIL);
    assert!(uuid::Uuid::parse_str(id).is_ok());
    assert_no_personal_data(&captured);
}

#[tokio::test]
async fn internal_errors_are_logged_redacted_within_the_request_span() {
    let router = with_observability(Router::new().route(
        "/boom",
        get(|| async {
            Err::<(), _>(AppError::internal(io::Error::other(format!(
                "Key (email)=({EMAIL}) already exists; peer {IP}"
            ))))
        }),
    ));
    let request = Request::get("/boom")
        .header(REQUEST_ID_HEADER, "req-boom")
        .body(Body::empty())
        .unwrap();

    let (response, captured) = call_logged(router, request).await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let error = captured.event("internal error");
    assert_eq!(error["level"], "ERROR");
    assert_eq!(error["span"]["request_id"], "req-boom");
    assert_eq!(error["span"]["route"], "/boom");
    let detail = error["error"].as_str().unwrap();
    assert!(
        detail.contains("Key (email)=(***) already exists"),
        "{detail}"
    );
    assert!(detail.contains("<ip>"), "{detail}");
    assert_eq!(captured.event("request completed")["status"], 500);
    assert_no_personal_data(&captured);
}
