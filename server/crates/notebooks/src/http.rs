//! Notebook and page endpoints (T2.5.1). Mount inside `idempotency::protect` (every mutation
//! carries an idempotency key, ADR-0003). Authorization goes through
//! `access::PermissionService` in [`NotebooksService`].
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | GET  | `/notebooks` | 200 `NotebookSummary[]`; creates the default notebook once |
//! | POST | `/notebooks` | 201 [`Notebook`] (Y1) |
//! | GET  | `/notebooks/{id}` | 200 [`Notebook`]; VIEW; visible pages only |
//! | POST | `/notebooks/{id}/pages` | 201 [`Page`]; EDIT; owned by the notebook's owner (Y5) |
//! | GET  | `/pages/{id}` | 200 [`Page`]; VIEW |

use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::model::{
    CreateNotebookRequest, CreatePageRequest, Notebook, NotebookSummary, Page, default_title,
};
use crate::service::NotebooksService;

/// State of the notebook routes.
#[derive(Clone)]
pub struct NotebooksHttpState {
    pub notebooks: NotebooksService,
    pub identity: IdentityService,
}

impl FromRef<NotebooksHttpState> for IdentityService {
    fn from_ref(state: &NotebooksHttpState) -> Self {
        state.identity.clone()
    }
}

/// The notebook routes.
pub fn router(state: NotebooksHttpState) -> Router {
    Router::new()
        .route("/notebooks", get(list_notebooks).post(create_notebook))
        .route("/notebooks/{id}", get(read_notebook))
        .route("/notebooks/{id}/pages", post(create_page))
        .route("/pages/{id}", get(read_page))
        .with_state(state)
}

async fn list_notebooks(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<NotebookSummary>>, AppError> {
    let user = state.identity.current_user(auth.user_id).await?;
    let actor = auth.user_id.as_uuid();
    state
        .notebooks
        .ensure_default(actor, default_title(&user.locale))
        .await?;
    state.notebooks.notebooks(actor).await.map(Json)
}

async fn create_notebook(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateNotebookRequest>,
) -> Result<(StatusCode, Json<Notebook>), AppError> {
    let notebook = state
        .notebooks
        .create(auth.user_id.as_uuid(), request)
        .await?;
    Ok((StatusCode::CREATED, Json(notebook)))
}

async fn read_notebook(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Notebook>, AppError> {
    state
        .notebooks
        .notebook(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn create_page(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<CreatePageRequest>,
) -> Result<(StatusCode, Json<Page>), AppError> {
    let page = state
        .notebooks
        .create_page(auth.user_id.as_uuid(), id, request)
        .await?;
    Ok((StatusCode::CREATED, Json(page)))
}

async fn read_page(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<Page>, AppError> {
    state
        .notebooks
        .page(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}
