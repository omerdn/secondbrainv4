//! Access endpoints (T1.1.5, ARCHITECTURE §6.5). Authorization goes through
//! [`PermissionService`] only.
//!
//! | Method | Path | Auth | Response |
//! |---|---|---|---|
//! | GET  | `/access/items/{item_id}/explanation` | yes | 200 [`Explanation`]; 404 if the caller cannot access the item (V9, V11) |
//! | POST | `/access/preview` | yes | 200 [`AccessPreview`]; body [`PreviewRequest`]; 404/403 if the caller may not take the action (V4, V5, V7) |

use axum::extract::{FromRef, Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{Perms, Relation, RelationTuple};
use crate::permissions::{Explanation, PermissionService, PreviewItem};
use crate::recompute::AccessContext;

/// State of the access routes.
pub struct AccessHttpState<C> {
    pub permissions: PermissionService<C>,
    pub identity: IdentityService,
    pub pool: PgPool,
}

impl<C> Clone for AccessHttpState<C> {
    fn clone(&self) -> Self {
        Self {
            permissions: self.permissions.clone(),
            identity: self.identity.clone(),
            pool: self.pool.clone(),
        }
    }
}

impl<C> FromRef<AccessHttpState<C>> for IdentityService {
    fn from_ref(state: &AccessHttpState<C>) -> Self {
        state.identity.clone()
    }
}

/// Role a previewed new member would get.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AccessMemberRole")]
pub enum MemberRole {
    Assignee,
    Watcher,
    /// Notebook member who edits its pages.
    Editor,
    /// Notebook member who views its pages.
    Viewer,
}

impl MemberRole {
    const fn relation(self) -> Relation {
        match self {
            Self::Assignee => Relation::Assignee,
            Self::Watcher => Relation::Watcher,
            Self::Editor => Relation::Editor,
            Self::Viewer => Relation::Viewer,
        }
    }
}

/// The action to preview (V7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
#[ts(export, rename = "AccessPreviewRequest")]
pub enum PreviewRequest {
    /// Link page `page_id` to item `item_id`.
    Link { page_id: Uuid, item_id: Uuid },
    /// Add `user_id` to item `item_id` with `role`.
    AddMember {
        item_id: Uuid,
        user_id: Uuid,
        role: MemberRole,
    },
}

/// Items whose viewers would grow, with the new viewers; empty if nobody gains access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct AccessPreview {
    pub items: Vec<PreviewItem>,
}

/// The access routes.
pub fn router<C: AccessContext>(state: AccessHttpState<C>) -> Router {
    Router::new()
        .route("/access/items/{item_id}/explanation", get(explanation::<C>))
        .route("/access/preview", post(preview::<C>))
        .with_state(state)
}

fn internal(err: sqlx::Error) -> AppError {
    AppError::Internal(Box::new(err))
}

async fn explanation<C: AccessContext>(
    State(state): State<AccessHttpState<C>>,
    Authenticated(auth): Authenticated,
    Path(item_id): Path<Uuid>,
) -> Result<Json<Explanation>, AppError> {
    state
        .permissions
        .explain(&state.pool, auth.user_id.as_uuid(), item_id)
        .await
        .map_err(internal)?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn preview<C: AccessContext>(
    State(state): State<AccessHttpState<C>>,
    Authenticated(auth): Authenticated,
    Json(request): Json<PreviewRequest>,
) -> Result<Json<AccessPreview>, AppError> {
    let actor = auth.user_id.as_uuid();
    let permissions = &state.permissions;
    let tuple = match request {
        PreviewRequest::Link { page_id, item_id } => {
            // V4: only someone who may link the page, and only to an item they can see.
            permissions
                .require(&state.pool, actor, page_id, Perms::LINK)
                .await?;
            permissions
                .require(&state.pool, actor, item_id, Perms::VIEW)
                .await?;
            RelationTuple::new(page_id, Relation::LinkedTo, item_id)
        }
        PreviewRequest::AddMember {
            item_id,
            user_id,
            role,
        } => {
            // V5: adding members needs SHARE.
            permissions
                .require(&state.pool, actor, item_id, Perms::SHARE)
                .await?;
            RelationTuple::new(item_id, role.relation(), user_id)
        }
    };
    let mut conn = state.pool.acquire().await.map_err(internal)?;
    let items = permissions
        .preview(&mut conn, tuple)
        .await
        .map_err(internal)?;
    Ok(Json(AccessPreview { items }))
}
