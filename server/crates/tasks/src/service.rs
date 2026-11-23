//! `TasksService`: tasks, subtasks and assignees (PRD G1, G2, G4, T2.1.1). Every authorization
//! goes through `access::PermissionService`; facts about the item come from `ItemsService`
//! (ADR-0007).

use access::{PermissionService, Perms};
use items::{CreateItemRequest, Item, ItemsAccessContext, ItemsService, Owner, TouchAction};
use kernel::{AppError, FieldError, FieldErrorCode, SharedClock, ValidationErrors};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::model::{
    CreateSubtaskRequest, CreateTaskRequest, SUBTASK_LIMIT, SubtaskView, TaskView,
    UpdateSubtaskRequest, UpdateTaskRequest, check_dates, check_subtask_title, may_mark,
    subtask_titles,
};
use crate::repo;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn subtask_title(title: &str) -> Result<String, AppError> {
    let title = title.trim();
    let mut errors = ValidationErrors::new();
    check_subtask_title(&mut errors, "title", title);
    errors.into_result()?;
    Ok(title.to_owned())
}

/// The owner and every assignee, each once (G1).
fn member_ids(item: &Item, subtasks: &[SubtaskView]) -> Vec<Uuid> {
    let mut members: Vec<Uuid> = subtasks
        .iter()
        .flat_map(|s| s.assignee_ids.iter().copied())
        .collect();
    if let Owner::User { user_id } = item.owner {
        members.push(user_id);
    }
    members.sort_unstable();
    members.dedup();
    members
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct TasksService {
    pool: PgPool,
    clock: SharedClock,
    permissions: PermissionService<ItemsAccessContext>,
    items: ItemsService,
}

impl TasksService {
    #[must_use]
    pub const fn new(
        pool: PgPool,
        clock: SharedClock,
        permissions: PermissionService<ItemsAccessContext>,
        items: ItemsService,
    ) -> Self {
        Self {
            pool,
            clock,
            permissions,
            items,
        }
    }

    /// Creates a personal task with its item and at least one subtask (G2), in one transaction.
    ///
    /// # Errors
    /// [`AppError::Validation`], or [`AppError::Internal`].
    pub async fn create(
        &self,
        actor: Uuid,
        request: CreateTaskRequest,
    ) -> Result<TaskView, AppError> {
        request.check()?;
        let titles = subtask_titles(&request.title, request.subtasks.as_deref());
        let new_item = CreateItemRequest {
            kind: "task".into(),
            title: request.title,
            description: request.description,
        }
        .validate()?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let item = self.items.create_in(&mut tx, actor, new_item).await?;
        repo::insert_task(
            &mut tx,
            item.id,
            (request.start_date, request.due_date),
            now,
        )
        .await
        .map_err(internal)?;
        for title in &titles {
            repo::insert_subtask(&mut tx, item.id, title, now)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        self.task(actor, item.id).await
    }

    /// A task with every G1 field (VIEW).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not visible or the item is not a task, or
    /// [`AppError::Internal`].
    pub async fn task(&self, actor: Uuid, id: Uuid) -> Result<TaskView, AppError> {
        let item = self.items.get(actor, id).await?;
        let task = repo::get_task(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let subtasks = repo::list_subtasks(&self.pool, id)
            .await
            .map_err(internal)?;
        let tags = self.items.item_tags(actor, id).await?;
        let pages = self
            .items
            .links(actor, id)
            .await?
            .into_iter()
            .filter(|link| link.item.kind == "page")
            .collect();
        let member_ids = member_ids(&item, &subtasks);
        Ok(TaskView {
            item,
            start_date: task.start_date,
            due_date: task.due_date,
            subtasks,
            tags,
            pages,
            member_ids,
        })
    }

    /// Changes the task's dates (EDIT).
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`], [`AppError::Validation`],
    /// [`AppError::Conflict`] for a task in the trash, or [`AppError::Internal`].
    pub async fn update(
        &self,
        actor: Uuid,
        id: Uuid,
        request: UpdateTaskRequest,
    ) -> Result<TaskView, AppError> {
        self.require_live(actor, id).await?;
        if request.start_date.is_none() && request.due_date.is_none() {
            let mut errors = ValidationErrors::new();
            errors.push(FieldError::new(
                "due_date",
                FieldErrorCode::Required,
                "nothing to change",
            ));
            errors.into_result()?;
        }
        let mut tx = self.begin_locked(id).await?;
        let current = repo::get_locked(&mut tx, id).await?;
        let start = request.start_date.unwrap_or(current.start_date);
        let due = request.due_date.unwrap_or(current.due_date);
        check_dates(start, due)?;
        repo::update_dates(&mut tx, id, (start, due), self.clock.now())
            .await
            .map_err(internal)?;
        self.finish(tx, id, TouchAction::Edited).await?;
        self.task(actor, id).await
    }

    /// Adds a subtask at the end (EDIT).
    ///
    /// # Errors
    /// As [`Self::update`]; [`AppError::Conflict`] at [`SUBTASK_LIMIT`].
    pub async fn add_subtask(
        &self,
        actor: Uuid,
        id: Uuid,
        request: CreateSubtaskRequest,
    ) -> Result<SubtaskView, AppError> {
        self.require_live(actor, id).await?;
        let title = subtask_title(&request.title)?;
        let mut tx = self.begin_locked(id).await?;
        let count = repo::count_subtasks(&mut tx, id).await.map_err(internal)?;
        if usize::try_from(count).unwrap_or(usize::MAX) >= SUBTASK_LIMIT {
            return Err(AppError::Conflict(format!(
                "a task can have at most {SUBTASK_LIMIT} subtasks"
            )));
        }
        let subtask = repo::insert_subtask(&mut tx, id, &title, self.clock.now())
            .await
            .map_err(internal)?;
        let view = load_subtask(&mut tx, id, subtask).await?;
        self.finish(tx, id, TouchAction::Edited).await?;
        Ok(view)
    }

    /// Renames a subtask (EDIT).
    ///
    /// # Errors
    /// As [`Self::update`].
    pub async fn rename_subtask(
        &self,
        actor: Uuid,
        id: Uuid,
        subtask: Uuid,
        request: UpdateSubtaskRequest,
    ) -> Result<SubtaskView, AppError> {
        self.require_live(actor, id).await?;
        let title = subtask_title(&request.title)?;
        let mut tx = self.begin_locked(id).await?;
        let renamed = repo::rename_subtask(&mut tx, id, subtask, &title, self.clock.now())
            .await
            .map_err(internal)?;
        if !renamed {
            return Err(AppError::NotFound);
        }
        let view = load_subtask(&mut tx, id, subtask).await?;
        self.finish(tx, id, TouchAction::Edited).await?;
        Ok(view)
    }

    /// Deletes a subtask (EDIT). The last one cannot be deleted (G2).
    ///
    /// # Errors
    /// As [`Self::update`]; [`AppError::Conflict`] for the last subtask.
    pub async fn delete_subtask(
        &self,
        actor: Uuid,
        id: Uuid,
        subtask: Uuid,
    ) -> Result<(), AppError> {
        self.require_live(actor, id).await?;
        let mut tx = self.begin_locked(id).await?;
        load_subtask(&mut tx, id, subtask).await?;
        let count = repo::count_subtasks(&mut tx, id).await.map_err(internal)?;
        if count <= 1 {
            return Err(AppError::Conflict(
                "a task must keep at least one subtask".into(),
            ));
        }
        repo::delete_subtask(&mut tx, id, subtask)
            .await
            .map_err(internal)?;
        self.finish(tx, id, TouchAction::Edited).await
    }

    /// Marks a subtask done (G4). One assignee is enough. Repeating it changes nothing: the first
    /// person and time stay.
    ///
    /// # Errors
    /// As [`Self::update`]; [`AppError::Forbidden`] for someone who is neither an assignee nor
    /// a manager of an assigned subtask.
    pub async fn complete_subtask(
        &self,
        actor: Uuid,
        id: Uuid,
        subtask: Uuid,
    ) -> Result<SubtaskView, AppError> {
        self.require_live(actor, id).await?;
        self.require_may_mark(actor, id, subtask).await?;
        let mut tx = self.begin_locked(id).await?;
        let completed = repo::mark_done(&mut tx, id, subtask, actor, self.clock.now())
            .await
            .map_err(internal)?;
        let view = load_subtask(&mut tx, id, subtask).await?;
        if completed {
            // A1: the task itself is completed when its last subtask is.
            if repo::all_done(&mut tx, id).await.map_err(internal)? {
                self.items
                    .record_completed(&mut tx, actor, id)
                    .await
                    .map_err(internal)?;
            } else {
                self.items
                    .touch(&mut tx, id, TouchAction::Completed)
                    .await
                    .map_err(internal)?;
            }
        }
        tx.commit().await.map_err(internal)?;
        Ok(view)
    }

    /// Marks a subtask not done. Repeating it changes nothing.
    ///
    /// # Errors
    /// As [`Self::complete_subtask`].
    pub async fn reopen_subtask(
        &self,
        actor: Uuid,
        id: Uuid,
        subtask: Uuid,
    ) -> Result<SubtaskView, AppError> {
        self.require_live(actor, id).await?;
        self.require_may_mark(actor, id, subtask).await?;
        let mut tx = self.begin_locked(id).await?;
        let reopened = repo::mark_undone(&mut tx, id, subtask, self.clock.now())
            .await
            .map_err(internal)?;
        let view = load_subtask(&mut tx, id, subtask).await?;
        if reopened {
            self.items
                .touch(&mut tx, id, TouchAction::Edited)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(view)
    }

    /// Assigns `user` to a subtask (EDIT). In this phase a person can only assign themselves;
    /// assigning others opens with T3.2.3. Repeating it changes nothing.
    ///
    /// # Errors
    /// As [`Self::update`]; [`AppError::Forbidden`] for another person.
    pub async fn assign(
        &self,
        actor: Uuid,
        id: Uuid,
        subtask: Uuid,
        user: Uuid,
    ) -> Result<SubtaskView, AppError> {
        self.require_live(actor, id).await?;
        if user != actor {
            return Err(AppError::Forbidden);
        }
        let mut tx = self.begin_locked(id).await?;
        load_subtask(&mut tx, id, subtask).await?;
        let added = repo::add_assignee(&mut tx, subtask, user, self.clock.now())
            .await
            .map_err(internal)?;
        let view = load_subtask(&mut tx, id, subtask).await?;
        if added {
            self.items
                .record_assigned(&mut tx, actor, id)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(view)
    }

    /// Removes `user` from a subtask (EDIT; own assignment only in this phase). Repeating it
    /// changes nothing.
    ///
    /// # Errors
    /// As [`Self::assign`].
    pub async fn unassign(
        &self,
        actor: Uuid,
        id: Uuid,
        subtask: Uuid,
        user: Uuid,
    ) -> Result<SubtaskView, AppError> {
        self.require_live(actor, id).await?;
        if user != actor {
            return Err(AppError::Forbidden);
        }
        let mut tx = self.begin_locked(id).await?;
        load_subtask(&mut tx, id, subtask).await?;
        let removed = repo::remove_assignee(&mut tx, subtask, user)
            .await
            .map_err(internal)?;
        let view = load_subtask(&mut tx, id, subtask).await?;
        if removed {
            self.items
                .touch(&mut tx, id, TouchAction::Edited)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(view)
    }

    /// EDIT on a task that is not in the trash. An item that is not a task is not found.
    async fn require_live(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        self.permissions
            .require(&self.pool, actor, id, Perms::EDIT)
            .await?;
        let item = self.items.get(actor, id).await?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        repo::get_task(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        Ok(())
    }

    /// G4: on an assigned subtask, only an assignee or a manager may change its state.
    async fn require_may_mark(&self, actor: Uuid, id: Uuid, subtask: Uuid) -> Result<(), AppError> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let current = load_subtask(&mut conn, id, subtask).await?;
        drop(conn);
        if may_mark(actor, &current.assignee_ids, false) {
            return Ok(());
        }
        self.permissions
            .require(&self.pool, actor, id, Perms::MANAGE)
            .await
    }

    async fn begin_locked(&self, id: Uuid) -> Result<Transaction<'static, Postgres>, AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        repo::lock_task(&mut tx, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        Ok(tx)
    }

    async fn finish(
        &self,
        mut tx: Transaction<'static, Postgres>,
        id: Uuid,
        action: TouchAction,
    ) -> Result<(), AppError> {
        self.items
            .touch(&mut tx, id, action)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }
}

async fn load_subtask(
    conn: &mut PgConnection,
    task: Uuid,
    subtask: Uuid,
) -> Result<SubtaskView, AppError> {
    repo::get_subtask(conn, task, subtask)
        .await
        .map_err(internal)?
        .ok_or(AppError::NotFound)
}
