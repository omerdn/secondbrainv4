//! Friendship endpoints (T3.1.1). Mount inside `idempotency::protect`. Every route acts for the
//! caller only.
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | POST   | `/friend-requests` | 201 [`OutgoingRequest`]; body [`SendFriendRequest`] (AK1, AK2) |
//! | GET    | `/friend-requests/incoming` | 200 [`IncomingRequest`]`[]` |
//! | GET    | `/friend-requests/outgoing` | 200 [`OutgoingRequest`]`[]` |
//! | POST   | `/friend-requests/{id}/accept` | 200 [`Friend`] |
//! | POST   | `/friend-requests/{id}/decline` | 204 |
//! | DELETE | `/friend-requests/{id}` | 204: the caller's own request |
//! | GET    | `/friends` | 200 [`Friend`]`[]` |
//! | DELETE | `/friends/{user_id}` | 204 |
//! | GET    | `/blocks` | 200 [`BlockedUser`]`[]` |
//! | PUT    | `/blocks/{user_id}` | 204 |
//! | DELETE | `/blocks/{user_id}` | 204 |

use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::model::{BlockedUser, Friend, IncomingRequest, OutgoingRequest, SendFriendRequest};
use crate::service::Friendships;

#[derive(Clone)]
pub struct SocialHttpState {
    pub friendships: Friendships,
    pub identity: IdentityService,
}

impl FromRef<SocialHttpState> for IdentityService {
    fn from_ref(state: &SocialHttpState) -> Self {
        state.identity.clone()
    }
}

pub fn router(state: SocialHttpState) -> Router {
    Router::new()
        .route("/friend-requests", post(send))
        .route("/friend-requests/incoming", get(incoming))
        .route("/friend-requests/outgoing", get(outgoing))
        .route("/friend-requests/{id}", delete(cancel))
        .route("/friend-requests/{id}/accept", post(accept))
        .route("/friend-requests/{id}/decline", post(decline))
        .route("/friends", get(friends))
        .route("/friends/{user_id}", delete(unfriend))
        .route("/blocks", get(blocks))
        .route("/blocks/{user_id}", put(block).delete(unblock))
        .with_state(state)
}

async fn send(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<SendFriendRequest>,
) -> Result<(StatusCode, Json<OutgoingRequest>), AppError> {
    state
        .friendships
        .send(auth.user_id.as_uuid(), &request.email)
        .await
        .map(|sent| (StatusCode::CREATED, Json(sent)))
}

async fn incoming(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<IncomingRequest>>, AppError> {
    state
        .friendships
        .incoming(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn outgoing(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<OutgoingRequest>>, AppError> {
    state
        .friendships
        .outgoing(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn accept(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Friend>, AppError> {
    state
        .friendships
        .accept(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn decline(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .friendships
        .decline(auth.user_id.as_uuid(), id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn cancel(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state.friendships.cancel(auth.user_id.as_uuid(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn friends(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<Friend>>, AppError> {
    state
        .friendships
        .friends(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn unfriend(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .friendships
        .unfriend(auth.user_id.as_uuid(), user_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn blocks(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<BlockedUser>>, AppError> {
    state
        .friendships
        .blocked(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn block(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .friendships
        .block(auth.user_id.as_uuid(), user_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unblock(
    State(state): State<SocialHttpState>,
    Authenticated(auth): Authenticated,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .friendships
        .unblock(auth.user_id.as_uuid(), user_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
