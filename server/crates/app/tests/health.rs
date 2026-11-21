//! `GET /health` (T0.2.1).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn call(method: Method, uri: &str) -> axum::response::Response {
    app::http::router()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn health_returns_ok_json() {
    let response = call(Method::GET, "/health").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, serde_json::json!({ "status": "ok" }));
}

#[tokio::test]
async fn health_only_accepts_get() {
    let response = call(Method::POST, "/health").await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn unknown_routes_are_not_found() {
    let response = call(Method::GET, "/nope").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
