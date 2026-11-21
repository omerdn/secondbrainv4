//! HTTP router composition and request observability.
//!
//! Every request gets an id (`x-request-id`: the client's if well-formed, otherwise a new
//! UUIDv7), returned in the response and attached to every log line of that request.
//!
//! # What a request log contains
//! Only: method, **route template** (`/items/{id}`, never the raw path), status, latency and
//! request id. Query strings, headers (cookies, `Authorization`), client IP addresses and bodies
//! are **never** logged; under KVKK/GDPR an IP address is personal data too.

use std::time::Duration;

use axum::extract::{MatchedPath, Request};
use axum::http::{HeaderName, HeaderValue};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tower::ServiceBuilder;
use tower_http::request_id::PropagateRequestIdLayer;
use tower_http::trace::TraceLayer;
use tracing::Span;
use ts_rs::TS;

/// Request id header.
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Longest client-supplied request id that is accepted as-is.
const MAX_REQUEST_ID_LEN: usize = 64;

/// Builds the application router.
pub fn router() -> Router {
    with_observability(Router::new().route("/health", get(health)))
}

/// Adds request ids and request logging to `router`. Apply after all routes are added.
pub fn with_observability(router: Router) -> Router {
    router.layer(
        ServiceBuilder::new()
            .layer(axum::middleware::map_request(ensure_request_id))
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(make_span)
                    .on_request(())
                    .on_response(on_response)
                    // 5xx are logged where they are created (`AppError::Internal`).
                    .on_failure(()),
            )
            .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER)),
    )
}

/// Accepts the client's request id only if it is short and made of `[A-Za-z0-9._-]`, so it
/// cannot smuggle arbitrary (possibly personal) text into the logs.
fn is_valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_REQUEST_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

async fn ensure_request_id(mut request: Request) -> Request {
    let valid = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_valid_request_id);
    if !valid && let Ok(value) = HeaderValue::from_str(&uuid::Uuid::now_v7().to_string()) {
        request.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    request
}

fn make_span(request: &Request) -> Span {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str);
    let request_id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-");
    tracing::info_span!(
        "request",
        method = %request.method(),
        route = %route,
        request_id = %request_id,
    )
}

fn on_response(response: &Response, latency: Duration, _span: &Span) {
    tracing::info!(
        status = response.status().as_u16(),
        latency_ms = u64::try_from(latency.as_millis()).unwrap_or(u64::MAX),
        "request completed"
    );
}

/// Body of `GET /health`.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Health {
    /// Always `"ok"` when the process is serving.
    pub status: &'static str,
}

/// Liveness probe: the process is up and serving HTTP.
async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_id_validation() {
        assert!(is_valid_request_id("req-123"));
        assert!(is_valid_request_id("01936d3c-7a8e-7cc1-9a3b-2f4e5d6c7b8a"));
        assert!(!is_valid_request_id(""));
        assert!(!is_valid_request_id("has space"));
        assert!(!is_valid_request_id("ali@example.com"));
        assert!(!is_valid_request_id(&"a".repeat(MAX_REQUEST_ID_LEN + 1)));
    }
}
