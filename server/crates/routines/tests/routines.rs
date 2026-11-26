//! Routines and plan versions (T2.2.1, PRD R1–R3, R11, R13) against a real Postgres 17. Requires
//! Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService, NewItem};
use kernel::{AppError, FixedClock};
use recurrence::{PlanVersion, Schedule};
use routines::{
    ChangePlanRequest, CompleteRoutineRequest, CreateRoutineRequest, RoutineDaysQuery,
    RoutineStreak, RoutineView, RoutinesService, UndoCompletionQuery, UpdateRoutineRequest,
};
use uuid::Uuid;

/// 23 November 2026, 12:00 UTC.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 23, 12, 0, 0).unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn request(rrule: &str) -> CreateRoutineRequest {
    CreateRoutineRequest {
        title: "Read".into(),
        description: None,
        timezone: "Europe/Istanbul".into(),
        rrule: rrule.into(),
        daily_target: None,
        start_date: Some(day(1)),
        end_date: None,
        reminder_times: None,
    }
}

fn plan(rrule: &str) -> ChangePlanRequest {
    ChangePlanRequest {
        rrule: rrule.into(),
        daily_target: None,
        valid_from: None,
    }
}

fn schedule_of(routine: &RoutineView) -> Schedule {
    let timezone: Tz = routine.timezone.parse().unwrap();
    let versions = routine
        .versions
        .iter()
        .map(|v| PlanVersion {
            rules: v.rrule.parse().unwrap(),
            valid_from: v.valid_from,
            valid_to: v.valid_to,
        })
        .collect();
    Schedule::new(timezone, versions).unwrap()
}

