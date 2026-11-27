//! A goal's progress from real tasks and routines (T2.3.2, PRD D3-D6), against Postgres 17: the
//! modules measure their own items through `ProgressProvider` and a routine only counts inside
//! the goal's range (D5). Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{Duration, NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use goals::{CreateGoalRequest, GoalProgressQuery, GoalsService, LinkGoalRequest};
use items::{ItemsAccessContext, ItemsService};
use kernel::FixedClock;
use routines::{CompleteRoutineRequest, CreateRoutineRequest, RoutinesService};
use tasks::{CreateSubtaskRequest, CreateTaskRequest, TasksService};
use uuid::Uuid;

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[tokio::test]
async fn d5_a_routine_counts_only_inside_the_goal_and_tasks_weigh_in() {
    let db = TestDb::new().await;
    // 22 November, noon UTC.
    let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap());
    let shared: kernel::SharedClock = Arc::new(clock.clone());
    let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
    let pool = db.pool().clone();
    let items = ItemsService::new(pool.clone(), shared.clone(), access.clone());
    let routines = RoutinesService::new(
        pool.clone(),
        shared.clone(),
        access.permissions(),
        items.clone(),
    );
    let tasks = TasksService::new(
        pool.clone(),
        shared.clone(),
        access.permissions(),
        items.clone(),
    );
    let goals = GoalsService::new(pool.clone(), shared, access.permissions(), items)
        .with_progress("task", Arc::new(tasks::TaskProgress::new(pool.clone())))
        .with_progress(
            "routine",
            Arc::new(routines::RoutineProgress::new(pool.clone())),
        )
        .with_progress(
            "tracker",
            Arc::new(tracking::TrackerProgress::new(pool.clone())),
        );
    let me = Uuid::now_v7();

    // A daily routine from the 20th, done every day from the 20th to the 26th.
    let routine = routines
        .create(
            me,
            CreateRoutineRequest {
                title: "Su".into(),
                description: None,
                timezone: "UTC".into(),
                rrule: "FREQ=DAILY".into(),
                daily_target: Some(2),
                start_date: Some(day(20)),
                end_date: None,
                reminder_times: None,
                completion_mode: None,
                rotation: None,
            },
        )
        .await
        .unwrap()
        .item
        .id;
    for d in 20..=26 {
        clock.set(Utc.with_ymd_and_hms(2026, 11, d, 12, 0, 0).unwrap());
        // Three on each day: one more than the target, which must not spill over (§7.2).
        let three = CompleteRoutineRequest {
            date: Some(day(d)),
            count: Some(3),
        };
        routines.complete(me, routine, three).await.unwrap();
    }
    // A task with four subtasks, one done.
    let task = tasks
        .create(
            me,
            CreateTaskRequest {
                title: "Rapor".into(),
                description: None,
                start_date: None,
                due_date: None,
                subtasks: Some(
                    ["a", "b", "c", "d"]
                        .into_iter()
                        .map(|t| CreateSubtaskRequest { title: t.into() })
                        .collect(),
                ),
            },
        )
        .await
        .unwrap();
    tasks
        .complete_subtask(me, task.item.id, task.subtasks[0].id)
        .await
        .unwrap();

    // The goal runs from the 23rd to the 29th; it is the 26th.
    let goal = goals
        .create(
            me,
            CreateGoalRequest {
                title: "Kasım sonu".into(),
                description: None,
                start_date: Some(day(23)),
                end_date: Some(day(29)),
            },
        )
        .await
        .unwrap()
        .item
        .id;
    goals
        .link(me, goal, routine, LinkGoalRequest::default())
        .await
        .unwrap();
    goals
        .link(me, goal, task.item.id, LinkGoalRequest::default())
        .await
        .unwrap();
    clock.advance(Duration::hours(1));
    let p = goals
        .progress(
            me,
            goal,
            GoalProgressQuery {
                today: Some(day(26)),
            },
        )
        .await
        .unwrap();
    let routine_part = p.items.iter().find(|i| i.item.id == routine).unwrap();
    let task_part = p.items.iter().find(|i| i.item.id == task.item.id).unwrap();
    // D5: only the 23rd to the 29th count: 7 occurrences × 2. Days 20-22 are outside.
    assert!(close(routine_part.total, 14.0));
    // 23-26 done, each capped at its target of 2.
    assert!(close(routine_part.done, 8.0));
    assert!(close(routine_part.expected_to_date, 8.0));
    // The task: one of four, expected to grow linearly over the goal's range.
    assert!(close(task_part.done, 1.0));
    assert!(close(task_part.total, 4.0));
    assert!(close(task_part.expected, 4.0 / 7.0));
    // D3 with equal weights, D4 both indicators.
    let actual = (8.0 / 14.0 + 0.25) / 2.0;
    assert!(close(p.actual, actual));
    assert!(close(p.expected, (8.0 / 14.0 + 4.0 / 7.0) / 2.0));
    assert!(close(p.pace, p.actual / p.expected));
}
