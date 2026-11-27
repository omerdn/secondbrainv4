//! Weekly summary (T4.3.5, PRD OZ1, BL3) against a real Postgres 17. Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use app::weekly::{WeeklySummaries, source_event};
use chrono::{NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{ItemsAccessContext, ItemsService};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use notebooks::collab::Collab;
use notebooks::process::ProcessPages;
use notifications::{Channel, NotificationKind, NotificationPayload, Notifications};
use routines::{CompleteRoutineRequest, CreateRoutineRequest, RoutinesService};
use uuid::Uuid;

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

struct World {
    db: TestDb,
    clock: FixedClock,
    identity: IdentityService,
    routines: RoutinesService,
    notifications: Notifications,
    weekly: WeeklySummaries,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        // Wednesday 18 November 2026, noon in Istanbul.
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 18, 9, 0, 0).unwrap());
        let shared: kernel::SharedClock = Arc::new(clock.clone());
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(pool.clone(), shared.clone(), access.clone());
        let identity = IdentityService::new(
            pool.clone(),
            shared.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        let routines = RoutinesService::new(
            pool.clone(),
            shared.clone(),
            access.permissions(),
            items.clone(),
        );
        let tasks = tasks::TasksService::new(
            pool.clone(),
            shared.clone(),
            access.permissions(),
            items.clone(),
        );
        let goals = goals::GoalsService::new(
            pool.clone(),
            shared.clone(),
            access.permissions(),
            items.clone(),
        );
        let sessions = sessions::WorkSessions::new(
            pool.clone(),
            shared.clone(),
            access.permissions(),
            realtime::Notifier::default(),
        );
        let notebooks = notebooks::NotebooksService::new(
            pool.clone(),
            shared.clone(),
            access.clone(),
            items.clone(),
        );
        let process = ProcessPages::new(
            pool.clone(),
            notebooks,
            (items.clone(), identity.clone()),
            Collab::new(pool.clone(), shared.clone()),
            &access,
        );
        let today = today::TodayService::new(
            pool.clone(),
            shared.clone(),
            items.clone(),
            tasks,
            routines.clone(),
            goals,
            process,
            sessions.clone(),
        );
        let notifications =
            Notifications::new(pool.clone(), shared.clone(), realtime::Notifier::default());
        let weekly = WeeklySummaries {
            pool,
            clock: shared,
            identity: identity.clone(),
            items,
            routines: routines.clone(),
            sessions,
            today,
            notifications: notifications.clone(),
        };
        Self {
            db,
            clock,
            identity,
            routines,
            notifications,
            weekly,
        }
    }

    async fn user(&self, email: &str, timezone: &str) -> Uuid {
        let request = RegisterRequest {
            email: email.into(),
            password: "correct horse".into(),
            display_name: "Ada".into(),
            client: ClientKind::Native,
            device: None,
            accepted_legal: identity::legal::current_acceptances(),
        }
        .validate()
        .unwrap();
        let (user, _) = self.identity.register(request).await.unwrap();
        sqlx::query("UPDATE users SET timezone = $2 WHERE id = $1")
            .bind(user.id.as_uuid())
            .bind(timezone)
            .execute(self.db.pool())
            .await
            .unwrap();
        user.id.as_uuid()
    }

    /// A daily routine started on the 1st, done on the 16th, 17th and 18th.
    async fn routine_done_three_days(&self, owner: Uuid) -> Uuid {
        let id = self
            .routines
            .create(
                owner,
                CreateRoutineRequest {
                    title: "Su iç".into(),
                    description: None,
                    timezone: "Europe/Istanbul".into(),
                    rrule: "FREQ=DAILY".into(),
                    daily_target: None,
                    start_date: Some(day(1)),
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
        for d in [16, 17, 18] {
            self.routines
                .complete(
                    owner,
                    id,
                    CompleteRoutineRequest {
                        date: Some(day(d)),
                        count: None,
                    },
                )
                .await
                .unwrap();
        }
        id
    }

    async fn emails(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'notifications.send_email'")
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }

    async fn queued(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'summary.weekly'")
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn oz1_last_weeks_summary_reaches_the_center_and_the_inbox_once() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Europe/Istanbul").await;
    w.routine_done_three_days(ada).await;
    // Monday 23 November, 09:10 in Istanbul.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 6, 10, 0).unwrap());
    w.weekly.send(ada, day(16)).await.unwrap();
    w.weekly.send(ada, day(16)).await.unwrap();

    let list = w.notifications.list(ada, None).await.unwrap();
    assert_eq!(
        list.notifications.len(),
        1,
        "once, even when the job runs twice"
    );
    let NotificationPayload::WeeklySummary(summary) = &list.notifications[0].payload else {
        panic!("expected a weekly summary");
    };
    assert_eq!((summary.week_start, summary.week_end), (day(16), day(22)));
    assert_eq!(summary.routines.len(), 1);
    assert_eq!(summary.routines[0].title, "Su iç");
    assert_eq!(summary.routines[0].done_days, 3);
    assert_eq!(summary.routines[0].scheduled_days, 7);
    // E-mail is a default channel of the summary (BL1), queued once per week.
    assert_eq!(w.emails().await, 1);
    let dedupe: String =
        sqlx::query_scalar("SELECT dedupe_key FROM jobs WHERE kind = 'notifications.send_email'")
            .fetch_one(w.db.pool())
            .await
            .unwrap();
    assert!(dedupe.contains(&source_event(ada, day(16)).to_string()));
}

#[tokio::test]
async fn bl3_the_summary_follows_the_channel_choice_and_an_empty_week_sends_nothing() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Europe/Istanbul").await;
    let bob = w.user("bob@example.com", "Europe/Istanbul").await;
    w.routine_done_three_days(ada).await;
    w.notifications
        .set_preference(ada, NotificationKind::WeeklySummary, vec![Channel::InApp])
        .await
        .unwrap();
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 6, 10, 0).unwrap());
    w.weekly.send(ada, day(16)).await.unwrap();
    w.weekly.send(bob, day(16)).await.unwrap();
    assert_eq!(
        w.notifications
            .list(ada, None)
            .await
            .unwrap()
            .notifications
            .len(),
        1
    );
    assert_eq!(w.emails().await, 0, "Ada turned the e-mail off");
    assert!(
        w.notifications
            .list(bob, None)
            .await
            .unwrap()
            .notifications
            .is_empty(),
        "Bob had nothing to tell"
    );
}

#[tokio::test]
async fn oz1_queued_on_each_users_own_monday_morning() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Europe/Istanbul").await;
    w.user("bob@example.com", "America/New_York").await;
    // 06:00 UTC on Monday: 09:00 in Istanbul, 01:00 in New York.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 6, 0, 0).unwrap());
    assert_eq!(w.weekly.tick().await.unwrap(), 1);
    assert_eq!(w.weekly.tick().await.unwrap(), 0, "once per week");
    let user: Uuid = sqlx::query_scalar(
        "SELECT (payload->>'user_id')::uuid FROM jobs WHERE kind = 'summary.weekly'",
    )
    .fetch_one(w.db.pool())
    .await
    .unwrap();
    assert_eq!(user, ada);
    // 14:00 UTC: 09:00 in New York.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 14, 0, 0).unwrap());
    assert_eq!(w.weekly.tick().await.unwrap(), 1);
    // Tuesday: nobody.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 24, 6, 0, 0).unwrap());
    assert_eq!(w.weekly.tick().await.unwrap(), 0);
    assert_eq!(w.queued().await, 2);
}
