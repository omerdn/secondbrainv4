//! Tracker storage (T1.3.1, PRD T1, T2) against a real Postgres 17. Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use items::{ItemsAccessContext, ItemsService, NewItem, TRASH_RETENTION};
use kernel::FixedClock;
use rust_decimal::Decimal;
use tracking::repo;
use tracking::{Aggregation, Direction, NewEntry, NewTracker, OwnerKind, Period};
use uuid::Uuid;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

struct World {
    db: TestDb,
    items: ItemsService,
    user: Uuid,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let clock = Arc::new(FixedClock::new(now()));
        let items = ItemsService::new(db.pool().clone(), clock, access);
        Self {
            db,
            items,
            user: Uuid::now_v7(),
        }
    }

    async fn item(&self) -> Uuid {
        let new = NewItem {
            kind: "tracker".into(),
            title: "Reading".into(),
            description: None,
        };
        self.items.create(self.user, new).await.unwrap().id
    }

    async fn tracker(&self) -> Uuid {
        let item_id = self.item().await;
        repo::insert_tracker(self.db.pool(), &definition(item_id, Period::Day), now())
            .await
            .unwrap();
        item_id
    }

    fn entry(&self, tracker: Uuid, value: Decimal, local_date: NaiveDate) -> NewEntry {
        NewEntry {
            tracker_item_id: tracker,
            user_id: self.user,
            value,
            occurred_at: now(),
            local_date,
            note_block_id: None,
        }
    }
}

fn definition(item_id: Uuid, period: Period) -> NewTracker {
    NewTracker {
        item_id,
        unit: "sayfa".into(),
        target_value: Decimal::new(300, 0),
        period,
        aggregation: Aggregation::Sum,
        direction: Direction::Increase,
        owner_kind: OwnerKind::Standalone,
    }
}

