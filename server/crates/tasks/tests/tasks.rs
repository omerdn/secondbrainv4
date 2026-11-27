//! Tasks, subtasks and assignees (T2.1.1, PRD G1, G2, G4) against a real Postgres 17. Requires
//! Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use items::{ActivityKind, ItemsAccessContext, ItemsService, NewItem};
use kernel::{AppError, FixedClock, PageParams, PageRequest};
use tasks::{
    CreateSubtaskRequest, CreateTaskRequest, TaskProgress, TaskView, TasksService,
    UpdateSubtaskRequest, UpdateTaskRequest, repo,
};
use tracking::{DateRange, ProgressProvider};
use uuid::Uuid;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 23, 12, 0, 0).unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn request(title: &str) -> CreateTaskRequest {
    CreateTaskRequest {
        title: title.into(),
        description: None,
        start_date: None,
        due_date: None,
        subtasks: None,
    }
}

fn subtask(title: &str) -> CreateSubtaskRequest {
    CreateSubtaskRequest {
        title: title.into(),
    }
}

struct World {
    db: TestDb,
    access: AccessEngine<ItemsAccessContext>,
    items: ItemsService,
    tasks: TasksService,
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
            clock,
            access.permissions(),
            items.clone(),
        );
        Self {
            db,
            access,
            items,
            tasks,
            owner: Uuid::now_v7(),
        }
    }

    async fn task(&self, subtasks: &[&str]) -> TaskView {
        let list = subtasks.iter().map(|t| subtask(t)).collect();
        self.tasks
            .create(
                self.owner,
                CreateTaskRequest {
                    subtasks: Some(list),
                    ..request("Ship")
                },
            )
            .await
            .unwrap()
    }

    /// Someone with VIEW + EDIT on the task, but not MANAGE.
    async fn collaborator(&self, task: Uuid) -> Uuid {
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

    async fn assign_directly(&self, subtask: Uuid, user: Uuid) {
        let mut conn = self.db.pool().acquire().await.unwrap();
        repo::add_assignee(&mut conn, subtask, user, now())
            .await
            .unwrap();
    }

    async fn activity_count(&self, task: Uuid, kind: ActivityKind) -> usize {
        let page = PageRequest::from_params(PageParams {
            limit: Some(100),
            cursor: None,
        })
        .unwrap();
        self.items
            .item_activity(self.owner, task, &page)
            .await
            .unwrap()
            .items
            .iter()
            .filter(|a| a.kind == kind)
            .count()
    }
}

#[tokio::test]
async fn g1_every_field_is_in_the_api() {
    let w = World::new().await;
    let task = w
        .tasks
        .create(
            w.owner,
            CreateTaskRequest {
                description: Some("Details".into()),
                start_date: Some(day(1)),
                due_date: Some(day(9)),
                ..request("Ship")
            },
        )
        .await
        .unwrap();
    let tag = w
        .items
        .create_tag(
            w.owner,
            items::CreateTagRequest {
                name: "work".into(),
                color: None,
            }
            .validate()
            .unwrap(),
        )
        .await
        .unwrap();
    w.items
        .attach_tag(w.owner, task.item.id, tag.id)
        .await
        .unwrap();
    let page = w
        .items
        .create(
            w.owner,
            NewItem {
                kind: "page".into(),
                title: "Notes".into(),
                description: None,
            },
        )
        .await
        .unwrap();
    w.items.link(w.owner, page.id, task.item.id).await.unwrap();

    let read = w.tasks.task(w.owner, task.item.id).await.unwrap();
    assert_eq!(read.item.kind, "task");
    assert_eq!(read.item.title, "Ship");
    assert_eq!(read.item.description.as_deref(), Some("Details"));
    assert_eq!(
        (read.start_date, read.due_date),
        (Some(day(1)), Some(day(9)))
    );
    assert_eq!(read.tags.len(), 1);
    assert_eq!(read.pages.len(), 1);
    assert_eq!(read.pages[0].item.id, page.id);
    assert_eq!(read.member_ids, [w.owner]);
}

