//! HTTP router composition.
//!
//! Domain modules will contribute their own routers here as they are implemented.

use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

/// Builds the application router.
pub fn router() -> Router {
    Router::new().route("/health", get(health))
}

/// Body of `GET /health`.
#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

/// Liveness probe: the process is up and serving HTTP. Dependency checks (database) are added
/// once the `db` crate exists (T0.2.3).
async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}
