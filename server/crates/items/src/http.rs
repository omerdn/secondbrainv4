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
//! | GET    | `/items/{id}/tags` | 200 `Tag[]`: the caller's own tags on the item (E2) |
//! | PUT    | `/items/{id}/tags/{tag_id}` | 204; attach the caller's tag; VIEW |
//! | DELETE | `/items/{id}/tags/{tag_id}` | 204; detach the caller's tag; VIEW |
//! | POST   | `/tags` | 201 [`Tag`]; body [`CreateTagRequest`]; 409 for a taken name |
//! | GET    | `/tags` | 200 `Tag[]`: the tags the caller can see (E4) |
//! | PATCH  | `/tags/{id}` | 200 [`Tag`]; body [`UpdateTagRequest`]; 404 if not theirs |
//! | DELETE | `/tags/{id}` | 204; 404 if not theirs |
//! | GET    | `/tags/{id}/items?state=…` | 200 `Item[]`: accessible items with the tag (E4) |
//! | GET    | `/items/{id}/links` | 200 `ItemLink[]`; hidden ends are left out (V9) |
//! | PUT    | `/items/{id}/links/{target_id}` | 200 [`ItemLink`]; LINK + target VIEW (V4) |
//! | DELETE | `/items/{id}/links/{target_id}` | 204; LINK + target VIEW |

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::links::ItemLink;
use crate::model::{CreateItemRequest, Item, ListQuery, UpdateItemRequest};
use crate::service::ItemsService;
use crate::tags::{CreateTagRequest, Tag, UpdateTagRequest};

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
        .route("/items/{id}/tags", get(item_tags))
        .route(
            "/items/{id}/tags/{tag_id}",
            put(attach_tag).delete(detach_tag),
        )
        .route("/tags", post(create_tag).get(list_tags))
        .route("/tags/{id}", patch(update_tag).delete(delete_tag))
        .route("/tags/{id}/items", get(items_by_tag))
        .route("/items/{id}/links", get(item_links))
        .route(
            "/items/{id}/links/{target_id}",
            put(link_item).delete(unlink_item),
        )
        .with_state(state)
}

async fn item_links(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ItemLink>>, AppError> {
    state
        .items
        .links(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn link_item(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, target_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ItemLink>, AppError> {
    state
        .items
        .link(auth.user_id.as_uuid(), id, target_id)
        .await
        .map(Json)
}

async fn unlink_item(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, target_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    state
        .items
        .unlink(auth.user_id.as_uuid(), id, target_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn item_tags(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Tag>>, AppError> {
    state
        .items
        .item_tags(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn attach_tag(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    state
        .items
        .attach_tag(auth.user_id.as_uuid(), id, tag_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach_tag(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    state
        .items
        .detach_tag(auth.user_id.as_uuid(), id, tag_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_tag(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateTagRequest>,
) -> Result<(StatusCode, Json<Tag>), AppError> {
    let new = request.validate()?;
    let tag = state.items.create_tag(auth.user_id.as_uuid(), new).await?;
    Ok((StatusCode::CREATED, Json(tag)))
}

async fn list_tags(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<Tag>>, AppError> {
    state
        .items
        .list_tags(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn update_tag(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateTagRequest>,
) -> Result<Json<Tag>, AppError> {
    let changes = request.validate()?;
    state
        .items
        .update_tag(auth.user_id.as_uuid(), id, changes)
        .await
        .map(Json)
}

async fn delete_tag(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state.items.delete_tag(auth.user_id.as_uuid(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn items_by_tag(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Item>>, AppError> {
    state
        .items
        .items_by_tag(auth.user_id.as_uuid(), id, query.state.unwrap_or_default())
        .await
        .map(Json)
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
