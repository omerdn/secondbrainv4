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
//! | POST  | `/routines/{id}/completions` | 201 [`RoutineDay`]; adds to a day (R4, R7); EDIT |
//! | DELETE | `/routines/{id}/completions?date=` | 200 [`RoutineDay`]; takes back the latest; EDIT |
//! | GET   | `/routines/{id}/completions?from=&to=` | 200 `RoutineDay[]`; VIEW |
//! | GET   | `/routines/{id}/completions/people?from=&to=` | 200 [`PeopleDays`]; who did what; VIEW |
//! | GET   | `/routines/{id}/streak` | 200 [`RoutineStreak`]; R9; VIEW |
//! | GET   | `/routines/{id}/rotation?from=&to=` | 200 [`RotationView`]; who is responsible (R6); VIEW |
//! | POST  | `/routines/{id}/rotation/swaps` | 200 [`RotationView`] of the swapped range; own turn or MANAGE |

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::completions::{
    CompleteRoutineRequest, PeopleDays, RoutineDay, RoutineDaysQuery, UndoCompletionQuery,
};
use crate::model::{ChangePlanRequest, CreateRoutineRequest, RoutineView, UpdateRoutineRequest};
use crate::rotation::{RotationQuery, RotationView, SwapRequest};
use crate::service::RoutinesService;
use crate::streak::RoutineStreak;

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
        .route(
            "/routines/{id}/completions",
            post(complete).get(list_days).delete(undo),
        )
        .route("/routines/{id}/completions/people", get(list_people_days))
        .route("/routines/{id}/streak", get(read_streak))
        .route("/routines/{id}/rotation", get(read_rotation))
        .route("/routines/{id}/rotation/swaps", post(swap))
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

async fn complete(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<CompleteRoutineRequest>,
) -> Result<(StatusCode, Json<RoutineDay>), AppError> {
    let day = state
        .routines
        .complete(auth.user_id.as_uuid(), id, request)
        .await?;
    Ok((StatusCode::CREATED, Json(day)))
}

async fn undo(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(query): Query<UndoCompletionQuery>,
) -> Result<Json<RoutineDay>, AppError> {
    state
        .routines
        .undo_completion(auth.user_id.as_uuid(), id, query)
        .await
        .map(Json)
}

async fn list_days(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(query): Query<RoutineDaysQuery>,
) -> Result<Json<Vec<RoutineDay>>, AppError> {
    state
        .routines
        .days(auth.user_id.as_uuid(), id, query)
        .await
        .map(Json)
}

async fn list_people_days(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(query): Query<RoutineDaysQuery>,
) -> Result<Json<PeopleDays>, AppError> {
    state
        .routines
        .people_days(auth.user_id.as_uuid(), id, query)
        .await
        .map(Json)
}

async fn read_streak(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<RoutineStreak>, AppError> {
    state
        .routines
        .streak(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn read_rotation(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(query): Query<RotationQuery>,
) -> Result<Json<RotationView>, AppError> {
    state
        .routines
        .rotation(auth.user_id.as_uuid(), id, query)
        .await
        .map(Json)
}

async fn swap(
    State(state): State<RoutinesHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<SwapRequest>,
) -> Result<Json<RotationView>, AppError> {
    state
        .routines
        .swap(auth.user_id.as_uuid(), id, request)
        .await
        .map(Json)
}