#[tokio::test]
async fn t1_every_definition_field_round_trips() {
    let w = World::new().await;
    let periods = [
        Period::Day,
        Period::Week,
        Period::Month,
        Period::Total,
        Period::Range {
            start: day(1),
            end: day(30),
        },
    ];
    for period in periods {
        for &aggregation in Aggregation::ALL {
            for &direction in Direction::ALL {
                for &owner_kind in OwnerKind::ALL {
                    let new = NewTracker {
                        aggregation,
                        direction,
                        owner_kind,
                        target_value: Decimal::new(125_050, 2),
                        ..definition(w.item().await, period)
                    };
                    let stored = repo::insert_tracker(w.db.pool(), &new, now())
                        .await
                        .unwrap();
                    assert_eq!(stored.unit, "sayfa");
                    assert_eq!(stored.target_value, new.target_value);
                    assert_eq!(stored.period, period);
                    assert_eq!(stored.aggregation, aggregation);
                    assert_eq!(stored.direction, direction);
                    assert_eq!(stored.owner_kind, owner_kind);
                    let read = repo::get_tracker(w.db.pool(), new.item_id).await.unwrap();
                    assert_eq!(read, Some(stored));
                }
            }
        }
    }
    assert_eq!(
        repo::get_tracker(w.db.pool(), Uuid::now_v7())
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn invalid_definitions_are_rejected_by_the_schema() {
    let w = World::new().await;
    let inverted = Period::Range {
        start: day(2),
        end: day(1),
    };
    let item = w.item().await;
    assert!(
        repo::insert_tracker(w.db.pool(), &definition(item, inverted), now())
            .await
            .is_err()
    );
    let zero = NewTracker {
        target_value: Decimal::ZERO,
        ..definition(item, Period::Day)
    };
    assert!(
        repo::insert_tracker(w.db.pool(), &zero, now())
            .await
            .is_err()
    );
    let blank = NewTracker {
        unit: "  ".into(),
        ..definition(item, Period::Day)
    };
    assert!(
        repo::insert_tracker(w.db.pool(), &blank, now())
            .await
            .is_err()
    );
    // No tracker without its item.
    let orphan = definition(Uuid::now_v7(), Period::Day);
    assert!(
        repo::insert_tracker(w.db.pool(), &orphan, now())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn t2_entries_keep_exact_values_and_an_optional_note_line() {
    let w = World::new().await;
    let tracker = w.tracker().await;
    let note = Uuid::now_v7();
    let new = NewEntry {
        note_block_id: Some(note),
        ..w.entry(tracker, Decimal::new(1999, 2), day(5))
    };
    let stored = repo::insert_entry(w.db.pool(), &new, now()).await.unwrap();
    assert_eq!(stored.value, Decimal::new(1999, 2));
    assert_eq!(stored.note_block_id, Some(note));
    assert_eq!(stored.local_date, day(5));
    assert_eq!(stored.user_id, w.user);
    assert_eq!(stored.created_at, now());
    assert_eq!(stored.deleted_at, None);

    // Decimal arithmetic stays exact, unlike floats (0.1 + 0.2).
    for value in [Decimal::new(1, 1), Decimal::new(2, 1)] {
        repo::insert_entry(w.db.pool(), &w.entry(tracker, value, day(6)), now())
            .await
            .unwrap();
    }
    let sum: Decimal = repo::list_entries(w.db.pool(), tracker, Some(day(6)), Some(day(6)))
        .await
        .unwrap()
        .iter()
        .map(|entry| entry.value)
        .sum();
    assert_eq!(sum, Decimal::new(3, 1));

    let unknown = w.entry(Uuid::now_v7(), Decimal::ONE, day(5));
    assert!(
        repo::insert_entry(w.db.pool(), &unknown, now())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn entries_list_by_day_and_soft_delete() {
    let w = World::new().await;
    let tracker = w.tracker().await;
    let other = w.tracker().await;
    let mut ids = Vec::new();
    for d in [3, 1, 2] {
        let new = w.entry(tracker, Decimal::from(d), day(d));
        ids.push(
            repo::insert_entry(w.db.pool(), &new, now())
                .await
                .unwrap()
                .id,
        );
    }
    let days = |entries: Vec<tracking::TrackerEntry>| {
        entries.iter().map(|e| e.local_date).collect::<Vec<_>>()
    };
    let all = repo::list_entries(w.db.pool(), tracker, None, None)
        .await
        .unwrap();
    assert_eq!(days(all), [day(1), day(2), day(3)]);
    let both_ends_included = repo::list_entries(w.db.pool(), tracker, Some(day(2)), Some(day(3)))
        .await
        .unwrap();
    assert_eq!(days(both_ends_included), [day(2), day(3)]);

    // Only through its own tracker.
    assert!(
        !repo::soft_delete_entry(w.db.pool(), other, ids[0], now())
            .await
            .unwrap()
    );
    assert!(
        repo::soft_delete_entry(w.db.pool(), tracker, ids[0], now())
            .await
            .unwrap()
    );
    let later = now() + Duration::hours(1);
    assert!(
        !repo::soft_delete_entry(w.db.pool(), tracker, ids[0], later)
            .await
            .unwrap(),
        "deleting twice is a no-op"
    );
    let deleted_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM tracker_entries WHERE id = $1")
            .bind(ids[0])
            .fetch_one(w.db.pool())
            .await
            .unwrap();
    assert_eq!(deleted_at, Some(now()), "the first deletion time stays");
    let live = repo::list_entries(w.db.pool(), tracker, None, None)
        .await
        .unwrap();
    assert_eq!(days(live), [day(1), day(2)]);
}

#[tokio::test]
async fn purging_the_item_removes_its_tracker_and_entries() {
    let w = World::new().await;
    let tracker = w.tracker().await;
    repo::insert_entry(w.db.pool(), &w.entry(tracker, Decimal::ONE, day(1)), now())
        .await
        .unwrap();
    w.items.set_trashed(w.user, tracker, true).await.unwrap();
    let later = now() + TRASH_RETENTION + Duration::hours(1);
    assert_eq!(w.items.purge_expired(later).await.unwrap(), 1);

    assert_eq!(repo::get_tracker(w.db.pool(), tracker).await.unwrap(), None);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM tracker_entries")
        .fetch_one(w.db.pool())
        .await
        .unwrap();
    assert_eq!(left, 0);
}
