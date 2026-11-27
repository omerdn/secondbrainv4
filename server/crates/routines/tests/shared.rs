//! Shared routines: completion modes and the shared streak (T3.3.1, PRD R5, R10) against a real
//! Postgres 17. Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService};
use kernel::FixedClock;
use routines::counts::CompletionMode;
use routines::{
    CompleteRoutineRequest, CreateRoutineRequest, RoutinesService, UpdateRoutineRequest,
};
use uuid::Uuid;

/// Friday 20 November 2026, noon in Istanbul.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn on(date: NaiveDate) -> CompleteRoutineRequest {
    CompleteRoutineRequest {
        date: Some(date),
        count: None,
    }
}

struct World {
    db: TestDb,
    access: AccessEngine<ItemsAccessContext>,
    routines: RoutinesService,
    ali: Uuid,
    ayse: Uuid,
    can: Uuid,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let clock = Arc::new(FixedClock::new(now()));
        let items = ItemsService::new(db.pool().clone(), clock.clone(), access.clone());
        let routines = RoutinesService::new(db.pool().clone(), clock, access.permissions(), items);
        Self {
            db,
            access,
            routines,
            ali: Uuid::now_v7(),
            ayse: Uuid::now_v7(),
            can: Uuid::now_v7(),
        }
    }

    /// Ali's daily routine with `target`, shared with Ayşe and Can as assignees.
    async fn shared(&self, mode: Option<CompletionMode>, target: i32) -> Uuid {
        let id = self
            .routines
            .create(
                self.ali,
                CreateRoutineRequest {
                    title: "Bulaşık".into(),
                    description: None,
                    timezone: "Europe/Istanbul".into(),
                    rrule: "FREQ=DAILY".into(),
                    daily_target: Some(target),
                    start_date: Some(day(15)),
                    end_date: None,
                    reminder_times: None,
                    completion_mode: mode,
                    rotation: None,
                },
            )
            .await
            .unwrap()
            .item
            .id;
        let mut tx = self.db.pool().begin().await.unwrap();
        for person in [self.ayse, self.can] {
            self.access
                .grant(&mut tx, RelationTuple::new(id, Relation::Assignee, person))
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        id
    }

    async fn mark(&self, who: Uuid, id: Uuid, date: NaiveDate) -> routines::RoutineDay {
        self.routines.complete(who, id, on(date)).await.unwrap()
    }
}

#[tokio::test]
async fn r5_in_a_shared_pool_one_person_is_enough_for_target_one() {
    let w = World::new().await;
    let id = w.shared(None, 1).await;
    assert_eq!(
        w.routines.routine(w.ali, id).await.unwrap().completion_mode,
        CompletionMode::Pool
    );
    let day = w.mark(w.can, id, day(20)).await;
    assert!(day.complete);
    assert_eq!((day.done, day.target), (1, 1));
}

#[tokio::test]
async fn r5_in_a_pool_everyone_adds_to_one_counter() {
    let w = World::new().await;
    let id = w.shared(Some(CompletionMode::Pool), 3).await;
    w.mark(w.ayse, id, day(20)).await;
    w.mark(w.can, id, day(20)).await;
    let day = w.mark(w.ayse, id, day(20)).await;
    assert_eq!((day.done, day.target, day.complete), (3, 3, true));
}

#[tokio::test]
async fn r5_each_person_must_reach_the_target_on_their_own() {
    let w = World::new().await;
    let id = w.shared(Some(CompletionMode::Individual), 1).await;
    let after_ayse = w.mark(w.ayse, id, day(20)).await;
    // Two people, target 1 each: half done.
    assert_eq!((after_ayse.done, after_ayse.target), (1, 2));
    assert!(!after_ayse.complete);
    // Ayşe doing it again does not cover Can.
    let again = w.mark(w.ayse, id, day(20)).await;
    assert_eq!((again.done, again.complete), (1, false));
    // The owner is not an assignee here: his mark counts for nobody.
    let owner = w.mark(w.ali, id, day(20)).await;
    assert!(!owner.complete);
    let both = w.mark(w.can, id, day(20)).await;
    assert_eq!((both.done, both.target, both.complete), (2, 2, true));
    // Taking Can's mark back opens the day again.
    let undone = w
        .routines
        .undo_completion(
            w.can,
            id,
            routines::UndoCompletionQuery {
                date: Some(day(20)),
            },
        )
        .await
        .unwrap();
    assert!(!undone.complete);
}

#[tokio::test]
async fn r10_the_streak_is_shared_and_follows_the_mode() {
    let w = World::new().await;
    let id = w.shared(Some(CompletionMode::Individual), 1).await;
    for d in 17..=19 {
        w.mark(w.ayse, id, day(d)).await;
        w.mark(w.can, id, day(d)).await;
    }
    // 17–19 complete for both: everyone sees the same streak (R10).
    for who in [w.ali, w.ayse, w.can] {
        let streak = w.routines.streak(who, id).await.unwrap();
        assert_eq!((streak.current, streak.today_pending), (3, true));
    }
    // Switching to a pool reads the same days under the new mode.
    let pool = UpdateRoutineRequest {
        completion_mode: Some(CompletionMode::Pool),
        ..UpdateRoutineRequest::default()
    };
    w.routines.update(w.ali, id, pool).await.unwrap();
    w.mark(w.ayse, id, day(20)).await;
    let streak = w.routines.streak(w.can, id).await.unwrap();
    assert_eq!((streak.current, streak.today_pending), (4, false));
    let days = w
        .routines
        .days(
            w.ali,
            id,
            routines::RoutineDaysQuery {
                from: Some(day(16)),
                to: Some(day(20)),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        days.iter().map(|d| d.complete).collect::<Vec<_>>(),
        [false, true, true, true, true]
    );
}

#[tokio::test]
async fn r5_a_personal_routine_counts_its_owner() {
    let w = World::new().await;
    let id = w
        .routines
        .create(
            w.ali,
            CreateRoutineRequest {
                title: "Kitap".into(),
                description: None,
                timezone: "Europe/Istanbul".into(),
                rrule: "FREQ=DAILY".into(),
                daily_target: Some(2),
                start_date: Some(day(15)),
                end_date: None,
                reminder_times: None,
                completion_mode: Some(CompletionMode::Individual),
                rotation: None,
            },
        )
        .await
        .unwrap()
        .item
        .id;
    w.mark(w.ali, id, day(20)).await;
    let day = w.mark(w.ali, id, day(20)).await;
    assert_eq!((day.done, day.target, day.complete), (2, 2, true));
}
