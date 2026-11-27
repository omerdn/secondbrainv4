//! Shared routine rotation and swaps (T3.3.2, PRD R6) against a real Postgres 17. Requires Docker
//! (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService};
use kernel::{AppError, FixedClock};
use routines::counts::CompletionMode;
use routines::rotation::{RotationQuery, RoutineRotation, SwapRequest};
use routines::{CreateRoutineRequest, RoutinesService, UpdateRoutineRequest};
use uuid::Uuid;

/// Friday 20 November 2026, noon in Istanbul.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn range(from: u32, to: u32) -> RotationQuery {
    RotationQuery {
        from: Some(day(from)),
        to: Some(day(to)),
    }
}

fn swap(first: u32, second: u32) -> SwapRequest {
    SwapRequest {
        first: day(first),
        second: day(second),
    }
}

struct World {
    db: TestDb,
    access: AccessEngine<ItemsAccessContext>,
    routines: RoutinesService,
    ali: Uuid,
    ayse: Uuid,
    can: Uuid,
    deniz: Uuid,
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
            deniz: Uuid::now_v7(),
        }
    }

    async fn relate(&self, id: Uuid, person: Uuid, grant: bool) {
        let mut tx = self.db.pool().begin().await.unwrap();
        let tuple = RelationTuple::new(id, Relation::Assignee, person);
        if grant {
            self.access.grant(&mut tx, tuple).await.unwrap();
        } else {
            self.access.revoke(&mut tx, tuple).await.unwrap();
        }
        tx.commit().await.unwrap();
    }

    /// Ali's daily routine from Sunday 15 November, with `people` assigned in this turn order.
    async fn rotating(&self, rotation: RoutineRotation, people: &[Uuid]) -> Uuid {
        let id = self
            .routines
            .create(
                self.ali,
                CreateRoutineRequest {
                    title: "Çöp".into(),
                    description: None,
                    timezone: "Europe/Istanbul".into(),
                    rrule: "FREQ=DAILY".into(),
                    daily_target: None,
                    start_date: Some(day(15)),
                    end_date: None,
                    reminder_times: None,
                    completion_mode: None,
                    rotation: Some(rotation),
                },
            )
            .await
            .unwrap()
            .item
            .id;
        for person in people {
            self.relate(id, *person, true).await;
        }
        self.routines
            .update(
                self.ali,
                id,
                UpdateRoutineRequest {
                    rotation_order: Some(people.to_vec()),
                    ..UpdateRoutineRequest::default()
                },
            )
            .await
            .unwrap();
        id
    }

    async fn turns(&self, id: Uuid, from: u32, to: u32) -> Vec<Uuid> {
        self.routines
            .rotation(self.ali, id, range(from, to))
            .await
            .unwrap()
            .assignments
            .into_iter()
            .map(|a| a.user_id)
            .collect()
    }
}

#[tokio::test]
async fn r6_the_next_person_takes_the_next_occurrence() {
    let w = World::new().await;
    let id = w
        .rotating(RoutineRotation::EveryOccurrence, &[w.ayse, w.can])
        .await;
    let view = w.routines.rotation(w.ali, id, range(15, 18)).await.unwrap();
    assert_eq!(view.rotation, RoutineRotation::EveryOccurrence);
    assert_eq!(view.members, vec![w.ayse, w.can]);
    assert_eq!(view.assignments[0].date, day(15));
    assert_eq!(
        w.turns(id, 15, 18).await,
        vec![w.ayse, w.can, w.ayse, w.can]
    );
    // Without a range: four weeks from today, which is the sixth occurrence (Can's).
    let ahead = w
        .routines
        .rotation(w.ali, id, RotationQuery::default())
        .await
        .unwrap();
    assert_eq!(ahead.assignments.len(), 28);
    assert_eq!(ahead.assignments[0].date, day(20));
    assert_eq!(ahead.assignments[0].user_id, w.can);
    assert_eq!(
        w.routines.routine(w.ali, id).await.unwrap().rotation,
        Some(RoutineRotation::EveryOccurrence)
    );
}

#[tokio::test]
async fn r6_weekly_rotation_keeps_a_person_for_the_calendar_week() {
    let w = World::new().await;
    let id = w.rotating(RoutineRotation::Weekly, &[w.ayse, w.can]).await;
    // Sunday the 15th closes the first week; the next week (16-22) is Can's, then Ayşe's.
    let turns = w.turns(id, 15, 23).await;
    assert_eq!(turns[0], w.ayse);
    assert!(turns[1..8].iter().all(|p| *p == w.can));
    assert_eq!(turns[8], w.ayse);
}

