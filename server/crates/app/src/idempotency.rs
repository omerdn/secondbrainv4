//! Server-side idempotency keys (ARCHITECTURE §10 "Yazma işlemleri", ADR-0003, T0.2.8).
//!
//! Every mutation sent by a client carries a client-generated `Idempotency-Key` header (a
//! UUID, see `packages/api-client`). For routes wrapped with [`protect`]:
//!
//! - `POST`, `PUT`, `PATCH` and `DELETE` from a signed-in user **must** carry the header
//!   (422 `validation_failed` otherwise). Reads pass through untouched.
//! - The first request with a key is executed and its response (status, content type, body)
//!   is stored for [`RETENTION`]. A repeat with the same key and the same request gets the
//!   stored response with `Idempotent-Replayed: true`, without running the handler again.
//! - The same key with a different method, path or body is rejected (422).
//! - While the first request is still running, a repeat gets 409 `conflict`. If the first
//!   attempt died (crash, cancelled connection) its claim expires after [`LEASE`] and the next
//!   repeat executes the request.
//! - 5xx responses are not stored, so the client may retry with the same key.
//! - Keys are scoped per user. Requests without a valid session pass through, so the handler
//!   answers 401 itself.
//!
//! # Exempt routes
//! `/auth/*` is not wrapped: those requests have no session yet and their responses carry
//! session tokens, which must never be stored. Register is naturally idempotent (unique
//! e-mail); a repeated login only creates another session.
//!
//! # Guarantee and its limit
//! Concurrent requests with one key cannot both execute (the claim is a primary-key insert).
//! The claim and the handler's own transaction are separate, so if the process dies after the
//! handler commits but before the response is stored, a retry after [`LEASE`] runs it again.

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use db::PgPool;
use identity::{AuthContext, IdentityService, UserId};
use kernel::{AppError, FieldError, FieldErrorCode, SharedClock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Request header carrying the key.
pub const KEY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");
/// Response header set on replayed responses.
pub const REPLAYED_HEADER: HeaderName = HeaderName::from_static("idempotent-replayed");
/// How long a key is remembered.
pub const RETENTION: Duration = Duration::hours(24);
/// How long an unfinished attempt blocks repeats before another attempt may take over.
pub const LEASE: Duration = Duration::minutes(2);
/// Longest accepted key, in bytes.
pub const MAX_KEY_LEN: usize = 255;

const FIELD: &str = "Idempotency-Key";
/// Request bodies are buffered to hash them (axum's default body limit).
const MAX_REQUEST_BODY: usize = 2 * 1024 * 1024;
/// Responses larger than this are not stored (none of ours come close).
const MAX_STORED_RESPONSE: usize = 1024 * 1024;

/// Middleware state.
#[derive(Clone)]
pub struct Idempotency {
    pool: PgPool,
    identity: IdentityService,
    clock: SharedClock,
}

impl Idempotency {
    /// Creates the middleware state.
    pub fn new(pool: PgPool, identity: IdentityService, clock: SharedClock) -> Self {
        Self {
            pool,
            identity,
            clock,
        }
    }
}

/// Wraps `routes` (one or more module routers) with idempotency handling.
pub fn protect(routes: Router, state: Idempotency) -> Router {
    routes.layer(axum::middleware::from_fn_with_state(state, middleware))
}

fn is_mutation(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// Checks the header value: 1–255 visible ASCII characters.
pub fn validate_key(value: Option<&HeaderValue>) -> Result<String, FieldError> {
    let value = value.ok_or_else(|| {
        FieldError::new(
            FIELD,
            FieldErrorCode::Required,
            "mutations require an Idempotency-Key header",
        )
    })?;
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return Err(FieldError::new(
            FIELD,
            FieldErrorCode::Required,
            "must not be empty",
        ));
    }
    if bytes.len() > MAX_KEY_LEN {
        return Err(FieldError::new(
            FIELD,
            FieldErrorCode::TooLong,
            "must be at most 255 characters",
        ));
    }
    if !bytes.iter().all(|b| (0x21..=0x7e).contains(b)) {
        return Err(FieldError::new(
            FIELD,
            FieldErrorCode::Invalid,
            "must be visible ASCII characters",
        ));
    }
    // Checked above: visible ASCII is valid UTF-8.
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Fingerprint of a request: method, path with query, body.
pub fn request_hash(method: &Method, path_and_query: &str, body: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(method.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(path_and_query.as_bytes());
    hasher.update(b"\n");
    hasher.update(body);
    hasher.finalize().into()
}

/// A completed response kept for replays.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stored {
    status: StatusCode,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl IntoResponse for Stored {
    fn into_response(self) -> Response {
        let mut response = (self.status, self.body).into_response();
        let headers = response.headers_mut();
        headers.remove(header::CONTENT_TYPE);
        if let Some(value) = self
            .content_type
            .and_then(|ct| HeaderValue::from_str(&ct).ok())
        {
            headers.insert(header::CONTENT_TYPE, value);
        }
        headers.insert(REPLAYED_HEADER, HeaderValue::from_static("true"));
        response
    }
}

/// Outcome of claiming a key.
#[derive(Debug)]
enum Claim {
    /// This request executes; `started` identifies the attempt.
    Execute {
        started: DateTime<Utc>,
    },
    Replay(Stored),
    Mismatch,
    InProgress,
}

type Row = (
    Vec<u8>,
    Option<i16>,
    Option<String>,
    Option<Vec<u8>>,
    DateTime<Utc>,
);

impl Idempotency {
    async fn claim(&self, user: UserId, key: &str, hash: &[u8; 32]) -> Result<Claim, AppError> {
        // A row can disappear between the insert and the select (5xx cleanup); retry then.
        for _ in 0..3 {
            let now = self.clock.now();
            let inserted = sqlx::query(
                "INSERT INTO idempotency_keys (user_id, key, request_hash, created_at)
                 VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            )
            .bind(user.as_uuid())
            .bind(key)
            .bind(hash.as_slice())
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(AppError::internal)?
            .rows_affected();
            if inserted == 1 {
                return Ok(Claim::Execute { started: now });
            }

            let row = sqlx::query_as::<_, Row>(
                "SELECT request_hash, status, content_type, response_body, created_at
                 FROM idempotency_keys WHERE user_id = $1 AND key = $2",
            )
            .bind(user.as_uuid())
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::internal)?;
            let Some((stored_hash, status, content_type, body, started)) = row else {
                continue;
            };

            if stored_hash != hash.as_slice() {
                return Ok(Claim::Mismatch);
            }
            if let Some(status) = status {
                let status = u16::try_from(status)
                    .ok()
                    .and_then(|s| StatusCode::from_u16(s).ok())
                    .ok_or_else(|| AppError::internal("invalid stored status"))?;
                return Ok(Claim::Replay(Stored {
                    status,
                    content_type,
                    body: body.unwrap_or_default(),
                }));
            }
            if started + LEASE > now {
                return Ok(Claim::InProgress);
            }
            // The previous attempt's lease expired: take over (compare-and-set on its start).
            let taken = sqlx::query(
                "UPDATE idempotency_keys SET created_at = $3
                 WHERE user_id = $1 AND key = $2 AND status IS NULL AND created_at = $4",
            )
            .bind(user.as_uuid())
            .bind(key)
            .bind(now)
            .bind(started)
            .execute(&self.pool)
            .await
            .map_err(AppError::internal)?
            .rows_affected();
            return Ok(if taken == 1 {
                Claim::Execute { started: now }
            } else {
                Claim::InProgress
            });
        }
        Err(AppError::internal("idempotency claim did not settle"))
    }

    /// Removes this attempt's claim so the client can retry.
    async fn release(&self, user: UserId, key: &str, started: DateTime<Utc>) {
        let result = sqlx::query(
            "DELETE FROM idempotency_keys
             WHERE user_id = $1 AND key = $2 AND status IS NULL AND created_at = $3",
        )
        .bind(user.as_uuid())
        .bind(key)
        .bind(started)
        .execute(&self.pool)
        .await;
        if let Err(err) = result {
            tracing::error!(error = %kernel::redact::redact(&err.to_string()), "idempotency release failed");
        }
    }

    async fn complete(
        &self,
        user: UserId,
        key: &str,
        started: DateTime<Utc>,
        stored: &Stored,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE idempotency_keys
             SET status = $4, content_type = $5, response_body = $6, completed_at = $7
             WHERE user_id = $1 AND key = $2 AND status IS NULL AND created_at = $3",
        )
        .bind(user.as_uuid())
        .bind(key)
        .bind(started)
        .bind(i16::try_from(stored.status.as_u16()).unwrap_or(i16::MAX))
        .bind(stored.content_type.as_deref())
        .bind(stored.body.as_slice())
        .bind(self.clock.now())
        .execute(&self.pool)
        .await
        .map(|_| ())
    }
}

async fn middleware(State(state): State<Idempotency>, request: Request, next: Next) -> Response {
    if !is_mutation(request.method()) {
        return next.run(request).await;
    }
    let Some(token) = identity::http::presented_token(request.headers()) else {
        return next.run(request).await;
    };
    let auth: AuthContext = match state.identity.authenticate(&token).await {
        Ok(auth) => auth,
        // Not signed in: the handler's own extractor answers 401.
        Err(AppError::Unauthenticated) => return next.run(request).await,
        Err(err) => return err.into_response(),
    };
    let key = match validate_key(request.headers().get(&KEY_HEADER)) {
        Ok(key) => key,
        Err(field) => return AppError::from(kernel::ValidationErrors::from(field)).into_response(),
    };

    let (mut parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, MAX_REQUEST_BODY).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let path = parts
        .uri
        .path_and_query()
        .map_or_else(|| parts.uri.path(), |pq| pq.as_str());
    let hash = request_hash(&parts.method, path, &bytes);

    let started = match state.claim(auth.user_id, &key, &hash).await {
        Ok(Claim::Execute { started }) => started,
        Ok(Claim::Replay(stored)) => return stored.into_response(),
        Ok(Claim::Mismatch) => {
            let field = FieldError::new(
                FIELD,
                FieldErrorCode::Invalid,
                "already used for a different request",
            );
            return AppError::from(kernel::ValidationErrors::from(field)).into_response();
        }
        Ok(Claim::InProgress) => {
            return AppError::Conflict(
                "a request with this idempotency key is still in progress".into(),
            )
            .into_response();
        }
        Err(err) => return err.into_response(),
    };

    // The handler sees the same authentication (no second session lookup).
    parts.extensions.insert(auth);
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;

    if response.status().is_server_error() {
        state.release(auth.user_id, &key, started).await;
        return response;
    }
    let (parts, body) = response.into_parts();
    let Ok(body): Result<Bytes, _> = to_bytes(body, MAX_STORED_RESPONSE).await else {
        state.release(auth.user_id, &key, started).await;
        return AppError::internal("response too large to store for idempotency").into_response();
    };
    let stored = Stored {
        status: parts.status,
        content_type: parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        body: body.to_vec(),
    };
    if let Err(err) = state.complete(auth.user_id, &key, started, &stored).await {
        // The change already happened; the client still gets its response. A retry after the
        // lease would execute again, so make this visible.
        tracing::error!(error = %kernel::redact::redact(&err.to_string()), "idempotency store failed");
    }
    Response::from_parts(parts, Body::from(body))
}

/// Deletes keys created before `before`, in small batches. Returns the number deleted.
pub async fn delete_expired(pool: &PgPool, before: DateTime<Utc>) -> Result<u64, sqlx::Error> {
    const BATCH: i64 = 1000;
    let mut total = 0;
    loop {
        let deleted = sqlx::query(
            "DELETE FROM idempotency_keys WHERE (user_id, key) IN (
                 SELECT user_id, key FROM idempotency_keys WHERE created_at < $1 LIMIT $2)",
        )
        .bind(before)
        .bind(BATCH)
        .execute(pool)
        .await?
        .rows_affected();
        total += deleted;
        if deleted < u64::try_from(BATCH).unwrap_or(u64::MAX) {
            return Ok(total);
        }
    }
}

