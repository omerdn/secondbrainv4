//! Work session endpoints (T2.6.1). Mount inside `idempotency::protect`: every command carries
//! an idempotency key (ADR-0003), and the timer itself ignores a command whose effect holds.
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | POST | `/sessions` | 200 [`WorkSession`]: the open one, opened if needed (W1) |
//! | GET  | `/sessions/current` | 200 [`WorkSession`]; 404 when none is open |
//! | GET  | `/sessions/{id}` | 200 [`WorkSession`]; participants only |
//! | POST | `/sessions/{id}/timer` | 200 [`WorkSession`]; body [`Command`] (W2, W3) |
//! | POST | `/sessions/{id}/close` | 200 [`WorkSession`] (W1) |

use axum::extract::{FromRef, Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::model::{OpenRequest, WorkSession};
use crate::service::WorkSessions;
use crate::timer::Command;

#[derive(Clone)]
pub struct SessionsHttpState {
    pub sessions: WorkSessions,
    pub identity: IdentityService,
}

impl FromRef<SessionsHttpState> for IdentityService {
    fn from_ref(state: &SessionsHttpState) -> Self {
        state.identity.clone()
    }
}

pub fn router(state: SessionsHttpState) -> Router {
    Router::new()
        .route("/sessions", post(open))
        .route("/sessions/current", get(current))
        .route("/sessions/{id}", get(read))
        .route("/sessions/{id}/timer", post(command))
        .route("/sessions/{id}/close", post(close))
        .with_state(state)
}

/// The body is optional: an empty one opens with the default pomodoro lengths.
async fn open(
    State(state): State<SessionsHttpState>,
    Authenticated(auth): Authenticated,
    body: axum::body::Bytes,
) -> Result<Json<WorkSession>, AppError> {
    let request: OpenRequest = if body.is_empty() {
        OpenRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|_| {
            AppError::from(kernel::ValidationErrors::from(kernel::FieldError::new(
                "pomodoro",
                kernel::FieldErrorCode::Invalid,
                "not a valid request body",
            )))
        })?
    };
    state
        .sessions
        .open(auth.user_id.as_uuid(), request)
        .await
        .map(Json)
}

async fn current(
    State(state): State<SessionsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<WorkSession>, AppError> {
    state
        .sessions
        .current(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn read(
    State(state): State<SessionsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkSession>, AppError> {
    state
        .sessions
        .get(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn command(
    State(state): State<SessionsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(command): Json<Command>,
) -> Result<Json<WorkSession>, AppError> {
    state
        .sessions
        .command(auth.user_id.as_uuid(), id, command)
        .await
        .map(Json)
}

async fn close(
    State(state): State<SessionsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkSession>, AppError> {
    state
        .sessions
        .close(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}
