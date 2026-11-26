//! Goal endpoints (T2.3.1). Mount inside `idempotency::protect` (every mutation carries an
//! idempotency key, ADR-0003). Authorization goes through `access::PermissionService` in
//! [`GoalsService`].
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | POST  | `/goals` | 201 [`Goal`]; both dates required (D1) |
//! | GET   | `/goals/{id}` | 200 [`Goal`]; VIEW; only visible links (V9) |
//! | PATCH | `/goals/{id}` | 200 [`Goal`]; dates; EDIT |
//! | GET   | `/goals/{id}/progress?today=` | 200 [`GoalProgress`]; D3-D6; VIEW |
//! | PUT   | `/goals/{id}/links/{item_id}` | 200 [`Goal`]; link or reweigh (D2, D3); EDIT + LINK |
//! | DELETE | `/goals/{id}/links/{item_id}` | 200 [`Goal`]; EDIT + LINK |

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::model::{CreateGoalRequest, Goal, LinkGoalRequest, UpdateGoalRequest};
use crate::progress::{GoalProgress, GoalProgressQuery};
use crate::service::GoalsService;

/// State of the goal routes.
#[derive(Clone)]
pub struct GoalsHttpState {
    pub goals: GoalsService,
    pub identity: IdentityService,
}

impl FromRef<GoalsHttpState> for IdentityService {
    fn from_ref(state: &GoalsHttpState) -> Self {
        state.identity.clone()
    }
}

/// The goal routes.
pub fn router(state: GoalsHttpState) -> Router {
    Router::new()
        .route("/goals", post(create_goal))
        .route("/goals/{id}", patch(update_goal).get(read_goal))
        .route("/goals/{id}/progress", get(read_progress))
        .route("/goals/{id}/links/{item_id}", put(link).delete(unlink))
        .with_state(state)
}

async fn create_goal(
    State(state): State<GoalsHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateGoalRequest>,
) -> Result<(StatusCode, Json<Goal>), AppError> {
    let goal = state.goals.create(auth.user_id.as_uuid(), request).await?;
    Ok((StatusCode::CREATED, Json(goal)))
}

async fn read_goal(
    State(state): State<GoalsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Goal>, AppError> {
    state.goals.goal(auth.user_id.as_uuid(), id).await.map(Json)
}

async fn update_goal(
    State(state): State<GoalsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateGoalRequest>,
) -> Result<Json<Goal>, AppError> {
    state
        .goals
        .update(auth.user_id.as_uuid(), id, request)
        .await
        .map(Json)
}

async fn link(
    State(state): State<GoalsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, item_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<LinkGoalRequest>,
) -> Result<Json<Goal>, AppError> {
    state
        .goals
        .link(auth.user_id.as_uuid(), id, item_id, request)
        .await
        .map(Json)
}

async fn unlink(
    State(state): State<GoalsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, item_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Goal>, AppError> {
    state
        .goals
        .unlink(auth.user_id.as_uuid(), id, item_id)
        .await
        .map(Json)
}

async fn read_progress(
    State(state): State<GoalsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(query): Query<GoalProgressQuery>,
) -> Result<Json<GoalProgress>, AppError> {
    state
        .goals
        .progress(auth.user_id.as_uuid(), id, query)
        .await
        .map(Json)
}
