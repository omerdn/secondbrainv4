//! Item endpoints (T1.2.1). Mount inside `idempotency::protect` (every mutation carries an
//! idempotency key, ADR-0003). Authorization goes through `access::PermissionService` in
//! [`ItemsService`].
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | POST   | `/items` | 201 [`Item`]; body [`CreateItemRequest`] |
//! | GET    | `/items?state=active\|archived\|trash` | 200 `Item[]` |
//! | GET    | `/items/{id}` | 200 [`Item`]; 404 if not visible (V9) |
//! | PATCH  | `/items/{id}` | 200 [`Item`]; body [`UpdateItemRequest`]; EDIT |
//! | POST   | `/items/{id}/archive`, `/items/{id}/unarchive` | 200 [`Item`]; MANAGE |
//! | DELETE | `/items/{id}` | 200 [`Item`] moved to the trash; owner only (V6) |
//! | POST   | `/items/{id}/restore` | 200 [`Item`]; owner only |

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::model::{CreateItemRequest, Item, ListQuery, UpdateItemRequest};
use crate::service::ItemsService;

/// State of the item routes.
#[derive(Clone)]
pub struct ItemsHttpState {
    pub items: ItemsService,
    pub identity: IdentityService,
}

impl FromRef<ItemsHttpState> for IdentityService {
    fn from_ref(state: &ItemsHttpState) -> Self {
        state.identity.clone()
    }
}

/// The item routes.
pub fn router(state: ItemsHttpState) -> Router {
    Router::new()
        .route("/items", post(create).get(list))
        .route("/items/{id}", get(read).patch(update).delete(trash))
        .route("/items/{id}/archive", post(archive))
        .route("/items/{id}/unarchive", post(unarchive))
        .route("/items/{id}/restore", post(restore))
        .with_state(state)
}

async fn create(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateItemRequest>,
) -> Result<(StatusCode, Json<Item>), AppError> {
    let new = request.validate()?;
    let item = state.items.create(auth.user_id.as_uuid(), new).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn list(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Item>>, AppError> {
    let items = state
        .items
        .list(auth.user_id.as_uuid(), query.state.unwrap_or_default())
        .await?;
    Ok(Json(items))
}

async fn read(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    state.items.get(auth.user_id.as_uuid(), id).await.map(Json)
}

async fn update(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateItemRequest>,
) -> Result<Json<Item>, AppError> {
    let changes = request.validate()?;
    state
        .items
        .update(auth.user_id.as_uuid(), id, changes)
        .await
        .map(Json)
}

async fn archive(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    state
        .items
        .set_archived(auth.user_id.as_uuid(), id, true)
        .await
        .map(Json)
}

async fn unarchive(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    state
        .items
        .set_archived(auth.user_id.as_uuid(), id, false)
        .await
        .map(Json)
}

async fn trash(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    state
        .items
        .set_trashed(auth.user_id.as_uuid(), id, true)
        .await
        .map(Json)
}

async fn restore(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    state
        .items
        .set_trashed(auth.user_id.as_uuid(), id, false)
        .await
        .map(Json)
}
