//! The personal kanban (PRD G6, ADR-0014, T2.1.3). Columns and placements belong to one user, so
//! a move never touches the shared task or item: other members cannot notice it, and progress
//! (G3) does not depend on it. Every board change locks the user's inbox row, so one user's board
//! changes run one at a time.

use std::collections::{HashMap, HashSet};

use access::Perms;
use chrono::{DateTime, NaiveDate, Utc};
use items::{Item, ItemFilter};
use kernel::validation::{max_chars, non_blank};
use kernel::{AppError, FieldError, FieldErrorCode, PageParams, PageRequest, ValidationErrors};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection};
use ts_rs::TS;
use uuid::Uuid;

use crate::fractional::{MAX_KEY_LEN, even_keys, key_between};
use crate::service::TasksService;

/// Most columns one board may have.
pub const COLUMN_LIMIT: usize = 30;
const COLUMN_NAME_MAX_CHARS: usize = 60;
const INBOX_NAME: &str = "Inbox";
/// The inbox stays first: other columns are always placed after this key.
const INBOX_KEY: &str = "V";
/// Pages of 200 tasks a board reads.
const BOARD_PAGES: usize = 25;

/// `POST /board/columns` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateBoardColumnRequest {
    pub name: String,
}

/// `PATCH /board/columns/{id}` body: only the fields present change.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateBoardColumnRequest {
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    /// Moves the column right after this one. The inbox stays first.
    #[serde(default)]
    #[ts(optional)]
    pub after_column_id: Option<Uuid>,
}

/// `PUT /board/tasks/{task_id}` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct MoveBoardTaskRequest {
    pub column_id: Uuid,
    /// The card to land after; the top of the column when absent.
    #[serde(default)]
    #[ts(optional)]
    pub after_task_id: Option<Uuid>,
}

/// A task on the caller's board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BoardCard {
    pub task_id: Uuid,
    pub title: String,
    #[ts(type = "string | null")]
    pub start_date: Option<NaiveDate>,
    #[ts(type = "string | null")]
    pub due_date: Option<NaiveDate>,
    pub subtasks_done: i32,
    pub subtasks_total: i32,
}

/// A column with its cards, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BoardColumn {
    pub id: Uuid,
    pub name: String,
    /// New tasks land here (G6). It is always first and cannot be deleted.
    pub is_inbox: bool,
    pub cards: Vec<BoardCard>,
}

/// The caller's board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Board {
    pub columns: Vec<BoardColumn>,
}

#[derive(FromRow)]
struct ColumnRow {
    id: Uuid,
    name: String,
    position: String,
    is_inbox: bool,
}

#[derive(FromRow)]
struct CardRow {
    task_id: Uuid,
    column_id: Uuid,
    start_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    subtasks_done: i32,
    subtasks_total: i32,
}

#[derive(FromRow)]
struct KeyRow {
    id: Uuid,
    position: String,
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn column_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    let mut errors = ValidationErrors::new();
    errors.check(non_blank("name", name));
    errors.check(max_chars("name", name, COLUMN_NAME_MAX_CHARS));
    errors.into_result()?;
    Ok(name.to_owned())
}

/// The user's inbox, created on first use and locked for the rest of the transaction.
async fn lock_inbox(conn: &mut PgConnection, user: Uuid, now: DateTime<Utc>) -> sqlx::Result<Uuid> {
    sqlx::query(
        "INSERT INTO user_board_columns (id, user_id, name, position, is_inbox, created_at,
                                         updated_at)
         VALUES ($1, $2, $3, $4, true, $5, $5)
         ON CONFLICT (user_id) WHERE is_inbox DO NOTHING",
    )
    .bind(kernel::Id::<()>::new_at(now).as_uuid())
    .bind(user)
    .bind(INBOX_NAME)
    .bind(INBOX_KEY)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    sqlx::query_scalar(
        "SELECT id FROM user_board_columns WHERE user_id = $1 AND is_inbox FOR UPDATE",
    )
    .bind(user)
    .fetch_one(conn)
    .await
}