#[tokio::test]
async fn g2_a_task_without_subtasks_gets_one_from_its_title() {
    let w = World::new().await;
    let task = w.tasks.create(w.owner, request(" Ship ")).await.unwrap();
    assert_eq!(task.subtasks.len(), 1);
    assert_eq!(task.subtasks[0].title, "Ship");

    let empty = CreateTaskRequest {
        subtasks: Some(vec![]),
        ..request("Plan")
    };
    let task = w.tasks.create(w.owner, empty).await.unwrap();
    assert_eq!(task.subtasks[0].title, "Plan");
}

#[tokio::test]
async fn g2_given_subtasks_are_kept_in_order() {
    let w = World::new().await;
    let task = w.task(&["a", "b", "c"]).await;
    let titles: Vec<&str> = task.subtasks.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["a", "b", "c"]);
    let added = w
        .tasks
        .add_subtask(w.owner, task.item.id, subtask(" d "))
        .await
        .unwrap();
    assert_eq!((added.title.as_str(), added.position), ("d", 3));
}

#[tokio::test]
async fn g2_the_last_subtask_cannot_be_deleted() {
    let w = World::new().await;
    let task = w.task(&["a", "b"]).await;
    let id = task.item.id;
    w.tasks
        .delete_subtask(w.owner, id, task.subtasks[0].id)
        .await
        .unwrap();
    let last = w
        .tasks
        .delete_subtask(w.owner, id, task.subtasks[1].id)
        .await;
    assert!(matches!(last, Err(AppError::Conflict(_))));
    assert_eq!(w.tasks.task(w.owner, id).await.unwrap().subtasks.len(), 1);
}

#[tokio::test]
async fn g2_concurrent_deletes_keep_one_subtask() {
    let w = World::new().await;
    let task = w.task(&["a", "b"]).await;
    let id = task.item.id;
    let (first, second) = tokio::join!(
        w.tasks.delete_subtask(w.owner, id, task.subtasks[0].id),
        w.tasks.delete_subtask(w.owner, id, task.subtasks[1].id),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(w.tasks.task(w.owner, id).await.unwrap().subtasks.len(), 1);
}

#[tokio::test]
async fn g4_one_assignee_completes_the_subtask() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    let sub = task.subtasks[0].id;
    let (first, second) = (w.collaborator(id).await, w.collaborator(id).await);
    w.assign_directly(sub, first).await;
    w.assign_directly(sub, second).await;

    let done = w.tasks.complete_subtask(second, id, sub).await.unwrap();
    assert_eq!(done.done_by, Some(second));
    assert_eq!(done.done_at, Some(now()));
    assert_eq!(done.assignee_ids.len(), 2);
}

#[tokio::test]
async fn g4_someone_who_is_not_assigned_cannot_mark_it() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    let sub = task.subtasks[0].id;
    let (assignee, other) = (w.collaborator(id).await, w.collaborator(id).await);
    w.assign_directly(sub, assignee).await;

    let refused = w.tasks.complete_subtask(other, id, sub).await;
    assert!(matches!(refused, Err(AppError::Forbidden)));
    // The owner manages the task.
    assert!(w.tasks.complete_subtask(w.owner, id, sub).await.is_ok());
    let refused = w.tasks.reopen_subtask(other, id, sub).await;
    assert!(matches!(refused, Err(AppError::Forbidden)));
}

#[tokio::test]
async fn completing_twice_keeps_the_first_person_and_one_event() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    let sub = task.subtasks[0].id;
    let collaborator = w.collaborator(id).await;

    w.tasks
        .complete_subtask(collaborator, id, sub)
        .await
        .unwrap();
    let again = w.tasks.complete_subtask(w.owner, id, sub).await.unwrap();
    assert_eq!(again.done_by, Some(collaborator));
    assert_eq!(w.activity_count(id, ActivityKind::Completed).await, 1);

    let open = w.tasks.reopen_subtask(w.owner, id, sub).await.unwrap();
    assert_eq!((open.done_at, open.done_by), (None, None));
    let open = w.tasks.reopen_subtask(w.owner, id, sub).await.unwrap();
    assert_eq!(open.done_at, None);
}

