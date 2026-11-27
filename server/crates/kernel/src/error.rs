//! Application error type and its HTTP mapping.
//!
//! Every endpoint returns `Result<_, AppError>`. The JSON body is uniform:
//!
//! ```json
//! { "error": { "code": "validation_failed", "message": "validation failed",
//!              "fields": [{ "field": "title", "code": "required", "message": "must not be blank" }] } }
//! ```
//!
//! # Not found vs. forbidden (PRD V9)
//!
//! If the caller cannot **see** an object, return [`AppError::NotFound`], never
//! [`AppError::Forbidden`]: a 403 would reveal that the object exists. `Forbidden` is only for
//! objects the caller can see but may not act on (e.g. an editor trying to delete — V6).

use std::error::Error as StdError;

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use ts_rs::TS;

use crate::validation::ValidationErrors;

/// Boxed underlying cause of an internal error.
pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Errors surfaced to API clients.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The object does not exist **or the caller cannot see it** (V9).
    #[error("not found")]
    NotFound,
    /// No valid session.
    #[error("authentication required")]
    Unauthenticated,
    /// The caller can see the object but lacks the permission for this action.
    #[error("forbidden")]
    Forbidden,
    /// One or more input fields are invalid.
    #[error("validation failed")]
    Validation(ValidationErrors),
    /// The request conflicts with the current state. The message is shown to the user.
    #[error("{0}")]
    Conflict(String),
    /// Too many requests (AK2, ARCHITECTURE §15).
    /// Sign-in is held until the user accepts the current legal texts (HS5, T4.3.3); the ticket
    /// finishes it through `POST /auth/consent`. Never contains personal data.
    #[error("consent required")]
    ConsentRequired { ticket: String },
    #[error("too many requests")]
    RateLimited {
        /// Value for the `Retry-After` header, if known.
        retry_after_secs: Option<u64>,
    },
    /// Unexpected failure. The cause is kept for logging and never sent to the client.
    #[error("internal error")]
    Internal(#[source] BoxError),
}

impl AppError {
    /// Wraps any error as [`AppError::Internal`].
    pub fn internal(err: impl Into<BoxError>) -> Self {
        Self::Internal(err.into())
    }

    /// Stable machine-readable code (part of the API contract).
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound => ErrorCode::NotFound,
            Self::Unauthenticated => ErrorCode::Unauthenticated,
            Self::Forbidden => ErrorCode::Forbidden,
            Self::Validation(_) => ErrorCode::ValidationFailed,
            Self::Conflict(_) => ErrorCode::Conflict,
            Self::ConsentRequired { .. } => ErrorCode::ConsentRequired,
            Self::RateLimited { .. } => ErrorCode::RateLimited,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }

    /// HTTP status for this error.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Conflict(_) | Self::ConsentRequired { .. } => StatusCode::CONFLICT,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<ValidationErrors> for AppError {
    fn from(errors: ValidationErrors) -> Self {
        Self::Validation(errors)
    }
}

/// Machine-readable error code sent to clients (`error.code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    /// See [`AppError::NotFound`].
    NotFound,
    /// See [`AppError::Unauthenticated`].
    Unauthenticated,
    /// See [`AppError::Forbidden`].
    Forbidden,
    /// See [`AppError::Validation`].
    ValidationFailed,
    /// See [`AppError::Conflict`].
    Conflict,
    /// See [`AppError::ConsentRequired`].
    ConsentRequired,
    /// See [`AppError::RateLimited`].
    RateLimited,
    /// See [`AppError::Internal`].
    Internal,
}

/// Wire format of an error response.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ErrorBody<'a> {
    /// The error payload.
    pub error: ErrorPayload<'a>,
}

/// Inner error object.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ErrorPayload<'a> {
    /// See [`AppError::code`].
    pub code: ErrorCode,
    /// Human-readable message. Generic for internal errors.
    pub message: String,
    /// Field errors, only for `validation_failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fields: Option<&'a ValidationErrors>,
    /// With `consent_required`: the ticket for `POST /auth/consent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub consent_ticket: Option<&'a str>,
}

