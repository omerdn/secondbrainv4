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
//! | POST   | `/teams/{team_id}/items` | 201 [`Item`]; body [`CreateTeamItemRequest`]; 404 outside the team, 403 without the role's right (V12, TK4) |
//! | PUT    | `/items/{id}/visibility` | 200 [`Item`]; body [`SetTeamVisibilityRequest`]; MANAGE; 422 for a personal item (V12, V8) |
//! | POST   | `/items/{id}/archive`, `/items/{id}/unarchive` | 200 [`Item`]; MANAGE |
//! | DELETE | `/items/{id}` | 200 [`Item`] moved to the trash; owner only (V6) |
//! | POST   | `/items/{id}/restore` | 200 [`Item`]; owner only |
//! | GET    | `/items/{id}/tags` | 200 `Tag[]`: the caller's own and team tags on the item (E2, E3) |
//! | PUT    | `/items/{id}/tags/{tag_id}` | 204; attach a visible tag; VIEW, EDIT for a team tag |
//! | DELETE | `/items/{id}/tags/{tag_id}` | 204; detach a visible tag; as PUT |
//! | POST   | `/tags` | 201 [`Tag`]; body [`CreateTagRequest`]; 409 for a taken name |
//! | POST   | `/teams/{team_id}/tags` | 201 [`Tag`]; body [`CreateTagRequest`]; 404 outside the team, 403 without "manage tags" (E3) |
//! | GET    | `/tags` | 200 `Tag[]`: own and team tags the caller can see (E4) |
//! | PATCH  | `/tags/{id}` | 200 [`Tag`]; body [`UpdateTagRequest`]; 404 if not visible, 403 for a team tag without "manage tags" |
//! | DELETE | `/tags/{id}` | 204; as PATCH |
//! | GET    | `/tags/{id}/items?state=…` | 200 `Item[]`: accessible items with the tag (E4) |
//! | GET    | `/items/{id}/links` | 200 `ItemLink[]`; hidden ends are left out (V9) |
//! | PUT    | `/items/{id}/links/{target_id}` | 200 [`ItemLink`]; LINK + target VIEW (V4) |
//! | DELETE | `/items/{id}/links/{target_id}` | 204; LINK + target VIEW |
//! | POST   | `/items/{id}/invitations` | 201 [`Invitation`]; SHARE + V1 |
//! | GET    | `/items/{id}/members` | 200 [`ItemMembers`]; VIEW; pending invitations for SHARE (N2) |
//! | PUT    | `/items/{id}/owner` | 204; body [`TransferOwnershipRequest`]; the owner hands a personal item to a member (HS2.1); 404 if not the owner, 422 for a page or a non-member |
//! | GET    | `/account/shared-items` | 200 [`OwnedSharedItem`]`[]`: shared items the caller owns and who would inherit each (HS2) |
//! | DELETE | `/items/{id}/members/{user_id}` | 204; SHARE, or oneself (V5) |
//! | GET    | `/invitations` | 200 `Invitation[]`: the caller's pending invitations |
//! | POST   | `/invitations/{id}/accept`, `/invitations/{id}/decline` | 200 [`Invitation`] |
//! | GET    | `/items/{id}/activity?limit=&cursor=` | 200 `Page<Activity>`; VIEW (A2) |
//! | GET    | `/activity?limit=&cursor=` | 200 `Page<Activity>` across visible items (A2) |
//! | PUT    | `/activity/{id}/reaction` | 200 `ActivityReaction[]`; a friend's completion (AK4) |
//! | DELETE | `/activity/{id}/reaction` | 200 `ActivityReaction[]`; takes the caller's back |
//! | GET    | `/items/filter?filter=<json>&limit=&cursor=` | 200 `Page<Item>` (F1) |
//! | POST   | `/saved-views` | 201 [`SavedView`]; 409 for a taken name (F2) |
//! | GET    | `/saved-views` | 200 `SavedView[]`: pinned first |
//! | PATCH  | `/saved-views/{id}` | 200 [`SavedView`]; rename, refilter, pin, reorder |
//! | DELETE | `/saved-views/{id}` | 204; 404 if not theirs |
//! | GET    | `/saved-views/{id}/items?limit=&cursor=` | 200 `Page<Item>` |

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::ValidationErrors;
use kernel::{AppError, FieldError, FieldErrorCode, Page, PageParams, PageRequest};
use serde::Deserialize;
use uuid::Uuid;