#[tokio::test]
async fn r6_a_swap_changes_only_the_two_dates_and_carries_into_later_reads() {
    let w = World::new().await;
    let id = w
        .rotating(RoutineRotation::EveryOccurrence, &[w.ayse, w.can, w.deniz])
        .await;
    // 20: Deniz, 21: Ayşe, 22: Can, 23: Deniz.
    let before = w.turns(id, 20, 24).await;
    assert_eq!(before, vec![w.deniz, w.ayse, w.can, w.deniz, w.ayse]);
    let swapped = w.routines.swap(w.ayse, id, swap(21, 22)).await.unwrap();
    assert_eq!(swapped.assignments.len(), 2);
    assert_eq!(
        w.turns(id, 20, 24).await,
        vec![w.deniz, w.can, w.ayse, w.deniz, w.ayse]
    );
    // Swapping back leaves the rotation as it was.
    w.routines.swap(w.can, id, swap(21, 22)).await.unwrap();
    assert_eq!(w.turns(id, 20, 24).await, before);
}

#[tokio::test]
async fn r6_only_a_turn_holder_or_a_manager_may_swap() {
    let w = World::new().await;
    let id = w
        .rotating(RoutineRotation::EveryOccurrence, &[w.ayse, w.can, w.deniz])
        .await;
    // 21 is Ayşe's and 22 Can's: Deniz has neither.
    assert!(matches!(
        w.routines.swap(w.deniz, id, swap(21, 22)).await,
        Err(AppError::Forbidden)
    ));
    // The owner manages the routine.
    w.routines.swap(w.ali, id, swap(21, 22)).await.unwrap();
    // A stranger does not learn the routine exists (V9).
    let stranger = Uuid::now_v7();
    assert!(matches!(
        w.routines.swap(stranger, id, swap(21, 22)).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.routines.rotation(stranger, id, range(20, 22)).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn r6_swaps_are_refused_for_the_past_and_for_no_change() {
    let w = World::new().await;
    let id = w
        .rotating(RoutineRotation::EveryOccurrence, &[w.ayse, w.can])
        .await;
    for request in [swap(19, 21), swap(21, 23)] {
        assert!(matches!(
            w.routines.swap(w.ali, id, request).await,
            Err(AppError::Validation(_))
        ));
    }
}

#[tokio::test]
async fn r6_a_member_leaving_keeps_the_rotation_consistent() {
    let w = World::new().await;
    let id = w
        .rotating(RoutineRotation::EveryOccurrence, &[w.ayse, w.can, w.deniz])
        .await;
    // Deniz takes Ayşe's 21st, then leaves the routine.
    w.routines.swap(w.deniz, id, swap(20, 21)).await.unwrap();
    w.relate(id, w.deniz, false).await;
    let view = w.routines.rotation(w.ali, id, range(15, 30)).await.unwrap();
    assert_eq!(view.members, vec![w.ayse, w.can]);
    assert_eq!(view.assignments.len(), 16);
    assert!(view.assignments.iter().all(|a| a.user_id != w.deniz));
    // The two who remain alternate: the swap made under the old order (both halves) is gone.
    let turns: Vec<Uuid> = view.assignments.iter().map(|a| a.user_id).collect();
    assert!(turns.windows(2).all(|pair| pair[0] != pair[1]));
    // Someone who joins takes turns after everyone in the stored order.
    w.relate(id, w.deniz, true).await;
    let rejoined = w.routines.rotation(w.ali, id, range(15, 17)).await.unwrap();
    assert_eq!(rejoined.members, vec![w.ayse, w.can, w.deniz]);
}

#[tokio::test]
async fn r6_rotation_settings_are_validated() {
    let w = World::new().await;
    let id = w
        .rotating(RoutineRotation::EveryOccurrence, &[w.ayse, w.can])
        .await;
    let bad = [
        // A rotation gives each day to one person; "each person" cannot apply.
        UpdateRoutineRequest {
            completion_mode: Some(CompletionMode::Individual),
            ..UpdateRoutineRequest::default()
        },
        UpdateRoutineRequest {
            rotation_order: Some(vec![w.ayse, Uuid::now_v7()]),
            ..UpdateRoutineRequest::default()
        },
        UpdateRoutineRequest {
            rotation_order: Some(vec![w.ayse, w.ayse]),
            ..UpdateRoutineRequest::default()
        },
    ];
    for request in bad {
        assert!(matches!(
            w.routines.update(w.ali, id, request).await,
            Err(AppError::Validation(_))
        ));
    }
}

#[tokio::test]
async fn r6_fixed_assignees_drop_the_swaps() {
    let w = World::new().await;
    let id = w
        .rotating(RoutineRotation::EveryOccurrence, &[w.ayse, w.can])
        .await;
    let before = w.turns(id, 20, 23).await;
    w.routines.swap(w.ali, id, swap(20, 21)).await.unwrap();
    let fixed = UpdateRoutineRequest {
        rotation: Some(None),
        ..UpdateRoutineRequest::default()
    };
    let view = w.routines.update(w.ali, id, fixed).await.unwrap();
    assert_eq!(view.rotation, None);
    assert!(matches!(
        w.routines.rotation(w.ali, id, range(20, 23)).await,
        Err(AppError::Conflict(_))
    ));
    let again = UpdateRoutineRequest {
        rotation: Some(Some(RoutineRotation::EveryOccurrence)),
        ..UpdateRoutineRequest::default()
    };
    w.routines.update(w.ali, id, again).await.unwrap();
    assert_eq!(w.turns(id, 20, 23).await, before);
}