async fn columns(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<Vec<ColumnRow>> {
    sqlx::query_as(
        "SELECT id, name, position, is_inbox FROM user_board_columns
         WHERE user_id = $1 ORDER BY position",
    )
    .bind(user)
    .fetch_all(conn)
    .await
}

async fn card_keys(conn: &mut PgConnection, user: Uuid, column: Uuid) -> sqlx::Result<Vec<KeyRow>> {
    sqlx::query_as(
        "SELECT task_id AS id, position FROM user_task_placements
         WHERE user_id = $1 AND column_id = $2 ORDER BY position",
    )
    .bind(user)
    .bind(column)
    .fetch_all(conn)
    .await
}

async fn put_placement(
    conn: &mut PgConnection,
    user: Uuid,
    task: Uuid,
    (column, key): (Uuid, &str),
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO user_task_placements (user_id, task_id, column_id, position, created_at,
                                           updated_at)
         VALUES ($1, $2, $3, $4, $5, $5)
         ON CONFLICT (user_id, task_id)
         DO UPDATE SET column_id = EXCLUDED.column_id, position = EXCLUDED.position,
                       updated_at = EXCLUDED.updated_at",
    )
    .bind(user)
    .bind(task)
    .bind(column)
    .bind(key)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// Appends tasks to the end of a column, in the given order.
async fn append(
    conn: &mut PgConnection,
    user: Uuid,
    column: Uuid,
    tasks: &[Uuid],
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    if tasks.is_empty() {
        return Ok(());
    }
    let existing = card_keys(&mut *conn, user, column).await?;
    let mut last = existing.last().map(|row| row.position.clone());
    if last.as_ref().is_some_and(|key| key.len() >= MAX_KEY_LEN) {
        let order: Vec<Uuid> = existing.iter().map(|row| row.id).collect();
        last = renumber(&mut *conn, user, column, &order, now).await?;
    }
    for task in tasks {
        let key = key_between(last.as_deref(), None);
        put_placement(&mut *conn, user, *task, (column, &key), now).await?;
        last = Some(key);
    }
    Ok(())
}

/// Rewrites a column with short keys in the given order; returns the last key.
async fn renumber(
    conn: &mut PgConnection,
    user: Uuid,
    column: Uuid,
    order: &[Uuid],
    now: DateTime<Utc>,
) -> sqlx::Result<Option<String>> {
    let keys = even_keys(order.len());
    for (task, key) in order.iter().zip(&keys) {
        put_placement(&mut *conn, user, *task, (column, key), now).await?;
    }
    Ok(keys.last().cloned())
}

/// Places a new task at the end of its creator's inbox (G6), in the creating transaction.
pub(crate) async fn place_new(
    conn: &mut PgConnection,
    user: Uuid,
    task: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    let inbox = lock_inbox(&mut *conn, user, now).await?;
    append(conn, user, inbox, &[task], now).await
}

impl TasksService {
    /// Active tasks `actor` can see, oldest first.
    async fn visible_tasks(&self, actor: Uuid) -> Result<Vec<Item>, AppError> {
        let filter = ItemFilter {
            kinds: Some(vec!["task".into()]),
            ..ItemFilter::default()
        };
        let mut found = Vec::new();
        let mut cursor = None;
        for _ in 0..BOARD_PAGES {
            let mut page = PageRequest::from_params(PageParams {
                limit: Some(200),
                cursor: None,
            })?;
            page.cursor = cursor;
            let result = self.items.filter_items(actor, &filter, &page).await?;
            found.extend(result.items);
            cursor = result.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        found.reverse();
        Ok(found)
    }

    /// The caller's board. Tasks they can see but have not placed yet (shared with them, G6) land
    /// in the inbox; tasks they can no longer see are left out (V9).
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn board(&self, actor: Uuid) -> Result<Board, AppError> {
        let visible = self.visible_tasks(actor).await?;
        let ids: Vec<Uuid> = visible.iter().map(|item| item.id).collect();
        let titles: HashMap<Uuid, String> = visible
            .into_iter()
            .map(|item| (item.id, item.title))
            .collect();
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let inbox = lock_inbox(&mut tx, actor, now).await.map_err(internal)?;
        let missing: HashSet<Uuid> = sqlx::query_scalar(
            "SELECT t.item_id FROM tasks t
             WHERE t.item_id = ANY($1)
               AND NOT EXISTS (SELECT 1 FROM user_task_placements p
                               WHERE p.user_id = $2 AND p.task_id = t.item_id)",
        )
        .bind(&ids)
        .bind(actor)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal)?
        .into_iter()
        .collect();
        let to_place: Vec<Uuid> = ids
            .iter()
            .copied()
            .filter(|id| missing.contains(id))
            .collect();
        append(&mut tx, actor, inbox, &to_place, now)
            .await
            .map_err(internal)?;
        let column_rows = columns(&mut tx, actor).await.map_err(internal)?;
        let card_rows: Vec<CardRow> = sqlx::query_as(
            "SELECT p.task_id, p.column_id, t.start_date, t.due_date,
                    (SELECT count(*) FILTER (WHERE s.done_at IS NOT NULL)::int4
                     FROM subtasks s WHERE s.task_id = t.item_id) AS subtasks_done,
                    (SELECT count(*)::int4 FROM subtasks s WHERE s.task_id = t.item_id)
                        AS subtasks_total
             FROM user_task_placements p JOIN tasks t ON t.item_id = p.task_id
             WHERE p.user_id = $1 AND p.task_id = ANY($2)
             ORDER BY p.position",
        )
        .bind(actor)
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;

        let mut by_column: HashMap<Uuid, Vec<BoardCard>> = HashMap::new();
        for row in card_rows {
            let Some(title) = titles.get(&row.task_id) else {
                continue;
            };
            by_column.entry(row.column_id).or_default().push(BoardCard {
                task_id: row.task_id,
                title: title.clone(),
                start_date: row.start_date,
                due_date: row.due_date,
                subtasks_done: row.subtasks_done,
                subtasks_total: row.subtasks_total,
            });
        }
        Ok(Board {
            columns: column_rows
                .into_iter()
                .map(|column| BoardColumn {
                    cards: by_column.remove(&column.id).unwrap_or_default(),
                    id: column.id,
                    name: column.name,
                    is_inbox: column.is_inbox,
                })
                .collect(),
        })
    }

    /// Adds a column at the end of the caller's board.
    ///
    /// # Errors
    /// [`AppError::Validation`], [`AppError::Conflict`] at [`COLUMN_LIMIT`], or
    /// [`AppError::Internal`].
    pub async fn create_column(
        &self,
        actor: Uuid,
        request: CreateBoardColumnRequest,
    ) -> Result<Board, AppError> {
        let name = column_name(&request.name)?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        lock_inbox(&mut tx, actor, now).await.map_err(internal)?;
        let existing = columns(&mut tx, actor).await.map_err(internal)?;
        if existing.len() >= COLUMN_LIMIT {
            return Err(AppError::Conflict(format!(
                "a board can have at most {COLUMN_LIMIT} columns"
            )));
        }
        let key = key_between(existing.last().map(|c| c.position.as_str()), None);
        sqlx::query(
            "INSERT INTO user_board_columns (id, user_id, name, position, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $5)",
        )
        .bind(kernel::Id::<()>::new_at(now).as_uuid())
        .bind(actor)
        .bind(&name)
        .bind(&key)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.board(actor).await
    }

    /// Renames or moves one of the caller's columns.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a column that is not theirs, [`AppError::Validation`],
    /// [`AppError::Conflict`] for moving the inbox, or [`AppError::Internal`].
    pub async fn update_column(
        &self,
        actor: Uuid,
        id: Uuid,
        request: UpdateBoardColumnRequest,
    ) -> Result<Board, AppError> {
        if request.name.is_none() && request.after_column_id.is_none() {
            let mut errors = ValidationErrors::new();
            errors.push(FieldError::new(
                "name",
                FieldErrorCode::Required,
                "nothing to change",
            ));
            errors.into_result()?;
        }
        let name = request.name.as_deref().map(column_name).transpose()?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        lock_inbox(&mut tx, actor, now).await.map_err(internal)?;
        let all = columns(&mut tx, actor).await.map_err(internal)?;
        let column = all
            .iter()
            .find(|column| column.id == id)
            .ok_or(AppError::NotFound)?;
        let mut position = column.position.clone();
        if let Some(after) = request.after_column_id {
            if column.is_inbox {
                return Err(AppError::Conflict("the inbox stays first".into()));
            }
            if after == id {
                return Err(AppError::Conflict("a column cannot follow itself".into()));
            }
            let others: Vec<&ColumnRow> = all.iter().filter(|c| c.id != id).collect();
            let at = others
                .iter()
                .position(|c| c.id == after)
                .ok_or(AppError::NotFound)?;
            position = key_between(
                Some(&others[at].position),
                others.get(at + 1).map(|c| c.position.as_str()),
            );
        }
        sqlx::query(
            "UPDATE user_board_columns
             SET name = COALESCE($3, name), position = $4, updated_at = $5
             WHERE id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(actor)
        .bind(&name)
        .bind(&position)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.board(actor).await
    }

    /// Deletes one of the caller's columns; its cards move to the end of the inbox, so no task
    /// leaves the board.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Conflict`] for the inbox, or [`AppError::Internal`].
    pub async fn delete_column(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let inbox = lock_inbox(&mut tx, actor, now).await.map_err(internal)?;
        if id == inbox {
            return Err(AppError::Conflict("the inbox cannot be deleted".into()));
        }
        let all = columns(&mut tx, actor).await.map_err(internal)?;
        if !all.iter().any(|column| column.id == id) {
            return Err(AppError::NotFound);
        }
        let cards: Vec<Uuid> = card_keys(&mut tx, actor, id)
            .await
            .map_err(internal)?
            .into_iter()
            .map(|row| row.id)
            .collect();
        append(&mut tx, actor, inbox, &cards, now)
            .await
            .map_err(internal)?;
        sqlx::query("DELETE FROM user_board_columns WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(actor)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// Puts a task the caller can see into one of their columns. VIEW is enough: only their own
    /// board changes, and the task and its item stay untouched (G6).
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a task, column or neighbouring card they do not have,
    /// [`AppError::Conflict`] for a task in the trash, or [`AppError::Internal`].
    pub async fn move_card(
        &self,
        actor: Uuid,
        task: Uuid,
        request: MoveBoardTaskRequest,
    ) -> Result<Board, AppError> {
        self.permissions
            .require(&self.pool, actor, task, Perms::VIEW)
            .await?;
        let item = self.items.get(actor, task).await?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        crate::repo::get_task(&self.pool, task)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        lock_inbox(&mut tx, actor, now).await.map_err(internal)?;
        let all = columns(&mut tx, actor).await.map_err(internal)?;
        if !all.iter().any(|column| column.id == request.column_id) {
            return Err(AppError::NotFound);
        }
        let others: Vec<KeyRow> = card_keys(&mut tx, actor, request.column_id)
            .await
            .map_err(internal)?
            .into_iter()
            .filter(|row| row.id != task)
            .collect();
        let at = match request.after_task_id {
            None => 0,
            Some(after) => {
                others
                    .iter()
                    .position(|row| row.id == after)
                    .ok_or(AppError::NotFound)?
                    + 1
            }
        };
        let before = at.checked_sub(1).and_then(|i| others.get(i));
        let key = key_between(
            before.map(|row| row.position.as_str()),
            others.get(at).map(|row| row.position.as_str()),
        );
        if key.len() > MAX_KEY_LEN {
            let mut order: Vec<Uuid> = others.iter().map(|row| row.id).collect();
            order.insert(at, task);
            renumber(&mut tx, actor, request.column_id, &order, now)
                .await
                .map_err(internal)?;
        } else {
            put_placement(&mut tx, actor, task, (request.column_id, &key), now)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        self.board(actor).await
    }
}