use crate::activity::Activity;
use crate::filters::ItemFilter;
use crate::invitations::{CreateInvitationRequest, Invitation};
use crate::links::ItemLink;
use crate::model::{CreateItemRequest, Item, ListQuery, UpdateItemRequest};
use crate::service::ItemsService;
use crate::tags::{CreateTagRequest, Tag, UpdateTagRequest};
use crate::team_items::{CreateTeamItemRequest, SetTeamVisibilityRequest};
use crate::views::{CreateSavedViewRequest, SavedView, UpdateSavedViewRequest};

/// `GET /items/filter` query. The filter is JSON because it holds lists.
#[derive(Debug, Default, Deserialize)]
struct FilterQuery {
    filter: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
}

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
        .route("/items/{id}/visibility", put(set_visibility))
        .route("/teams/{team_id}/items", post(create_team_item))
        .route("/items/{id}/archive", post(archive))
        .route("/items/{id}/unarchive", post(unarchive))
        .route("/items/{id}/restore", post(restore))
        .route("/items/{id}/tags", get(item_tags))
        .route(
            "/items/{id}/tags/{tag_id}",
            put(attach_tag).delete(detach_tag),
        )
        .route("/tags", post(create_tag).get(list_tags))
        .route("/teams/{team_id}/tags", post(create_team_tag))
        .route("/tags/{id}", patch(update_tag).delete(delete_tag))
        .route("/tags/{id}/items", get(items_by_tag))
        .route("/items/{id}/links", get(item_links))
        .route(
            "/items/{id}/links/{target_id}",
            put(link_item).delete(unlink_item),
        )
        .route("/items/{id}/invitations", post(invite))
        .route("/items/{id}/members", get(members))
        .route("/items/{id}/owner", put(transfer_ownership))
        .route("/account/shared-items", get(owned_shared))
        .route(
            "/items/{id}/members/{user_id}",
            axum::routing::delete(remove_member),
        )
        .route("/invitations", get(pending_invitations))
        .route("/invitations/{id}/accept", post(accept_invitation))
        .route("/invitations/{id}/decline", post(decline_invitation))
        .route("/items/{id}/activity", get(item_activity))
        .route("/activity", get(activity_feed))
        .route("/activity/{id}/reaction", put(react).delete(unreact))
        .route("/items/filter", get(filter_items))
        .route("/saved-views", post(create_view).get(list_views))
        .route("/saved-views/{id}", patch(update_view).delete(delete_view))
        .route("/saved-views/{id}/items", get(view_items))
        .with_state(state)
}

async fn filter_items(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Query(query): Query<FilterQuery>,
) -> Result<Json<Page<Item>>, AppError> {
    let filter: ItemFilter = match query.filter.as_deref() {
        None | Some("") => ItemFilter::default(),
        Some(raw) => serde_json::from_str(raw).map_err(|_| {
            AppError::from(ValidationErrors::from(FieldError::new(
                "filter",
                FieldErrorCode::Invalid,
                "invalid filter",
            )))
        })?,
    };
    let page = PageRequest::from_params(PageParams {
        limit: query.limit,
        cursor: query.cursor,
    })?;
    state
        .items
        .filter_checked(auth.user_id.as_uuid(), &filter, &page)
        .await
        .map(Json)
}

