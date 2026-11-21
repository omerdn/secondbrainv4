//! CORS for the web client (T0.4.9): only configured origins, with credentials; preflight is
//! answered before module middleware (rate limits, CSRF, idempotency).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Router, middleware};
use db::testing::TestDb;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tower::ServiceExt;

const WEB: &str = "http://localhost:8081";
const EVIL: &str = "https://evil.example";

/// A module route behind a middleware that rejects everything, like CSRF or a rate limit would
/// reject a preflight that reached it.
fn app() -> Router {
    let modules = Router::new()
        .route("/things", post(|| async { "created" }))
        .route_layer(middleware::from_fn(
            |_req: Request<Body>, _next: middleware::Next| async {
                StatusCode::FORBIDDEN.into_response()
            },
        ));
    app::http::router_with(modules, &[WEB.to_owned()])
}

async fn call(request: Request<Body>) -> Response {
    app().oneshot(request).await.unwrap()
}

fn preflight(origin: &str, method: &str, headers: &str) -> Request<Body> {
    Request::builder()
        .method(Method::OPTIONS)
        .uri("/things")
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, method)
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, headers)
        .body(Body::empty())
        .unwrap()
}

fn get_health(origin: &str) -> Request<Body> {
    Request::builder()
        .uri("/health")
        .header(header::ORIGIN, origin)
        .body(Body::empty())
        .unwrap()
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> &str {
    headers
        .get(name)
        .map(|value| value.to_str().unwrap())
        .unwrap_or_default()
}

fn assert_no_cors(headers: &HeaderMap, context: &str) {
    let cors: Vec<_> = headers
        .keys()
        .filter(|name| name.as_str().starts_with("access-control-"))
        .collect();
    assert!(cors.is_empty(), "{context}: {cors:?}");
}

fn list(headers: &HeaderMap, name: header::HeaderName) -> Vec<String> {
    header_str(headers, name)
        .split(',')
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .collect()
}

#[tokio::test]
async fn preflight_from_the_web_origin_is_answered_with_credentials() {
    let response = call(preflight(
        WEB,
        "POST",
        "content-type,authorization,x-csrf-token,idempotency-key",
    ))
    .await;

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "not rejected by module middleware"
    );
    let headers = response.headers();
    assert_eq!(
        header_str(headers, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        WEB
    );
    assert_eq!(
        header_str(headers, header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
        "true"
    );
    let allowed = list(headers, header::ACCESS_CONTROL_ALLOW_HEADERS);
    for name in [
        "content-type",
        "authorization",
        "x-csrf-token",
        "idempotency-key",
    ] {
        assert!(allowed.contains(&name.to_owned()), "{name} in {allowed:?}");
    }
    assert!(!allowed.contains(&"*".to_owned()));
    let methods = list(headers, header::ACCESS_CONTROL_ALLOW_METHODS);
    for method in ["get", "post", "patch", "delete"] {
        assert!(
            methods.contains(&method.to_owned()),
            "{method} in {methods:?}"
        );
    }
    assert_eq!(header_str(headers, header::ACCESS_CONTROL_MAX_AGE), "600");
}

#[tokio::test]
async fn preflight_from_another_origin_gets_no_cors_headers() {
    let response = call(preflight(EVIL, "POST", "content-type")).await;
    assert_no_cors(response.headers(), EVIL);
}

#[tokio::test]
async fn similar_looking_origins_are_not_allowed() {
    for origin in [
        "http://localhost:8082",
        "https://localhost:8081",
        "http://localhost:8081.evil.example",
        "null",
    ] {
        let response = call(get_health(origin)).await;
        assert_no_cors(response.headers(), origin);
        let response = call(preflight(origin, "POST", "content-type")).await;
        assert_no_cors(response.headers(), origin);
    }
}

#[tokio::test]
async fn simple_request_from_the_web_origin_exposes_client_headers() {
    let response = call(get_health(WEB)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        header_str(headers, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        WEB
    );
    assert_eq!(
        header_str(headers, header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
        "true"
    );
    assert!(
        list(headers, header::VARY).contains(&"origin".to_owned()),
        "caches must key on Origin"
    );
    let exposed = list(headers, header::ACCESS_CONTROL_EXPOSE_HEADERS);
    for name in ["retry-after", "x-request-id", "idempotent-replayed"] {
        assert!(exposed.contains(&name.to_owned()), "{name} in {exposed:?}");
    }
}

#[tokio::test]
async fn simple_request_from_another_origin_gets_no_cors_headers() {
    let response = call(get_health(EVIL)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "CORS is enforced by the browser"
    );
    assert_no_cors(response.headers(), EVIL);
}

#[tokio::test]
async fn default_router_allows_no_origin() {
    let response = app::http::router().oneshot(get_health(WEB)).await.unwrap();
    assert_no_cors(response.headers(), WEB);
}

/// The real server (`app::run`) answers a preflight for a rate-limited, CSRF-protected auth
/// route from `PUBLIC_WEB_URL` and `CORS_EXTRA_ORIGINS`.
#[tokio::test]
async fn running_server_allows_configured_origins() {
    let test_db = TestDb::new().await;
    let vars = [
        ("DATABASE_URL", test_db.url()),
        ("DB_MIGRATE_ON_START", "false"),
        ("SHUTDOWN_TIMEOUT_SECS", "5"),
        ("PUBLIC_WEB_URL", WEB),
        ("CORS_EXTRA_ORIGINS", "http://localhost:19006"),
    ];
    let config = app::Config::from_lookup(|key| {
        vars.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_owned())
    })
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(app::run(config, listener, async move {
        let _ = stopped.await;
    }));

    for (origin, allowed) in [(WEB, true), ("http://localhost:19006", true), (EVIL, false)] {
        let response = timeout(Duration::from_secs(10), raw_preflight(addr, origin))
            .await
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let lower = response.to_ascii_lowercase();
        if allowed {
            assert!(
                lower.contains(&format!("access-control-allow-origin: {origin}\r\n")),
                "{origin}: {response}"
            );
        } else {
            assert!(!lower.contains("access-control-"), "{origin}: {response}");
        }
    }

    stop.send(()).unwrap();
    timeout(Duration::from_secs(10), handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

async fn raw_preflight(addr: std::net::SocketAddr, origin: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "OPTIONS /auth/login HTTP/1.1\r\nHost: t\r\nOrigin: {origin}\r\n\
         Access-Control-Request-Method: POST\r\n\
         Access-Control-Request-Headers: content-type,x-csrf-token\r\n\
         Connection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).await.unwrap();
    buf
}
