//! Team endpoints (T5.1.1). Mount inside `idempotency::protect`.
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | POST | `/teams` | 201 [`Team`]; body [`CreateTeamRequest`]; the caller is admin (TK1) |
//! | GET  | `/teams` | 200 [`Team`]`[]`: the caller's teams |
//! | GET  | `/teams/{id}` | 200 [`TeamDetail`]; members only, else 404 |
//! | POST | `/teams/{id}/invitations` | 204; body [`InviteToTeamRequest`]; admin or manager (TK2) |
//! | POST | `/teams/{id}/invitations/by-email` | 204; body [`InviteToTeamByEmailRequest`]; the same answer whether the address has an account (T5.2.6) |
//! | GET  | `/team-invitations` | 200 [`TeamInvitation`]`[]`: waiting for the caller |
//! | POST | `/teams/{id}/join` | 200 [`Team`]: accepts the caller's invitation |
//! | POST | `/teams/{id}/decline` | 204: declines it |
//! | GET  | `/teams/{id}/matrix` | 200 [`TeamMatrix`]; members only (TK3) |
//! | PUT  | `/teams/{id}/matrix` | 200 [`TeamMatrix`]; admins; access recomputed (V8) |
//! | PUT  | `/teams/{id}/members/{user_id}/role` | 204; body [`SetRoleRequest`]; admins (TK3, V8) |
//! | POST | `/teams/{id}/leave` | 204; the caller leaves; access to team items drops at once (TK7, V10); 422 for the last admin |
//! | DELETE | `/teams/{id}/members/{user_id}` | 204; "invite members", admins for admins and managers (TK5, TK7) |

use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::matrix::TeamMatrix;
use crate::model::{
    CreateTeamRequest, InviteToTeamByEmailRequest, InviteToTeamRequest, SetRoleRequest, Team,
    TeamDetail, TeamInvitation,
};
use crate::service::TeamsService;

#[derive(Clone)]
pub struct TeamsHttpState {
    pub teams: TeamsService,
    pub identity: IdentityService,
}

impl FromRef<TeamsHttpState> for IdentityService {
    fn from_ref(state: &TeamsHttpState) -> Self {
        state.identity.clone()
    }
}

pub fn router(state: TeamsHttpState) -> Router {
    Router::new()
        .route("/teams", post(create).get(list))
        .route("/teams/{id}", get(detail))
        .route("/teams/{id}/invitations", post(invite))
        .route("/teams/{id}/invitations/by-email", post(invite_by_email))
        .route("/teams/{id}/join", post(join))
        .route("/teams/{id}/decline", post(decline))
        .route("/team-invitations", get(invitations))
        .route("/teams/{id}/matrix", get(matrix).put(set_matrix))
        .route("/teams/{id}/members/{user_id}/role", put(set_role))
        .route("/teams/{id}/leave", post(leave))
        .route("/teams/{id}/members/{user_id}", delete(remove_member))
        .with_state(state)
}

async fn create(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateTeamRequest>,
) -> Result<(StatusCode, Json<Team>), AppError> {
    let team = state.teams.create(auth.user_id.as_uuid(), request).await?;
    Ok((StatusCode::CREATED, Json(team)))
}

async fn list(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<Team>>, AppError> {
    state.teams.teams(auth.user_id.as_uuid()).await.map(Json)
}

async fn detail(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<TeamDetail>, AppError> {
    state.teams.team(auth.user_id.as_uuid(), id).await.map(Json)
}

async fn invite(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<InviteToTeamRequest>,
) -> Result<StatusCode, AppError> {
    state
        .teams
        .invite(auth.user_id.as_uuid(), id, request)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn invitations(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<TeamInvitation>>, AppError> {
    state
        .teams
        .invitations(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn join(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Team>, AppError> {
    state
        .teams
        .accept(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn decline(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state.teams.decline(auth.user_id.as_uuid(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn matrix(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<TeamMatrix>, AppError> {
    state
        .teams
        .matrix(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn set_matrix(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(matrix): Json<TeamMatrix>,
) -> Result<Json<TeamMatrix>, AppError> {
    state
        .teams
        .set_matrix(auth.user_id.as_uuid(), id, matrix)
        .await
        .map(Json)
}

async fn set_role(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<SetRoleRequest>,
) -> Result<StatusCode, AppError> {
    state
        .teams
        .set_role(auth.user_id.as_uuid(), id, user_id, request.role)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn leave(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state.teams.leave(auth.user_id.as_uuid(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_member(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    state
        .teams
        .remove_member(auth.user_id.as_uuid(), id, user_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn invite_by_email(
    State(state): State<TeamsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<InviteToTeamByEmailRequest>,
) -> Result<StatusCode, AppError> {
    state
        .teams
        .invite_by_email(auth.user_id.as_uuid(), id, request)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
