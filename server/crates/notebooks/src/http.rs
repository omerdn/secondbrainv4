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
//! | GET  | `/items/{id}/process-page` | 200 [`ProcessPage`] with the latest line; VIEW on the item (S3) |
//! | POST | `/items/{id}/process-page` | 200 [`ProcessPage`], created on the first note; EDIT (S1) |
//! | GET  | `/pages/{id}/versions?before=` | 200 [`PageVersion`]`[]`, newest first; VIEW (Y3) |
//! | GET  | `/pages/{id}/versions/{version}` | 200 [`PageVersionContent`]; VIEW (Y3) |
//! | POST | `/pages/{id}/versions/{version}/restore` | 200 [`PageVersion`], the new latest; EDIT (Y3) |

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::collab::Collab;
use crate::model::{
    CreateNotebookRequest, CreatePageRequest, Notebook, NotebookSummary, Page, PageVersion,
    PageVersionContent, VersionsQuery, default_title,
};
use crate::process::{ProcessPage, ProcessPages};
use crate::service::NotebooksService;

/// State of the notebook routes.
#[derive(Clone)]
pub struct NotebooksHttpState {
    pub notebooks: NotebooksService,
    pub identity: IdentityService,
    /// Open rooms, so a restored version reaches every editor.
    pub collab: Collab,
    pub process: ProcessPages,
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
        .route(
            "/items/{id}/process-page",
            get(read_process_page).post(open_process_page),
        )
        .route("/pages/{id}/versions", get(list_versions))
        .route("/pages/{id}/versions/{version}", get(read_version))
        .route(
            "/pages/{id}/versions/{version}/restore",
            post(restore_version),
        )
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

async fn list_versions(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Query(query): Query<VersionsQuery>,
) -> Result<Json<Vec<PageVersion>>, AppError> {
    state
        .notebooks
        .versions(auth.user_id.as_uuid(), id, query.before)
        .await
        .map(Json)
}

async fn read_version(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, version)): Path<(Uuid, i32)>,
) -> Result<Json<PageVersionContent>, AppError> {
    state
        .notebooks
        .version(auth.user_id.as_uuid(), id, version)
        .await
        .map(Json)
}

async fn restore_version(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, version)): Path<(Uuid, i32)>,
) -> Result<Json<PageVersion>, AppError> {
    state
        .notebooks
        .restore_version(auth.user_id.as_uuid(), id, version, &state.collab)
        .await
        .map(Json)
}

async fn read_process_page(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ProcessPage>, AppError> {
    state
        .process
        .get(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}

async fn open_process_page(
    State(state): State<NotebooksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ProcessPage>, AppError> {
    state
        .process
        .open(auth.user_id.as_uuid(), id)
        .await
        .map(Json)
}
