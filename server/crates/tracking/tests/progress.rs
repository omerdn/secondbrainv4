//! `TrackerProgress` over a real Postgres 17 (T1.3.3). Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService, NewItem};
use kernel::{AppError, FixedClock};
use rust_decimal::Decimal;
use tracking::repo;
use tracking::{
    Aggregation, DateRange, Direction, NewEntry, NewTracker, OwnerKind, Period, ProgressProvider,
    TrackerProgress,
};
use uuid::Uuid;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, d).unwrap()
}

async fn tracker(db: &TestDb, user: Uuid, direction: Direction, target: i64) -> Uuid {
    let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
    let items = ItemsService::new(db.pool().clone(), Arc::new(FixedClock::new(now())), access);
    let new = NewItem {
        kind: "tracker".into(),
        title: "T".into(),
        description: None,
    };
    let item_id = items.create(user, new).await.unwrap().id;
    let definition = NewTracker {
        item_id,
        unit: "x".into(),
        target_value: Decimal::from(target),
        period: Period::Day,
        aggregation: Aggregation::Sum,
        direction,
        owner_kind: OwnerKind::Standalone,
    };
    repo::insert_tracker(db.pool(), &definition, now())
        .await
        .unwrap();
    item_id
}

async fn add(db: &TestDb, user: Uuid, tracker: Uuid, value: i64, on: NaiveDate) -> Uuid {
    let new = NewEntry {
        tracker_item_id: tracker,
        user_id: user,
        value: Decimal::from(value),
        occurred_at: now(),
        local_date: on,
        note_block_id: None,
    };
    repo::insert_entry(db.pool(), &new, now()).await.unwrap().id
}

#[tokio::test]
async fn provider_reads_entries_of_the_range_for_both_directions() {
    let db = TestDb::new().await;
    let user = Uuid::now_v7();
    let provider = TrackerProgress::new(db.pool().clone());
    let range = DateRange::new(day(1), day(3)).unwrap();

    let rising = tracker(&db, user, Direction::Increase, 10).await;
    add(&db, user, rising, 25, day(1)).await;
    let removed = add(&db, user, rising, 5, day(2)).await;
    add(&db, user, rising, 4, day(2)).await;
    add(&db, user, rising, 10, day(4)).await; // outside the range
    repo::soft_delete_entry(db.pool(), rising, removed, now())
        .await
        .unwrap();
    let progress = provider.progress(rising, range, day(2)).await.unwrap();
    assert_eq!(progress.done, 14.0);
    assert_eq!(progress.total, 30.0);
    assert_eq!(progress.expected_to_date, 20.0);

    let limit = tracker(&db, user, Direction::Limit, 10).await;
    add(&db, user, limit, 5, day(1)).await;
    add(&db, user, limit, 11, day(2)).await;
    let status = provider.status(limit, range, day(3)).await.unwrap();
    assert_eq!(status.progress.done, 2.0);
    assert_eq!(status.progress.total, 3.0);
    assert!(!status.successful);
}

#[tokio::test]
async fn an_item_without_a_tracker_is_not_found() {
    let db = TestDb::new().await;
    let provider = TrackerProgress::new(db.pool().clone());
    let range = DateRange::new(day(1), day(3)).unwrap();
    assert!(matches!(
        provider.progress(Uuid::now_v7(), range, day(1)).await,
        Err(AppError::NotFound)
    ));
}
