//! Per-client rate limits for the auth endpoints (T0.3.6, ARCHITECTURE §15, `tower-governor`).
//!
//! Limits are token buckets per client IP, kept in memory per server instance (with several
//! instances the effective limit is multiplied; acceptable for brute-force protection). The
//! client IP is only used as an in-memory key and is never logged (KVKK).
//!
//! | Group | Endpoints | Burst | Refill |
//! |---|---|---|---|
//! | credentials | register, login, password reset, e-mail verify, Google/Apple native | 10 | 1 per 6 s |
//! | e-mail | password forgot, resend verification | 3 | 1 per 60 s |
//!
//! A rejected request gets 429 `rate_limited` with `Retry-After`.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::{IntoResponse, Response};
use kernel::AppError;
use tower_governor::GovernorError;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::{KeyExtractor, SmartIpKeyExtractor};

/// How to find the client IP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RateLimits {
    /// Trust `X-Forwarded-For` / `X-Real-IP` / `Forwarded` (only behind a reverse proxy that
    /// sets them; otherwise clients could pick their own key).
    pub trust_proxy_headers: bool,
}

/// Token bucket: `burst` requests at once, then one more every `refill`.
#[derive(Debug, Clone, Copy)]
pub struct Quota {
    pub burst: u32,
    pub refill: Duration,
}

/// Credential-checking endpoints.
pub const CREDENTIALS: Quota = Quota {
    burst: 10,
    refill: Duration::from_secs(6),
};
/// Endpoints that send an e-mail.
pub const EMAIL: Quota = Quota {
    burst: 3,
    refill: Duration::from_secs(60),
};

/// Client IP key: the TCP peer, or the proxy headers when trusted.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp {
    trust_proxy_headers: bool,
}

impl KeyExtractor for ClientIp {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        if self.trust_proxy_headers {
            return SmartIpKeyExtractor.extract(req);
        }
        req.extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            .ok_or(GovernorError::UnableToExtractKey)
    }
}

fn to_response(error: GovernorError) -> Response<Body> {
    match error {
        GovernorError::TooManyRequests { wait_time, .. } => AppError::RateLimited {
            retry_after_secs: Some(wait_time.max(1)),
        }
        .into_response(),
        GovernorError::UnableToExtractKey => {
            AppError::internal("client address unavailable for rate limiting").into_response()
        }
        GovernorError::Other { .. } => AppError::internal("rate limiter failure").into_response(),
    }
}

/// Applies `quota` to every route of `routes`. Also starts a background task that forgets idle
/// clients every minute, so memory does not grow with the number of IPs seen.
pub fn limit<S: Clone + Send + Sync + 'static>(
    routes: Router<S>,
    quota: Quota,
    limits: RateLimits,
) -> Router<S> {
    let built = GovernorConfigBuilder::default()
        .period(quota.refill)
        .burst_size(quota.burst)
        .key_extractor(ClientIp {
            trust_proxy_headers: limits.trust_proxy_headers,
        })
        .finish();
    let Some(config) = built.map(Arc::new) else {
        // Only possible with a zero quota, which the constants above never are.
        tracing::error!("invalid rate limit quota; limits disabled");
        return routes;
    };
    let limiter = config.limiter().clone();
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                limiter.retain_recent();
            }
        });
    }
    routes.layer(GovernorLayer::new(config).error_handler(to_response))
}