async fn create_view(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateSavedViewRequest>,
) -> Result<(StatusCode, Json<SavedView>), AppError> {
    let new = request.validate()?;
    let view = state.items.create_view(auth.user_id.as_uuid(), new).await?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn list_views(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<SavedView>>, AppError> {
    state
        .items
        .list_views(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn update_view(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateSavedViewRequest>,
) -> Result<Json<SavedView>, AppError> {
    let changes = request.validate()?;
    state
        .items
        .update_view(auth.user_id.as_uuid(), id, changes)
        .await
        .map(Json)
}

async fn delete_view(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state.items.delete_view(auth.user_id.as_uuid(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn view_items(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(params): Query<PageParams>,
) -> Result<Json<Page<Item>>, AppError> {
    let page = PageRequest::from_params(params)?;
    state
        .items
        .view_items(auth.user_id.as_uuid(), id, &page)
        .await
        .map(Json)
}

async fn item_activity(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(params): Query<PageParams>,
) -> Result<Json<Page<Activity>>, AppError> {
    let page = PageRequest::from_params(params)?;
    state
        .items
        .item_activity(auth.user_id.as_uuid(), id, &page)
        .await
        .map(Json)
}

async fn react(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<crate::reactions::ReactRequest>,
) -> Result<Json<Vec<crate::reactions::ReactionCount>>, AppError> {
    state
        .items
        .react(auth.user_id.as_uuid(), id, request)
        .await
        .map(Json)
}

async fn unreact(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::reactions::ReactionCount>>, AppError> {
    state
        .items
        .unreact(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn activity_feed(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Query(params): Query<PageParams>,
) -> Result<Json<Page<Activity>>, AppError> {
    let page = PageRequest::from_params(params)?;
    state
        .items
        .activity_feed(auth.user_id.as_uuid(), &page)
        .await
        .map(Json)
}

async fn invite(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<CreateInvitationRequest>,
) -> Result<(StatusCode, Json<Invitation>), AppError> {
    let invitation = state
        .items
        .invite(auth.user_id.as_uuid(), id, request)
        .await?;
    Ok((StatusCode::CREATED, Json(invitation)))
}

async fn members(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::members::ItemMembers>, AppError> {
    let identity = state.identity.clone();
    state
        .items
        .members(auth.user_id.as_uuid(), id, |ids| async move {
            identity.display_names(&ids).await
        })
        .await
        .map(Json)
}

async fn transfer_ownership(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<crate::deletion::TransferOwnershipRequest>,
) -> Result<StatusCode, AppError> {
    state
        .items
        .transfer_ownership(auth.user_id.as_uuid(), id, request.user_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn owned_shared(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<crate::deletion::OwnedSharedItem>>, AppError> {
    let identity = state.identity.clone();
    state
        .items
        .owned_shared(auth.user_id.as_uuid(), |ids| async move {
            identity.display_names(&ids).await
        })
        .await
        .map(Json)
}

async fn remove_member(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    state
        .items
        .remove_member(auth.user_id.as_uuid(), id, user_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn pending_invitations(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<Invitation>>, AppError> {
    state
        .items
        .pending_invitations(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn accept_invitation(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Invitation>, AppError> {
    state
        .items
        .accept_invitation(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn decline_invitation(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Invitation>, AppError> {
    state
        .items
        .decline_invitation(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
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

async fn create_team_tag(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(team): Path<Uuid>,
    Json(request): Json<CreateTagRequest>,
) -> Result<(StatusCode, Json<Tag>), AppError> {
    let new = request.validate()?;
    let tag = state
        .items
        .create_team_tag(auth.user_id.as_uuid(), team, new)
        .await?;
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

async fn create_team_item(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(team): Path<Uuid>,
    Json(request): Json<CreateTeamItemRequest>,
) -> Result<(StatusCode, Json<Item>), AppError> {
    let new = request.validate()?;
    let item = state
        .items
        .create_team_item(auth.user_id.as_uuid(), team, new)
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn set_visibility(
    State(state): State<ItemsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<SetTeamVisibilityRequest>,
) -> Result<Json<Item>, AppError> {
    state
        .items
        .set_team_visibility(auth.user_id.as_uuid(), id, request.visibility)
        .await
        .map(Json)
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