struct World {
    _db: TestDb,
    items: ItemsService,
    routines: RoutinesService,
    clock: Arc<FixedClock>,
    owner: Uuid,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let clock = Arc::new(FixedClock::new(now()));
        let items = ItemsService::new(db.pool().clone(), clock.clone(), access.clone());
        let routines = RoutinesService::new(
            db.pool().clone(),
            clock.clone(),
            access.permissions(),
            items.clone(),
        );
        Self {
            _db: db,
            items,
            routines,
            clock,
            owner: Uuid::now_v7(),
        }
    }

    async fn routine(&self, rrule: &str) -> RoutineView {
        self.routines
            .create(self.owner, request(rrule))
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn r1_every_recurrence_kind_is_saved() {
    let w = World::new().await;
    for rrule in [
        "FREQ=DAILY",
        "FREQ=DAILY;INTERVAL=3",
        "FREQ=WEEKLY;BYDAY=MO,WE",
        "FREQ=WEEKLY;INTERVAL=2;BYDAY=FR",
        "FREQ=MONTHLY;BYMONTHDAY=15,20",
        "FREQ=MONTHLY;BYMONTHDAY=-1",
        "FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=8\nFREQ=YEARLY;BYMONTH=9;BYMONTHDAY=1",
    ] {
        let routine = w.routine(rrule).await;
        assert_eq!(routine.versions.len(), 1);
        assert_eq!(routine.versions[0].rrule, rrule);
        assert_eq!(routine.item.kind, "routine");
    }
}

#[tokio::test]
async fn r2_a_routine_has_its_own_time_zone_and_dates() {
    let w = World::new().await;
    // 23 November 23:30 UTC is already the 24th in Istanbul.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 23, 30, 0).unwrap());
    let routine = w
        .routines
        .create(
            w.owner,
            CreateRoutineRequest {
                start_date: None,
                end_date: Some(day(30)),
                ..request("FREQ=DAILY")
            },
        )
        .await
        .unwrap();
    assert_eq!(routine.timezone, "Europe/Istanbul");
    assert_eq!(routine.start_date, day(24));
    assert_eq!(routine.end_date, Some(day(30)));
    assert_eq!(routine.versions[0].valid_to, Some(day(30)));
}

#[tokio::test]
async fn r3_and_r13_target_and_reminders_are_saved() {
    let w = World::new().await;
    let routine = w
        .routines
        .create(
            w.owner,
            CreateRoutineRequest {
                daily_target: Some(8),
                reminder_times: Some(vec!["20:30".into(), "08:00".into()]),
                ..request("FREQ=DAILY")
            },
        )
        .await
        .unwrap();
    assert_eq!(routine.daily_target, 8);
    assert_eq!(routine.reminder_times, ["08:00", "20:30"]);
    assert_eq!(w.routine("FREQ=DAILY").await.daily_target, 1);
}

#[tokio::test]
async fn r11_a_plan_change_keeps_past_occurrences_and_targets() {
    let w = World::new().await;
    let before = w
        .routines
        .create(
            w.owner,
            CreateRoutineRequest {
                daily_target: Some(2),
                ..request("FREQ=DAILY")
            },
        )
        .await
        .unwrap();
    let past = schedule_of(&before).occurrences(day(1), day(22)).unwrap();

    let after = w
        .routines
        .change_plan(
            w.owner,
            before.item.id,
            ChangePlanRequest {
                daily_target: Some(5),
                ..plan("FREQ=WEEKLY;BYDAY=MO")
            },
        )
        .await
        .unwrap();

    assert_eq!(after.versions.len(), 2);
    assert_eq!(after.versions[0].rrule, "FREQ=DAILY");
    assert_eq!(after.versions[0].daily_target, 2);
    assert_eq!(after.versions[0].valid_to, Some(day(22)));
    assert_eq!(after.versions[1].valid_from, day(23));
    assert_eq!(after.versions[1].daily_target, 5);
    assert_eq!(after.daily_target, 5);
    let schedule = schedule_of(&after);
    assert_eq!(schedule.occurrences(day(1), day(22)).unwrap(), past);
    // 23 November is a Monday; the next day no longer occurs.
    assert!(schedule.occurs_on(day(23)));
    assert!(!schedule.occurs_on(day(24)));
}

#[tokio::test]
async fn r11_the_target_is_kept_when_only_the_rule_changes() {
    let w = World::new().await;
    let routine = w
        .routines
        .create(
            w.owner,
            CreateRoutineRequest {
                daily_target: Some(4),
                ..request("FREQ=DAILY")
            },
        )
        .await
        .unwrap();
    let after = w
        .routines
        .change_plan(w.owner, routine.item.id, plan("FREQ=DAILY;INTERVAL=2"))
        .await
        .unwrap();
    assert_eq!(after.versions[1].daily_target, 4);
}

#[tokio::test]
async fn r11_a_plan_cannot_change_in_the_past() {
    let w = World::new().await;
    let routine = w.routine("FREQ=DAILY").await;
    let result = w
        .routines
        .change_plan(
            w.owner,
            routine.item.id,
            ChangePlanRequest {
                valid_from: Some(day(22)),
                ..plan("FREQ=WEEKLY")
            },
        )
        .await;
    assert!(matches!(result, Err(AppError::Validation(_))));
}

#[tokio::test]
async fn a_change_on_the_first_day_replaces_the_version() {
    let w = World::new().await;
    let routine = w
        .routines
        .create(
            w.owner,
            CreateRoutineRequest {
                start_date: Some(day(23)),
                ..request("FREQ=DAILY")
            },
        )
        .await
        .unwrap();
    let after = w
        .routines
        .change_plan(w.owner, routine.item.id, plan("FREQ=WEEKLY;BYDAY=MO"))
        .await
        .unwrap();
    assert_eq!(after.versions.len(), 1);
    assert_eq!(after.versions[0].rrule, "FREQ=WEEKLY;BYDAY=MO");
}

#[tokio::test]
async fn a_future_routine_changes_from_its_first_day() {
    let w = World::new().await;
    let routine = w
        .routines
        .create(
            w.owner,
            CreateRoutineRequest {
                start_date: Some(day(28)),
                ..request("FREQ=DAILY")
            },
        )
        .await
        .unwrap();
    let after = w
        .routines
        .change_plan(w.owner, routine.item.id, plan("FREQ=WEEKLY"))
        .await
        .unwrap();
    assert_eq!(after.versions.len(), 1);
    assert_eq!(after.versions[0].valid_from, day(28));
}

#[tokio::test]
async fn r11_reopening_an_ended_routine_does_not_fill_the_gap() {
    let w = World::new().await;
    let routine = w
        .routines
        .create(
            w.owner,
            CreateRoutineRequest {
                end_date: Some(day(25)),
                ..request("FREQ=DAILY")
            },
        )
        .await
        .unwrap();
    let id = routine.item.id;
    w.clock.set(now() + Duration::days(5)); // 28 November
    let after = w
        .routines
        .update(
            w.owner,
            id,
            UpdateRoutineRequest {
                end_date: Some(None),
                ..UpdateRoutineRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(after.end_date, None);
    assert_eq!(after.versions.len(), 2);
    let schedule = schedule_of(&after);
    assert!(schedule.occurs_on(day(25)));
    assert!(!schedule.occurs_on(day(26)));
    assert!(!schedule.occurs_on(day(27)));
    assert!(schedule.occurs_on(day(28)));
}

#[tokio::test]
async fn the_end_date_cannot_move_into_the_past() {
    let w = World::new().await;
    let routine = w.routine("FREQ=DAILY").await;
    let result = w
        .routines
        .update(
            w.owner,
            routine.item.id,
            UpdateRoutineRequest {
                end_date: Some(Some(day(22))),
                ..UpdateRoutineRequest::default()
            },
        )
        .await;
    assert!(matches!(result, Err(AppError::Validation(_))));
}

#[tokio::test]
async fn the_end_date_follows_the_current_version() {
    let w = World::new().await;
    let routine = w.routine("FREQ=DAILY").await;
    let updated = w
        .routines
        .update(
            w.owner,
            routine.item.id,
            UpdateRoutineRequest {
                end_date: Some(Some(day(29))),
                ..UpdateRoutineRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.end_date, Some(day(29)));
    assert_eq!(updated.versions[0].valid_to, Some(day(29)));
}

#[tokio::test]
async fn time_zone_and_reminders_change() {
    let w = World::new().await;
    let routine = w.routine("FREQ=DAILY").await;
    let updated = w
        .routines
        .update(
            w.owner,
            routine.item.id,
            UpdateRoutineRequest {
                timezone: Some("Asia/Tokyo".into()),
                reminder_times: Some(vec!["07:15".into()]),
                ..UpdateRoutineRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.timezone, "Asia/Tokyo");
    assert_eq!(updated.reminder_times, ["07:15"]);
    assert_eq!(updated.versions.len(), 1);
}

#[tokio::test]
async fn invalid_requests_are_refused() {
    let w = World::new().await;
    let bad = [
        CreateRoutineRequest {
            timezone: "Mars/Base".into(),
            ..request("FREQ=DAILY")
        },
        request("FREQ=DAILY;COUNT=2"),
        CreateRoutineRequest {
            daily_target: Some(0),
            ..request("FREQ=DAILY")
        },
        CreateRoutineRequest {
            start_date: Some(day(9)),
            end_date: Some(day(1)),
            ..request("FREQ=DAILY")
        },
        CreateRoutineRequest {
            reminder_times: Some(vec!["25:00".into()]),
            ..request("FREQ=DAILY")
        },
        CreateRoutineRequest {
            title: "  ".into(),
            ..request("FREQ=DAILY")
        },
    ];
    for item in bad {
        assert!(matches!(
            w.routines.create(w.owner, item).await,
            Err(AppError::Validation(_))
        ));
    }
    let routine = w.routine("FREQ=DAILY").await;
    let nothing = w
        .routines
        .update(w.owner, routine.item.id, UpdateRoutineRequest::default())
        .await;
    assert!(matches!(nothing, Err(AppError::Validation(_))));
}

#[tokio::test]
async fn v9_a_stranger_gets_not_found_everywhere() {
    let w = World::new().await;
    let routine = w.routine("FREQ=DAILY").await;
    let id = routine.item.id;
    let stranger = Uuid::now_v7();
    assert!(matches!(
        w.routines.routine(stranger, id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.routines
            .update(stranger, id, UpdateRoutineRequest::default())
            .await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.routines
            .change_plan(stranger, id, plan("FREQ=DAILY"))
            .await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn an_item_of_another_kind_is_not_a_routine() {
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
        w.routines.routine(w.owner, goal.id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.routines
            .change_plan(w.owner, goal.id, plan("FREQ=DAILY"))
            .await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn a_routine_in_the_trash_cannot_change() {
    let w = World::new().await;
    let routine = w.routine("FREQ=DAILY").await;
    w.items
        .set_trashed(w.owner, routine.item.id, true)
        .await
        .unwrap();
    assert!(matches!(
        w.routines
            .change_plan(w.owner, routine.item.id, plan("FREQ=WEEKLY"))
            .await,
        Err(AppError::Conflict(_))
    ));
}

fn on(date: NaiveDate) -> CompleteRoutineRequest {
    CompleteRoutineRequest {
        date: Some(date),
        count: None,
    }
}

async fn with_target(w: &World, target: i32) -> Uuid {
    w.routines
        .create(
            w.owner,
            CreateRoutineRequest {
                daily_target: Some(target),
                ..request("FREQ=DAILY")
            },
        )
        .await
        .unwrap()
        .item
        .id
}

#[tokio::test]
async fn r4_a_partial_day_counts() {
    let w = World::new().await;
    let id = with_target(&w, 8).await;
    let mut last = None;
    for _ in 0..5 {
        last = Some(
            w.routines
                .complete(w.owner, id, CompleteRoutineRequest::default())
                .await
                .unwrap(),
        );
    }
    let day_state = last.unwrap();
    assert_eq!(
        (day_state.date, day_state.done, day_state.target),
        (day(23), 5, 8)
    );
    assert!(!day_state.complete);
    let more = CompleteRoutineRequest {
        count: Some(3),
        ..CompleteRoutineRequest::default()
    };
    assert!(
        w.routines
            .complete(w.owner, id, more)
            .await
            .unwrap()
            .complete
    );
}

#[tokio::test]
async fn r7_only_the_last_three_days_can_be_marked() {
    let w = World::new().await;
    let id = w.routine("FREQ=DAILY").await.item.id;
    assert!(w.routines.complete(w.owner, id, on(day(20))).await.is_ok());
    for bad in [day(19), day(24)] {
        assert!(matches!(
            w.routines.complete(w.owner, id, on(bad)).await,
            Err(AppError::Validation(_))
        ));
    }
}

#[tokio::test]
async fn r7_today_is_the_routines_own_day() {
    let w = World::new().await;
    let id = w.routine("FREQ=DAILY").await.item.id;
    // 23 November 22:30 UTC is already the 24th in Istanbul, so the 21st is the last allowed.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 22, 30, 0).unwrap());
    let today = w
        .routines
        .complete(w.owner, id, CompleteRoutineRequest::default())
        .await
        .unwrap();
    assert_eq!(today.date, day(24));
    assert!(w.routines.complete(w.owner, id, on(day(21))).await.is_ok());
    assert!(w.routines.complete(w.owner, id, on(day(20))).await.is_err());
}

#[tokio::test]
async fn a_day_without_an_occurrence_cannot_be_marked() {
    let w = World::new().await;
    // 23 November 2026 is a Monday.
    let id = w.routine("FREQ=WEEKLY;BYDAY=MO").await.item.id;
    assert!(w.routines.complete(w.owner, id, on(day(23))).await.is_ok());
    assert!(matches!(
        w.routines.complete(w.owner, id, on(day(22))).await,
        Err(AppError::Validation(_))
    ));
}

#[tokio::test]
async fn r11_each_day_keeps_the_target_of_its_plan() {
    let w = World::new().await;
    let id = with_target(&w, 2).await;
    w.routines.complete(w.owner, id, on(day(22))).await.unwrap();
    let change = ChangePlanRequest {
        daily_target: Some(5),
        ..plan("FREQ=DAILY")
    };
    w.routines.change_plan(w.owner, id, change).await.unwrap();
    let days = w
        .routines
        .days(
            w.owner,
            id,
            RoutineDaysQuery {
                from: Some(day(22)),
                to: Some(day(23)),
            },
        )
        .await
        .unwrap();
    assert_eq!((days[0].target, days[0].done), (2, 1));
    assert_eq!((days[1].target, days[1].done), (5, 0));
}

#[tokio::test]
async fn undo_takes_back_the_latest_and_is_harmless_when_empty() {
    let w = World::new().await;
    let id = with_target(&w, 3).await;
    let two = CompleteRoutineRequest {
        count: Some(2),
        ..CompleteRoutineRequest::default()
    };
    w.routines.complete(w.owner, id, two).await.unwrap();
    // A minute later: "latest" is by the time it was entered.
    w.clock.advance(Duration::minutes(1));
    w.routines
        .complete(w.owner, id, CompleteRoutineRequest::default())
        .await
        .unwrap();
    let query = UndoCompletionQuery::default();
    assert_eq!(
        w.routines
            .undo_completion(w.owner, id, query)
            .await
            .unwrap()
            .done,
        2
    );
    assert_eq!(
        w.routines
            .undo_completion(w.owner, id, query)
            .await
            .unwrap()
            .done,
        0
    );
    assert_eq!(
        w.routines
            .undo_completion(w.owner, id, query)
            .await
            .unwrap()
            .done,
        0
    );
    let old = UndoCompletionQuery {
        date: Some(day(19)),
    };
    assert!(matches!(
        w.routines.undo_completion(w.owner, id, old).await,
        Err(AppError::Validation(_))
    ));
}

#[tokio::test]
async fn concurrent_completions_all_count() {
    let w = World::new().await;
    let id = with_target(&w, 8).await;
    let add = || {
        w.routines
            .complete(w.owner, id, CompleteRoutineRequest::default())
    };
    let results = tokio::join!(add(), add(), add(), add());
    assert!(results.0.is_ok() && results.1.is_ok() && results.2.is_ok() && results.3.is_ok());
    let days = w
        .routines
        .days(w.owner, id, RoutineDaysQuery::default())
        .await
        .unwrap();
    assert_eq!(days.len(), 30);
    assert_eq!(days.last().unwrap().done, 4);
}

#[tokio::test]
async fn a_range_must_be_valid() {
    let w = World::new().await;
    let id = w.routine("FREQ=DAILY").await.item.id;
    let reversed = RoutineDaysQuery {
        from: Some(day(10)),
        to: Some(day(9)),
    };
    assert!(w.routines.days(w.owner, id, reversed).await.is_err());
    let long = RoutineDaysQuery {
        from: Some(day(1) - Duration::days(routines::DAYS_MAX)),
        to: Some(day(1)),
    };
    assert!(w.routines.days(w.owner, id, long).await.is_err());
}

#[tokio::test]
async fn v9_a_stranger_cannot_touch_completions() {
    let w = World::new().await;
    let id = w.routine("FREQ=DAILY").await.item.id;
    let stranger = Uuid::now_v7();
    assert!(matches!(
        w.routines
            .complete(stranger, id, CompleteRoutineRequest::default())
            .await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.routines
            .undo_completion(stranger, id, UndoCompletionQuery::default())
            .await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.routines
            .days(stranger, id, RoutineDaysQuery::default())
            .await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.routines.streak(stranger, id).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn r9_the_streak_comes_from_complete_occurrences() {
    let w = World::new().await;
    let id = with_target(&w, 2).await;
    let streak = |current, today_pending| RoutineStreak {
        current,
        today_pending,
    };
    assert_eq!(
        w.routines.streak(w.owner, id).await.unwrap(),
        streak(0, true)
    );
    for date in [day(20), day(21), day(22)] {
        let twice = CompleteRoutineRequest {
            date: Some(date),
            count: Some(2),
        };
        w.routines.complete(w.owner, id, twice).await.unwrap();
    }
    // Today is still open: it does not break the streak (R9).
    assert_eq!(
        w.routines.streak(w.owner, id).await.unwrap(),
        streak(3, true)
    );
    // A partial day does not continue it (R4, R9).
    w.routines.complete(w.owner, id, on(day(23))).await.unwrap();
    assert_eq!(
        w.routines.streak(w.owner, id).await.unwrap(),
        streak(3, true)
    );
    w.routines.complete(w.owner, id, on(day(23))).await.unwrap();
    assert_eq!(
        w.routines.streak(w.owner, id).await.unwrap(),
        streak(4, false)
    );
    // Two days later the 24th was missed.
    w.clock.advance(Duration::days(2));
    assert_eq!(
        w.routines.streak(w.owner, id).await.unwrap(),
        streak(0, true)
    );
}

#[tokio::test]
async fn r9_days_without_an_occurrence_do_not_break_the_streak() {
    let w = World::new().await;
    // Mondays: the 16th and the 23rd.
    let id = w.routine("FREQ=WEEKLY;BYDAY=MO").await.item.id;
    w.clock.set(now() - Duration::days(7));
    w.routines.complete(w.owner, id, on(day(16))).await.unwrap();
    w.clock.set(now());
    w.routines.complete(w.owner, id, on(day(23))).await.unwrap();
    // Sunday the 29th: six days without an occurrence since the last one.
    w.clock.set(now() + Duration::days(6));
    let result = w.routines.streak(w.owner, id).await.unwrap();
    assert_eq!(result.current, 2);
    assert!(!result.today_pending);
}