/// `err: cause: cause...` for logging.
fn error_chain(err: &(dyn StdError + 'static)) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(cause) = &self {
            // Logged inside the request span (request id, route); the client only sees a generic
            // message. Library error text can contain personal data (e.g. Postgres constraint
            // details), so it is redacted first.
            tracing::error!(
                error = %crate::redact::redact(&error_chain(cause.as_ref())),
                "internal error"
            );
        }

        let fields = match &self {
            Self::Validation(errors) => Some(errors),
            _ => None,
        };
        let consent_ticket = match &self {
            Self::ConsentRequired { ticket } => Some(ticket.as_str()),
            _ => None,
        };
        // `Display` of `Internal` is the generic "internal error"; the source is not exposed.
        let body = ErrorBody {
            error: ErrorPayload {
                code: self.code(),
                message: self.to_string(),
                fields,
                consent_ticket,
            },
        };
        let mut response = (self.status(), Json(body)).into_response();

        if let Self::RateLimited {
            retry_after_secs: Some(secs),
        } = self
        {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(secs));
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;

    use super::*;
    use crate::validation::{FieldError, FieldErrorCode};

    async fn render(err: AppError) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn maps_each_variant_to_status_and_code() {
        let cases = [
            (AppError::NotFound, 404, "not_found"),
            (AppError::Unauthenticated, 401, "unauthenticated"),
            (AppError::Forbidden, 403, "forbidden"),
            (
                AppError::Validation(ValidationErrors::new()),
                422,
                "validation_failed",
            ),
            (AppError::Conflict("taken".into()), 409, "conflict"),
            (
                AppError::RateLimited {
                    retry_after_secs: None,
                },
                429,
                "rate_limited",
            ),
            (AppError::internal("boom"), 500, "internal"),
            (
                AppError::ConsentRequired { ticket: "t".into() },
                409,
                "consent_required",
            ),
        ];
        for (err, status, code) in cases {
            let (actual_status, headers, body) = render(err).await;
            assert_eq!(actual_status.as_u16(), status, "{code}");
            assert_eq!(headers[header::CONTENT_TYPE], "application/json");
            assert_eq!(body["error"]["code"], code);
        }
    }

    #[tokio::test]
    async fn validation_errors_list_fields() {
        let errors = ValidationErrors::from(FieldError::new(
            "title",
            FieldErrorCode::Required,
            "must not be blank",
        ));
        let (_, _, body) = render(AppError::Validation(errors)).await;
        assert_eq!(
            body,
            serde_json::json!({ "error": {
                "code": "validation_failed",
                "message": "validation failed",
                "fields": [{ "field": "title", "code": "required", "message": "must not be blank" }]
            }})
        );
    }

    #[tokio::test]
    async fn internal_errors_do_not_leak_their_cause() {
        let cause = std::io::Error::other("connection to 10.0.0.5 refused for user=ali");
        let err = AppError::internal(cause);
        assert!(err.source().is_some(), "cause is kept for logging");

        let (_, _, body) = render(err).await;
        let text = body.to_string();
        assert_eq!(body["error"]["message"], "internal error");
        assert!(!text.contains("10.0.0.5"));
        assert!(!text.contains("ali"));
        assert!(body["error"].get("fields").is_none());
    }

    #[tokio::test]
    async fn rate_limited_sets_retry_after() {
        let (_, headers, _) = render(AppError::RateLimited {
            retry_after_secs: Some(60),
        })
        .await;
        assert_eq!(headers[header::RETRY_AFTER], "60");
    }

    #[tokio::test]
    async fn conflict_message_is_user_facing() {
        let (_, _, body) = render(AppError::Conflict("already a member".into())).await;
        assert_eq!(body["error"]["message"], "already a member");
    }
}
