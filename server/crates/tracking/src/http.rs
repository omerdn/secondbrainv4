//! Tracker entry endpoints (T1.3.4). Mount inside `idempotency::protect` (every mutation carries
//! an idempotency key, ADR-0003). Authorization goes through `access::PermissionService` in
//! [`TrackerEntries`].
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | POST   | `/trackers` | 201 [`TrackerView`]; creates the item and its definition |
//! | GET    | `/trackers/{id}` | 200 [`TrackerView`]; VIEW |
//! | PATCH  | `/trackers/{id}` | 200 [`TrackerView`]; EDIT |
//! | GET    | `/trackers/{id}/summary` | 200 [`TrackerSummary`]; current period, VIEW |
//! | POST   | `/trackers/{id}/entries` | 201 [`EntryView`]; EDIT |
//! | GET    | `/trackers/{id}/entries?from=&to=&limit=&cursor=` | 200 `Page<TrackerEntry>`; VIEW |
//! | DELETE | `/trackers/{id}/entries/{entry_id}` | 204; own: EDIT, someone else's: MANAGE |

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::{AppError, Page, PageParams, PageRequest};
use uuid::Uuid;

use crate::definitions::{CreateTrackerRequest, TrackerView, UpdateTrackerRequest};
use crate::service::{CreateEntryRequest, EntryView, ListEntriesQuery, TrackerEntries};
use crate::summary::TrackerSummary;

/// State of the tracker routes.
#[derive(Clone)]
pub struct TrackingHttpState {
    pub entries: TrackerEntries,
    pub identity: IdentityService,
}

impl FromRef<TrackingHttpState> for IdentityService {
    fn from_ref(state: &TrackingHttpState) -> Self {
        state.identity.clone()
    }
}

/// The tracker routes.
pub fn router(state: TrackingHttpState) -> Router {
    Router::new()
        .route("/trackers", post(create_tracker))
        .route("/trackers/{id}", get(read_tracker).patch(update_tracker))
        .route("/trackers/{id}/summary", get(read_summary))
        .route("/trackers/{id}/entries", post(add_entry).get(list_entries))
        .route("/trackers/{id}/entries/{entry_id}", delete(delete_entry))
        .with_state(state)
}

async fn create_tracker(
    State(state): State<TrackingHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateTrackerRequest>,
) -> Result<(StatusCode, Json<TrackerView>), AppError> {
    let tracker = state
        .entries
        .create_tracker(auth.user_id.as_uuid(), request)
        .await?;
    Ok((StatusCode::CREATED, Json(tracker)))
}

async fn read_tracker(
    State(state): State<TrackingHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<TrackerView>, AppError> {
    state
        .entries
        .tracker(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn update_tracker(
    State(state): State<TrackingHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateTrackerRequest>,
) -> Result<Json<TrackerView>, AppError> {
    state
        .entries
        .update_tracker(auth.user_id.as_uuid(), id, request)
        .await
        .map(Json)
}

async fn read_summary(
    State(state): State<TrackingHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<TrackerSummary>, AppError> {
    state
        .entries
        .summary(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn add_entry(
    State(state): State<TrackingHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<CreateEntryRequest>,
) -> Result<(StatusCode, Json<EntryView>), AppError> {
    let entry = state
        .entries
        .add(auth.user_id.as_uuid(), id, request)
        .await?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn list_entries(
    State(state): State<TrackingHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(query): Query<ListEntriesQuery>,
) -> Result<Json<Page<EntryView>>, AppError> {
    let page = PageRequest::from_params(PageParams {
        limit: query.limit,
        cursor: query.cursor,
    })?;
    state
        .entries
        .list(auth.user_id.as_uuid(), id, (query.from, query.to), &page)
        .await
        .map(Json)
}

async fn delete_entry(
    State(state): State<TrackingHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, entry_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    state
        .entries
        .delete(auth.user_id.as_uuid(), id, entry_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
