//! `tasks`: Tasks, subtasks and the personal kanban (ADR-0014).
//!
//! See ARCHITECTURE §5.4 and PRD G1–G6. A task is an `items` row plus a detail row here; `items`
//! is only reached through [`items::ItemsService`] (ADR-0007).

mod board;
mod due;
mod fractional;
pub mod http;
mod model;
mod progress;
pub mod repo;
mod service;

pub use board::{
    Board, BoardCard, BoardColumn, COLUMN_LIMIT, CreateBoardColumnRequest, MoveBoardTaskRequest,
    UpdateBoardColumnRequest,
};
pub use due::DueTask;
pub use fractional::{MAX_KEY_LEN, even_keys, key_between};
pub use model::{
    CreateSubtaskRequest, CreateTaskRequest, SUBTASK_LIMIT, SubtaskView, TaskView,
    UpdateSubtaskRequest, UpdateTaskRequest, may_mark, subtask_titles,
};
pub use progress::{TaskProgress, completion, task_progress};
pub use service::TasksService;