#[tokio::test]
async fn a1_the_task_is_completed_with_its_last_subtask() {
    let w = World::new().await;
    let task = w.task(&["a", "b"]).await;
    let id = task.item.id;
    w.tasks
        .complete_subtask(w.owner, id, task.subtasks[0].id)
        .await
        .unwrap();
    assert_eq!(w.activity_count(id, ActivityKind::Completed).await, 0);
    w.tasks
        .complete_subtask(w.owner, id, task.subtasks[1].id)
        .await
        .unwrap();
    assert_eq!(w.activity_count(id, ActivityKind::Completed).await, 1);
}

#[tokio::test]
async fn assignment_is_repeatable_and_one_can_leave_it() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    let sub = task.subtasks[0].id;

    w.tasks.assign(w.owner, id, sub, w.owner).await.unwrap();
    let again = w.tasks.assign(w.owner, id, sub, w.owner).await.unwrap();
    assert_eq!(again.assignee_ids, [w.owner]);
    assert_eq!(w.activity_count(id, ActivityKind::Assigned).await, 1);

    let removed = w.tasks.unassign(w.owner, id, sub, w.owner).await.unwrap();
    assert!(removed.assignee_ids.is_empty());
    assert!(w.tasks.unassign(w.owner, id, sub, w.owner).await.is_ok());
}

#[tokio::test]
async fn g4_the_owner_assigns_several_members_and_one_of_them_completes() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    let sub = task.subtasks[0].id;
    let (ayse, can) = (w.collaborator(id).await, w.collaborator(id).await);

    w.tasks.assign(w.owner, id, sub, ayse).await.unwrap();
    let both = w.tasks.assign(w.owner, id, sub, can).await.unwrap();
    let mut assigned = both.assignee_ids.clone();
    assigned.sort();
    let mut expected = vec![ayse, can];
    expected.sort();
    assert_eq!(assigned, expected);

    let done = w.tasks.complete_subtask(can, id, sub).await.unwrap();
    assert_eq!(done.done_by, Some(can));
    // The owner takes someone off; a member may only take themselves off.
    assert!(matches!(
        w.tasks.unassign(ayse, id, sub, can).await,
        Err(AppError::Forbidden)
    ));
    let left = w.tasks.unassign(w.owner, id, sub, can).await.unwrap();
    assert_eq!(left.assignee_ids, [ayse]);
    let left = w.tasks.unassign(ayse, id, sub, ayse).await.unwrap();
    assert!(left.assignee_ids.is_empty());
}

#[tokio::test]
async fn only_members_who_work_on_the_task_are_assigned() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    let sub = task.subtasks[0].id;
    let ayse = w.collaborator(id).await;

    // A stranger, and a watcher who only looks on (N2): not assignable.
    let stranger = Uuid::now_v7();
    assert!(matches!(
        w.tasks.assign(w.owner, id, sub, stranger).await,
        Err(AppError::Validation(_))
    ));
    let watcher = Uuid::now_v7();
    let mut conn = w.db.pool().acquire().await.unwrap();
    w.access
        .grant(
            &mut conn,
            RelationTuple::new(id, Relation::Watcher, watcher),
        )
        .await
        .unwrap();
    drop(conn);
    assert!(matches!(
        w.tasks.assign(w.owner, id, sub, watcher).await,
        Err(AppError::Validation(_))
    ));
    // A member without MANAGE assigns only themselves.
    assert!(matches!(
        w.tasks.assign(ayse, id, sub, w.owner).await,
        Err(AppError::Forbidden)
    ));
    w.tasks.assign(ayse, id, sub, ayse).await.unwrap();
    let view = w.tasks.task(w.owner, id).await.unwrap();
    assert_eq!(view.subtasks[0].assignee_ids, [ayse]);
}

