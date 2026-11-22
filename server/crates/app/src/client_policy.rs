//! Minimum supported client version (T4.2.7, ADR-0023).
//!
//! Desktop, iOS and Android builds update themselves for most changes (desktop updater, OTA),
//! but a native or protocol change can leave an old install behind. `GET /client/policy` tells
//! each platform the oldest version the server still supports and where to get a new one; an
//! older app blocks with a "download the new version" screen. The web app is always current.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use ts_rs::TS;

/// Body of `GET /client/policy`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ClientPolicy {
    pub ios: PlatformPolicy,
    pub android: PlatformPolicy,
    pub desktop: PlatformPolicy,
}

/// Version policy of one platform.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PlatformPolicy {
    /// Oldest supported version (`major.minor.patch`); `null` = every version is supported.
    pub min_version: Option<String>,
    /// Where a user gets the current version.
    pub download_url: Option<String>,
}

/// `GET /client/policy` (public, no session: the check runs before sign-in).
pub fn router(policy: ClientPolicy) -> Router {
    Router::new()
        .route("/client/policy", get(policy_handler))
        .with_state(Arc::new(policy))
}

async fn policy_handler(State(policy): State<Arc<ClientPolicy>>) -> Json<ClientPolicy> {
    Json((*policy).clone())
}

/// `major.minor.patch` with numeric parts.
pub(crate) fn is_version(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[test]
    fn versions_are_three_numbers() {
        assert!(is_version("0.1.0"));
        assert!(is_version("12.34.567"));
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.x",
            "1..3",
            "1.2.3-beta",
        ] {
            assert!(!is_version(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn serves_the_policy() {
        let policy = ClientPolicy {
            desktop: PlatformPolicy {
                min_version: Some("0.2.0".into()),
                download_url: Some("https://example.test/desktop".into()),
            },
            ..ClientPolicy::default()
        };
        let response = router(policy)
            .oneshot(Request::get("/client/policy").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["desktop"]["min_version"], "0.2.0");
        assert_eq!(
            json["desktop"]["download_url"],
            "https://example.test/desktop"
        );
        assert!(json["ios"]["min_version"].is_null());
    }
}
