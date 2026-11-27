//! Notification center endpoints (T2.8.1). Mount inside `idempotency::protect`. Every route
//! reads or changes only the caller's own notifications and preferences.
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | GET  | `/notifications?before=` | 200 [`NotificationList`] |
//! | POST | `/notifications/{id}/read` | 200 [`Notification`]; 404 when not the caller's |
//! | POST | `/notifications/read-all` | 204 |
//! | GET  | `/notifications/preferences` | 200 [`NotificationPreference`]`[]` (BL3) |
//! | PUT  | `/notifications/preferences/{kind}` | 200 [`NotificationPreference`]; body [`UpdatePreferenceRequest`] |
//! | GET  | `/notifications/push/key` | 200 [`PushPublicKey`] (T4.2.4) |
//! | PUT  | `/notifications/push/subscriptions` | 204; body [`PushSubscriptionRequest`]; 422 for a bad endpoint or keys |
//! | POST | `/notifications/push/subscriptions/remove` | 204; body [`RemovePushSubscriptionRequest`] |

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::model::{
    Notification, NotificationKind, NotificationList, NotificationPreference, NotificationsQuery,
    UpdatePreferenceRequest,
};
use crate::push::{PushPublicKey, PushSubscriptionRequest, RemovePushSubscriptionRequest};
use crate::service::Notifications;

#[derive(Clone)]
pub struct NotificationsHttpState {
    pub notifications: Notifications,
    pub identity: IdentityService,
}

impl FromRef<NotificationsHttpState> for IdentityService {
    fn from_ref(state: &NotificationsHttpState) -> Self {
        state.identity.clone()
    }
}

pub fn router(state: NotificationsHttpState) -> Router {
    Router::new()
        .route("/notifications", get(list))
        .route("/notifications/read-all", post(read_all))
        .route("/notifications/preferences", get(preferences))
        .route("/notifications/preferences/{kind}", put(set_preference))
        .route("/notifications/push/key", get(push_key))
        .route("/notifications/push/subscriptions", put(subscribe_push))
        .route(
            "/notifications/push/subscriptions/remove",
            post(unsubscribe_push),
        )
        .route("/notifications/{id}/read", post(read))
        .with_state(state)
}

async fn list(
    State(state): State<NotificationsHttpState>,
    Authenticated(auth): Authenticated,
    Query(query): Query<NotificationsQuery>,
) -> Result<Json<NotificationList>, AppError> {
    state
        .notifications
        .list(auth.user_id.as_uuid(), query.before)
        .await
        .map(Json)
}

async fn read(
    State(state): State<NotificationsHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Notification>, AppError> {
    state
        .notifications
        .mark_read(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn read_all(
    State(state): State<NotificationsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<StatusCode, AppError> {
    state
        .notifications
        .mark_all_read(auth.user_id.as_uuid())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn preferences(
    State(state): State<NotificationsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<NotificationPreference>>, AppError> {
    state
        .notifications
        .preferences(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn set_preference(
    State(state): State<NotificationsHttpState>,
    Authenticated(auth): Authenticated,
    Path(kind): Path<String>,
    Json(request): Json<UpdatePreferenceRequest>,
) -> Result<Json<NotificationPreference>, AppError> {
    let kind = NotificationKind::parse(&kind).ok_or(AppError::NotFound)?;
    state
        .notifications
        .set_preference(auth.user_id.as_uuid(), kind, request.channels)
        .await
        .map(Json)
}

async fn push_key(
    State(state): State<NotificationsHttpState>,
    Authenticated(_): Authenticated,
) -> Json<PushPublicKey> {
    Json(state.notifications.push_public_key())
}

async fn subscribe_push(
    State(state): State<NotificationsHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<PushSubscriptionRequest>,
) -> Result<StatusCode, AppError> {
    state
        .notifications
        .subscribe_push(auth.user_id.as_uuid(), &request)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unsubscribe_push(
    State(state): State<NotificationsHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<RemovePushSubscriptionRequest>,
) -> Result<StatusCode, AppError> {
    state
        .notifications
        .unsubscribe_push(auth.user_id.as_uuid(), &request.endpoint)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