#[tokio::test]
async fn dates_change_clear_and_stay_in_order() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    let set = UpdateTaskRequest {
        start_date: Some(Some(day(2))),
        due_date: Some(Some(day(5))),
    };
    let updated = w.tasks.update(w.owner, id, set).await.unwrap();
    assert_eq!(updated.due_date, Some(day(5)));

    let before_start = UpdateTaskRequest {
        due_date: Some(Some(day(1))),
        ..UpdateTaskRequest::default()
    };
    let refused = w.tasks.update(w.owner, id, before_start).await;
    assert!(matches!(refused, Err(AppError::Validation(_))));

    let clear = UpdateTaskRequest {
        due_date: Some(None),
        ..UpdateTaskRequest::default()
    };
    let cleared = w.tasks.update(w.owner, id, clear).await.unwrap();
    assert_eq!((cleared.start_date, cleared.due_date), (Some(day(2)), None));

    let nothing = w
        .tasks
        .update(w.owner, id, UpdateTaskRequest::default())
        .await;
    assert!(matches!(nothing, Err(AppError::Validation(_))));
}

#[tokio::test]
async fn v9_a_stranger_gets_not_found_everywhere() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    let sub = task.subtasks[0].id;
    let stranger = Uuid::now_v7();

    assert!(matches!(
        w.tasks.task(stranger, id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.tasks
            .update(stranger, id, UpdateTaskRequest::default())
            .await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.tasks.add_subtask(stranger, id, subtask("x")).await,
        Err(AppError::NotFound)
    ));
    let rename = UpdateSubtaskRequest { title: "x".into() };
    assert!(matches!(
        w.tasks.rename_subtask(stranger, id, sub, rename).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.tasks.delete_subtask(stranger, id, sub).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.tasks.complete_subtask(stranger, id, sub).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.tasks.reopen_subtask(stranger, id, sub).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.tasks.assign(stranger, id, sub, stranger).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.tasks.unassign(stranger, id, sub, stranger).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn a_subtask_of_another_task_is_not_found() {
    let w = World::new().await;
    let mine = w.task(&["a"]).await;
    let other = w.task(&["b"]).await;
    let result = w
        .tasks
        .complete_subtask(w.owner, mine.item.id, other.subtasks[0].id)
        .await;
    assert!(matches!(result, Err(AppError::NotFound)));
}

#[tokio::test]
async fn an_item_of_another_kind_is_not_a_task() {
    let w = World::new().await;
    let goal = w
        .items
        .create(
            w.owner,
            NewItem {
                kind: "goal".into(),
                title: "Goal".into(),
                description: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        w.tasks.task(w.owner, goal.id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.tasks.add_subtask(w.owner, goal.id, subtask("x")).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn a_task_in_the_trash_cannot_change() {
    let w = World::new().await;
    let task = w.task(&["a"]).await;
    let id = task.item.id;
    w.items.set_trashed(w.owner, id, true).await.unwrap();
    assert!(matches!(
        w.tasks
            .complete_subtask(w.owner, id, task.subtasks[0].id)
            .await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn invalid_requests_are_refused() {
    let w = World::new().await;
    let reversed = CreateTaskRequest {
        start_date: Some(day(9)),
        due_date: Some(day(1)),
        ..request("Ship")
    };
    assert!(matches!(
        w.tasks.create(w.owner, reversed).await,
        Err(AppError::Validation(_))
    ));
    let blank = CreateTaskRequest {
        subtasks: Some(vec![subtask("  ")]),
        ..request("Ship")
    };
    assert!(matches!(
        w.tasks.create(w.owner, blank).await,
        Err(AppError::Validation(_))
    ));
    let too_many = CreateTaskRequest {
        subtasks: Some(vec![subtask("x"); tasks::SUBTASK_LIMIT + 1]),
        ..request("Ship")
    };
    assert!(matches!(
        w.tasks.create(w.owner, too_many).await,
        Err(AppError::Validation(_))
    ));
}

#[tokio::test]
async fn g3_the_provider_reads_done_over_total_and_the_task_dates() {
    let w = World::new().await;
    let task = w
        .tasks
        .create(
            w.owner,
            CreateTaskRequest {
                start_date: Some(day(1)),
                due_date: Some(day(10)),
                subtasks: Some(vec![subtask("a"), subtask("b"), subtask("c"), subtask("d")]),
                ..request("Ship")
            },
        )
        .await
        .unwrap();
    let id = task.item.id;
    w.tasks
        .complete_subtask(w.owner, id, task.subtasks[0].id)
        .await
        .unwrap();
    let provider = TaskProgress::new(w.db.pool().clone());
    let range = DateRange::new(day(1), day(30)).unwrap();
    let progress = provider.progress(id, range, day(5)).await.unwrap();
    assert!((progress.done - 1.0).abs() < f64::EPSILON);
    assert!((progress.total - 4.0).abs() < f64::EPSILON);
    assert!((progress.expected_to_date - 2.0).abs() < 1e-9);
    assert!((tasks::completion(1, 4) - progress.done / progress.total).abs() < f64::EPSILON);
}

#[tokio::test]
async fn the_provider_does_not_know_other_items() {
    let w = World::new().await;
    let provider = TaskProgress::new(w.db.pool().clone());
    let range = DateRange::new(day(1), day(30)).unwrap();
    assert!(matches!(
        provider.progress(Uuid::now_v7(), range, day(5)).await,
        Err(AppError::NotFound)
    ));
}

impl World {
    /// The ids of the owner's items whose status is `status` (F1).
    async fn with_status(&self, status: &str) -> Vec<Uuid> {
        let filter = items::ItemFilter {
            statuses: Some(vec![status.into()]),
            ..items::ItemFilter::default()
        };
        let page = PageRequest::from_params(PageParams {
            limit: Some(100),
            cursor: None,
        })
        .unwrap();
        self.items
            .filter_items(self.owner, &filter, &page)
            .await
            .unwrap()
            .items
            .iter()
            .map(|item| item.id)
            .collect()
    }
}

#[tokio::test]
async fn f1_a_tasks_status_follows_its_subtasks() {
    let w = World::new().await;
    let task = w.task(&["One", "Two"]).await;
    let id = task.item.id;
    let (one, two) = (task.subtasks[0].id, task.subtasks[1].id);
    assert_eq!(w.with_status("open").await, vec![id]);
    assert!(w.with_status("done").await.is_empty());

    w.tasks.complete_subtask(w.owner, id, one).await.unwrap();
    assert_eq!(w.with_status("open").await, vec![id], "one subtask is left");
    w.tasks.complete_subtask(w.owner, id, two).await.unwrap();
    assert_eq!(
        w.with_status("done").await,
        vec![id],
        "G3: every subtask is done"
    );
    assert!(w.with_status("open").await.is_empty());

    w.tasks.reopen_subtask(w.owner, id, two).await.unwrap();
    assert_eq!(w.with_status("open").await, vec![id]);
    w.tasks.complete_subtask(w.owner, id, two).await.unwrap();

    let three = w
        .tasks
        .add_subtask(w.owner, id, subtask("Three"))
        .await
        .unwrap();
    assert_eq!(
        w.with_status("open").await,
        vec![id],
        "a new subtask is open"
    );
    w.tasks.delete_subtask(w.owner, id, three.id).await.unwrap();
    assert_eq!(
        w.with_status("done").await,
        vec![id],
        "the open one is gone"
    );
}

#[tokio::test]
async fn t3_0_3_the_migration_fills_existing_tasks() {
    let w = World::new().await;
    let open = w.task(&["One", "Two"]).await;
    let done = w.task(&["Only"]).await;
    w.tasks
        .complete_subtask(w.owner, done.item.id, done.subtasks[0].id)
        .await
        .unwrap();
    // As before T3.0.3: no task had a status.
    sqlx::query("UPDATE items SET status = NULL")
        .execute(w.db.pool())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261127000700_task_item_status.sql"
    ))
    .execute(w.db.pool())
    .await
    .unwrap();
    assert_eq!(w.with_status("open").await, vec![open.item.id]);
    assert_eq!(w.with_status("done").await, vec![done.item.id]);
}
