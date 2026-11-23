//! The personal kanban (T2.1.3, PRD G6, ADR-0014) against a real Postgres 17. Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use chrono::{DateTime, Duration, TimeZone, Utc};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService, NewItem};
use kernel::{AppError, FixedClock};
use tasks::{
    Board, CreateBoardColumnRequest, CreateTaskRequest, MoveBoardTaskRequest, TasksService,
    UpdateBoardColumnRequest,
};
use uuid::Uuid;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 23, 12, 0, 0).unwrap()
}

fn new_task(title: &str) -> CreateTaskRequest {
    CreateTaskRequest {
        title: title.into(),
        description: None,
        start_date: None,
        due_date: None,
        subtasks: None,
    }
}

fn titles(board: &Board, column: usize) -> Vec<&str> {
    board.columns[column]
        .cards
        .iter()
        .map(|card| card.title.as_str())
        .collect()
}

fn names(board: &Board) -> Vec<&str> {
    board.columns.iter().map(|c| c.name.as_str()).collect()
}

struct World {
    db: TestDb,
    access: AccessEngine<ItemsAccessContext>,
    items: ItemsService,
    tasks: TasksService,
    clock: Arc<FixedClock>,
    owner: Uuid,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let clock = Arc::new(FixedClock::new(now()));
        let items = ItemsService::new(db.pool().clone(), clock.clone(), access.clone());
        let tasks = TasksService::new(
            db.pool().clone(),
            clock.clone(),
            access.permissions(),
            items.clone(),
        );
        Self {
            db,
            access,
            items,
            tasks,
            clock,
            owner: Uuid::now_v7(),
        }
    }

    async fn task(&self, title: &str) -> Uuid {
        self.tasks
            .create(self.owner, new_task(title))
            .await
            .unwrap()
            .item
            .id
    }

    async fn column(&self, user: Uuid, name: &str) -> Uuid {
        let board = self
            .tasks
            .create_column(user, CreateBoardColumnRequest { name: name.into() })
            .await
            .unwrap();
        board.columns.last().unwrap().id
    }

    async fn share(&self, task: Uuid) -> Uuid {
        let user = Uuid::now_v7();
        let mut conn = self.db.pool().acquire().await.unwrap();
        self.access
            .grant(
                &mut conn,
                RelationTuple::new(task, Relation::Assignee, user),
            )
            .await
            .unwrap();
        user
    }

    async fn place(&self, user: Uuid, task: Uuid, column: Uuid, after: Option<Uuid>) -> Board {
        self.tasks
            .move_card(
                user,
                task,
                MoveBoardTaskRequest {
                    column_id: column,
                    after_task_id: after,
                },
            )
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn a_new_task_lands_in_the_inbox() {
    let w = World::new().await;
    w.task("first").await;
    w.task("second").await;
    let board = w.tasks.board(w.owner).await.unwrap();
    assert_eq!(board.columns.len(), 1);
    assert!(board.columns[0].is_inbox);
    assert_eq!(titles(&board, 0), ["first", "second"]);
    assert_eq!(board.columns[0].cards[0].subtasks_total, 1);
}

#[tokio::test]
async fn an_empty_board_has_an_inbox() {
    let w = World::new().await;
    let board = w.tasks.board(w.owner).await.unwrap();
    assert_eq!(board.columns.len(), 1);
    assert!(board.columns[0].is_inbox);
}

#[tokio::test]
async fn g6_one_members_placement_does_not_affect_another() {
    let w = World::new().await;
    let task = w.task("shared").await;
    let member = w.share(task).await;
    let owner_column = w.column(w.owner, "Doing").await;
    let member_column = w.column(member, "Later").await;

    w.place(w.owner, task, owner_column, None).await;
    let member_board = w.tasks.board(member).await.unwrap();
    assert_eq!(titles(&member_board, 0), ["shared"], "still in their inbox");
    assert!(titles(&member_board, 1).is_empty());

    w.place(member, task, member_column, None).await;
    let owner_board = w.tasks.board(w.owner).await.unwrap();
    assert_eq!(names(&owner_board), ["Inbox", "Doing"]);
    assert!(titles(&owner_board, 0).is_empty());
    assert_eq!(titles(&owner_board, 1), ["shared"]);
}

#[tokio::test]
async fn g6_a_shared_task_lands_in_the_new_members_first_column() {
    let w = World::new().await;
    let task = w.task("shared").await;
    let member = w.share(task).await;
    let board = w.tasks.board(member).await.unwrap();
    assert!(board.columns[0].is_inbox);
    assert_eq!(titles(&board, 0), ["shared"]);
}

#[tokio::test]
async fn g6_a_move_does_not_change_the_task_or_its_progress() {
    let w = World::new().await;
    let task = w.task("quiet").await;
    let before = w.tasks.task(w.owner, task).await.unwrap();
    let column = w.column(w.owner, "Done").await;
    w.clock.advance(Duration::days(2));
    w.place(w.owner, task, column, None).await;
    let after = w.tasks.task(w.owner, task).await.unwrap();
    assert_eq!(after.item.last_touched_at, before.item.last_touched_at);
    assert_eq!(after.item.updated_at, before.item.updated_at);
    assert_eq!(
        after.subtasks, before.subtasks,
        "a column is not completion"
    );
}

#[tokio::test]
async fn cards_keep_the_order_they_are_dropped_in() {
    let w = World::new().await;
    let (a, b, c) = (w.task("a").await, w.task("b").await, w.task("c").await);
    let inbox = w.tasks.board(w.owner).await.unwrap().columns[0].id;

    let board = w.place(w.owner, c, inbox, None).await;
    assert_eq!(titles(&board, 0), ["c", "a", "b"]);
    let board = w.place(w.owner, b, inbox, Some(c)).await;
    assert_eq!(titles(&board, 0), ["c", "b", "a"]);
    let board = w.place(w.owner, c, inbox, Some(a)).await;
    assert_eq!(titles(&board, 0), ["b", "a", "c"]);
}

#[tokio::test]
async fn many_moves_to_the_same_spot_renumber_safely() {
    let w = World::new().await;
    let (a, b) = (w.task("a").await, w.task("b").await);
    let inbox = w.tasks.board(w.owner).await.unwrap().columns[0].id;
    for round in 0..600 {
        let (mover, other) = if round % 2 == 0 { (b, a) } else { (a, b) };
        // Always squeeze between the top of the column and the other card.
        let board = w.place(w.owner, mover, inbox, None).await;
        assert_eq!(board.columns[0].cards[0].task_id, mover);
        assert_eq!(board.columns[0].cards[1].task_id, other);
    }
    let longest: Option<i32> = sqlx::query_scalar(
        "SELECT max(char_length(position)) FROM user_task_placements WHERE user_id = $1",
    )
    .bind(w.owner)
    .fetch_one(w.db.pool())
    .await
    .unwrap();
    assert!(longest.unwrap() <= 65);
}

#[tokio::test]
async fn columns_are_renamed_and_reordered_with_the_inbox_first() {
    let w = World::new().await;
    let first = w.column(w.owner, "One").await;
    let second = w.column(w.owner, "Two").await;
    let inbox = w.tasks.board(w.owner).await.unwrap().columns[0].id;

    let board = w
        .tasks
        .update_column(
            w.owner,
            second,
            UpdateBoardColumnRequest {
                name: Some(" Second ".into()),
                after_column_id: Some(inbox),
            },
        )
        .await
        .unwrap();
    assert_eq!(names(&board), ["Inbox", "Second", "One"]);

    let board = w
        .tasks
        .update_column(
            w.owner,
            second,
            UpdateBoardColumnRequest {
                name: None,
                after_column_id: Some(first),
            },
        )
        .await
        .unwrap();
    assert_eq!(names(&board), ["Inbox", "One", "Second"]);

    let moved = w
        .tasks
        .update_column(
            w.owner,
            inbox,
            UpdateBoardColumnRequest {
                name: None,
                after_column_id: Some(first),
            },
        )
        .await;
    assert!(matches!(moved, Err(AppError::Conflict(_))));
}

#[tokio::test]
async fn deleting_a_column_moves_its_cards_to_the_inbox() {
    let w = World::new().await;
    let (a, b) = (w.task("a").await, w.task("b").await);
    let column = w.column(w.owner, "Doing").await;
    w.place(w.owner, a, column, None).await;
    w.place(w.owner, b, column, Some(a)).await;
    let c = w.task("c").await;

    w.tasks.delete_column(w.owner, column).await.unwrap();
    let board = w.tasks.board(w.owner).await.unwrap();
    assert_eq!(names(&board), ["Inbox"]);
    assert_eq!(titles(&board, 0), ["c", "a", "b"]);
    assert_eq!(board.columns[0].cards[0].task_id, c);
}

#[tokio::test]
async fn the_inbox_cannot_be_deleted() {
    let w = World::new().await;
    let inbox = w.tasks.board(w.owner).await.unwrap().columns[0].id;
    assert!(matches!(
        w.tasks.delete_column(w.owner, inbox).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn v9_someone_elses_board_is_not_found() {
    let w = World::new().await;
    let task = w.task("mine").await;
    let column = w.column(w.owner, "Mine").await;
    let stranger = Uuid::now_v7();
    let their_column = w.column(stranger, "Theirs").await;

    // A task they cannot see, in their own column.
    let hidden = w
        .tasks
        .move_card(
            stranger,
            task,
            MoveBoardTaskRequest {
                column_id: their_column,
                after_task_id: None,
            },
        )
        .await;
    assert!(matches!(hidden, Err(AppError::NotFound)));

    // Someone else's column.
    let member = w.share(task).await;
    let foreign = w
        .tasks
        .move_card(
            member,
            task,
            MoveBoardTaskRequest {
                column_id: column,
                after_task_id: None,
            },
        )
        .await;
    assert!(matches!(foreign, Err(AppError::NotFound)));
    assert!(matches!(
        w.tasks.delete_column(stranger, column).await,
        Err(AppError::NotFound)
    ));
    let rename = UpdateBoardColumnRequest {
        name: Some("x".into()),
        after_column_id: None,
    };
    assert!(matches!(
        w.tasks.update_column(stranger, column, rename).await,
        Err(AppError::NotFound)
    ));
    let board = w.tasks.board(stranger).await.unwrap();
    assert!(board.columns.iter().all(|c| c.cards.is_empty()));
}

#[tokio::test]
async fn v10_a_task_that_lost_access_leaves_the_board() {
    let w = World::new().await;
    let task = w.task("shared").await;
    let member = w.share(task).await;
    assert_eq!(titles(&w.tasks.board(member).await.unwrap(), 0), ["shared"]);
    let mut conn = w.db.pool().acquire().await.unwrap();
    w.access
        .revoke(
            &mut conn,
            RelationTuple::new(task, Relation::Assignee, member),
        )
        .await
        .unwrap();
    drop(conn);
    assert!(titles(&w.tasks.board(member).await.unwrap(), 0).is_empty());
}

#[tokio::test]
async fn trashed_tasks_leave_the_board_and_cannot_move() {
    let w = World::new().await;
    let task = w.task("gone").await;
    let inbox = w.tasks.board(w.owner).await.unwrap().columns[0].id;
    w.items.set_trashed(w.owner, task, true).await.unwrap();
    assert!(titles(&w.tasks.board(w.owner).await.unwrap(), 0).is_empty());
    let moved = w
        .tasks
        .move_card(
            w.owner,
            task,
            MoveBoardTaskRequest {
                column_id: inbox,
                after_task_id: None,
            },
        )
        .await;
    assert!(matches!(moved, Err(AppError::Conflict(_))));
}

#[tokio::test]
async fn an_item_without_a_task_row_stays_off_the_board() {
    let w = World::new().await;
    w.items
        .create(
            w.owner,
            NewItem {
                kind: "task".into(),
                title: "bare item".into(),
                description: None,
            },
        )
        .await
        .unwrap();
    let board = w.tasks.board(w.owner).await.unwrap();
    assert!(titles(&board, 0).is_empty());
}

#[tokio::test]
async fn invalid_columns_are_refused() {
    let w = World::new().await;
    for name in ["  ", &"x".repeat(61)] {
        let result = w
            .tasks
            .create_column(w.owner, CreateBoardColumnRequest { name: name.into() })
            .await;
        assert!(matches!(result, Err(AppError::Validation(_))));
    }
    for index in 1..tasks::COLUMN_LIMIT {
        w.column(w.owner, &format!("c{index}")).await;
    }
    let over = w
        .tasks
        .create_column(
            w.owner,
            CreateBoardColumnRequest {
                name: "over".into(),
            },
        )
        .await;
    assert!(matches!(over, Err(AppError::Conflict(_))));
}

#[tokio::test]
async fn concurrent_creates_all_reach_the_inbox() {
    let w = World::new().await;
    let results = tokio::join!(
        w.tasks.create(w.owner, new_task("a")),
        w.tasks.create(w.owner, new_task("b")),
        w.tasks.create(w.owner, new_task("c")),
        w.tasks.create(w.owner, new_task("d")),
    );
    assert!(results.0.is_ok() && results.1.is_ok() && results.2.is_ok() && results.3.is_ok());
    let board = w.tasks.board(w.owner).await.unwrap();
    assert_eq!(board.columns[0].cards.len(), 4);
}
