//! Routine endpoints (T2.2.1). Mount inside `idempotency::protect` (every mutation carries an
//! idempotency key, ADR-0003). Authorization goes through `access::PermissionService` in
//! [`RoutinesService`].
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | POST  | `/routines` | 201 [`RoutineView`]; creates the item and its first plan version |
//! | GET   | `/routines/{id}` | 200 [`RoutineView`]; VIEW |
//! | PATCH | `/routines/{id}` | 200 [`RoutineView`]; time zone, end date, reminders; EDIT |
//! | POST  | `/routines/{id}/plan` | 200 [`RoutineView`]; opens a plan version (R11); EDIT |

use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::routing::{patch, post};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::model::{ChangePlanRequest, CreateRoutineRequest, RoutineView, UpdateRoutineRequest};
use crate::service::RoutinesService;

/// State of the routine routes.
#[derive(Clone)]
pub struct RoutinesHttpState {
    pub routines: RoutinesService,
    pub identity: IdentityService,
}

impl FromRef<RoutinesHttpState> for IdentityService {
    fn from_ref(state: &RoutinesHttpState) -> Self {
        state.identity.clone()
    }
}

/// The routine routes.
pub fn router(state: RoutinesHttpState) -> Router {
    Router::new()
        .route("/routines", post(create_routine))
        .route("/routines/{id}", patch(update_routine).get(read_routine))
        .route("/routines/{id}/plan", post(change_plan))
        .with_state(state)
}

async fn create_routine(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateRoutineRequest>,
) -> Result<(StatusCode, Json<RoutineView>), AppError> {
    let routine = state
        .routines
        .create(auth.user_id.as_uuid(), request)
        .await?;
    Ok((StatusCode::CREATED, Json(routine)))
}

async fn read_routine(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<RoutineView>, AppError> {
    state
        .routines
        .routine(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn update_routine(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateRoutineRequest>,
) -> Result<Json<RoutineView>, AppError> {
    state
        .routines
        .update(auth.user_id.as_uuid(), id, request)
        .await
        .map(Json)
}

async fn change_plan(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<ChangePlanRequest>,
) -> Result<Json<RoutineView>, AppError> {
    state
        .routines
        .change_plan(auth.user_id.as_uuid(), id, request)
        .await
        .map(Json)
}
