//! Task endpoints (T2.1.1). Mount inside `idempotency::protect` (every mutation carries an
//! idempotency key, ADR-0003). Authorization goes through `access::PermissionService` in
//! [`TasksService`].
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | POST   | `/tasks` | 201 [`TaskView`]; creates the item and at least one subtask (G2) |
//! | GET    | `/tasks/{id}` | 200 [`TaskView`]; VIEW |
//! | PATCH  | `/tasks/{id}` | 200 [`TaskView`]; dates, EDIT |
//! | POST   | `/tasks/{id}/subtasks` | 201 [`SubtaskView`]; EDIT |
//! | PATCH  | `/tasks/{id}/subtasks/{subtask_id}` | 200 [`SubtaskView`]; EDIT |
//! | DELETE | `/tasks/{id}/subtasks/{subtask_id}` | 204; EDIT, 409 for the last one (G2) |
//! | POST   | `/tasks/{id}/subtasks/{subtask_id}/complete` | 200 [`SubtaskView`]; G4 |
//! | POST   | `/tasks/{id}/subtasks/{subtask_id}/reopen` | 200 [`SubtaskView`]; G4 |
//! | PUT    | `/tasks/{id}/subtasks/{subtask_id}/assignees/{user_id}` | 200 [`SubtaskView`]; self |
//! | DELETE | `/tasks/{id}/subtasks/{subtask_id}/assignees/{user_id}` | 200 [`SubtaskView`]; self |
//! | GET    | `/board` | 200 [`Board`]; the caller's personal kanban (G6) |
//! | POST   | `/board/columns` | 201 [`Board`] |
//! | PATCH  | `/board/columns/{id}` | 200 [`Board`]; rename or reorder |
//! | DELETE | `/board/columns/{id}` | 204; its cards move to the inbox |
//! | PUT    | `/board/tasks/{task_id}` | 200 [`Board`]; VIEW on the task |

use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;
use uuid::Uuid;

use crate::board::{
    Board, CreateBoardColumnRequest, MoveBoardTaskRequest, UpdateBoardColumnRequest,
};
use crate::model::{
    CreateSubtaskRequest, CreateTaskRequest, SubtaskView, TaskView, UpdateSubtaskRequest,
    UpdateTaskRequest,
};
use crate::service::TasksService;

/// State of the task routes.
#[derive(Clone)]
pub struct TasksHttpState {
    pub tasks: TasksService,
    pub identity: IdentityService,
}

impl FromRef<TasksHttpState> for IdentityService {
    fn from_ref(state: &TasksHttpState) -> Self {
        state.identity.clone()
    }
}

/// The task routes.
pub fn router(state: TasksHttpState) -> Router {
    Router::new()
        .route("/tasks", post(create_task))
        .route("/tasks/{id}", patch(update_task).get(read_task))
        .route("/tasks/{id}/subtasks", post(add_subtask))
        .route(
            "/tasks/{id}/subtasks/{subtask_id}",
            patch(rename_subtask).delete(delete_subtask),
        )
        .route(
            "/tasks/{id}/subtasks/{subtask_id}/complete",
            post(complete_subtask),
        )
        .route(
            "/tasks/{id}/subtasks/{subtask_id}/reopen",
            post(reopen_subtask),
        )
        .route(
            "/tasks/{id}/subtasks/{subtask_id}/assignees/{user_id}",
            put(assign).delete(unassign),
        )
        .route("/board", get(read_board))
        .route("/board/columns", post(create_column))
        .route(
            "/board/columns/{id}",
            patch(update_column).delete(delete_column),
        )
        .route("/board/tasks/{task_id}", put(move_card))
        .with_state(state)
}

async fn create_task(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskView>), AppError> {
    let task = state.tasks.create(auth.user_id.as_uuid(), request).await?;
    Ok((StatusCode::CREATED, Json(task)))
}

async fn read_task(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskView>, AppError> {
    state.tasks.task(auth.user_id.as_uuid(), id).await.map(Json)
}

async fn update_task(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateTaskRequest>,
) -> Result<Json<TaskView>, AppError> {
    state
        .tasks
        .update(auth.user_id.as_uuid(), id, request)
        .await
        .map(Json)
}

async fn add_subtask(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<CreateSubtaskRequest>,
) -> Result<(StatusCode, Json<SubtaskView>), AppError> {
    let subtask = state
        .tasks
        .add_subtask(auth.user_id.as_uuid(), id, request)
        .await?;
    Ok((StatusCode::CREATED, Json(subtask)))
}

async fn rename_subtask(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, subtask)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateSubtaskRequest>,
) -> Result<Json<SubtaskView>, AppError> {
    state
        .tasks
        .rename_subtask(auth.user_id.as_uuid(), id, subtask, request)
        .await
        .map(Json)
}

async fn delete_subtask(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, subtask)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    state
        .tasks
        .delete_subtask(auth.user_id.as_uuid(), id, subtask)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn complete_subtask(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, subtask)): Path<(Uuid, Uuid)>,
) -> Result<Json<SubtaskView>, AppError> {
    state
        .tasks
        .complete_subtask(auth.user_id.as_uuid(), id, subtask)
        .await
        .map(Json)
}

async fn reopen_subtask(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, subtask)): Path<(Uuid, Uuid)>,
) -> Result<Json<SubtaskView>, AppError> {
    state
        .tasks
        .reopen_subtask(auth.user_id.as_uuid(), id, subtask)
        .await
        .map(Json)
}

async fn assign(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, subtask, user)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<SubtaskView>, AppError> {
    state
        .tasks
        .assign(auth.user_id.as_uuid(), id, subtask, user)
        .await
        .map(Json)
}

async fn unassign(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path((id, subtask, user)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<SubtaskView>, AppError> {
    state
        .tasks
        .unassign(auth.user_id.as_uuid(), id, subtask, user)
        .await
        .map(Json)
}

async fn read_board(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Board>, AppError> {
    state.tasks.board(auth.user_id.as_uuid()).await.map(Json)
}

async fn create_column(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<CreateBoardColumnRequest>,
) -> Result<(StatusCode, Json<Board>), AppError> {
    let board = state
        .tasks
        .create_column(auth.user_id.as_uuid(), request)
        .await?;
    Ok((StatusCode::CREATED, Json(board)))
}

async fn update_column(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateBoardColumnRequest>,
) -> Result<Json<Board>, AppError> {
    state
        .tasks
        .update_column(auth.user_id.as_uuid(), id, request)
        .await
        .map(Json)
}

async fn delete_column(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .tasks
        .delete_column(auth.user_id.as_uuid(), id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn move_card(
    State(state): State<TasksHttpState>,
    Authenticated(auth): Authenticated,
    Path(task): Path<Uuid>,
    Json(request): Json<MoveBoardTaskRequest>,
) -> Result<Json<Board>, AppError> {
    state
        .tasks
        .move_card(auth.user_id.as_uuid(), task, request)
        .await
        .map(Json)
}