/// Daily cleanup of keys older than [`RETENTION`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupIdempotencyKeys {}

impl jobs::Job for CleanupIdempotencyKeys {
    const KIND: &'static str = "idempotency.cleanup";
}

/// Cron expression for [`CleanupIdempotencyKeys`] (every day 03:30 UTC).
pub const CLEANUP_CRON: &str = "0 30 3 * * *";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mutations_are_handled() {
        for m in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(is_mutation(&m));
        }
        for m in [Method::GET, Method::HEAD, Method::OPTIONS] {
            assert!(!is_mutation(&m));
        }
    }

    #[test]
    fn key_validation() {
        let code = |v: Option<&str>| {
            validate_key(v.map(|v| HeaderValue::from_str(v).unwrap()).as_ref())
                .err()
                .map(|e| e.code)
        };
        assert_eq!(code(None), Some(FieldErrorCode::Required));
        assert_eq!(code(Some("")), Some(FieldErrorCode::Required));
        assert_eq!(code(Some("has space")), Some(FieldErrorCode::Invalid));
        assert_eq!(code(Some(&"k".repeat(256))), Some(FieldErrorCode::TooLong));
        assert_eq!(code(Some(&"k".repeat(255))), None);
        assert_eq!(code(Some("0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b")), None);
    }

    #[test]
    fn request_hash_covers_method_path_and_body() {
        let base = request_hash(&Method::POST, "/items", b"{}");
        assert_eq!(base, request_hash(&Method::POST, "/items", b"{}"));
        assert_ne!(base, request_hash(&Method::PUT, "/items", b"{}"));
        assert_ne!(base, request_hash(&Method::POST, "/items?x=1", b"{}"));
        assert_ne!(base, request_hash(&Method::POST, "/items", b"{ }"));
        // The separator keeps path/body boundaries unambiguous.
        assert_ne!(
            request_hash(&Method::POST, "/a", b"b"),
            request_hash(&Method::POST, "/a\nb", b"")
        );
    }
}
