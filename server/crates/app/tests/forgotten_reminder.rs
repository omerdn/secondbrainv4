//! Forgotten items reminder (T4.3.9, PRD BL2, BL3, H4) against a real Postgres 17. Requires
//! Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use app::forgotten::ForgottenReminders;
use chrono::{NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{ItemsAccessContext, ItemsService, NewItem};
use kernel::FixedClock;
use kernel::email::RecordingEmailSender;
use notebooks::collab::Collab;
use notebooks::process::ProcessPages;
use notifications::{NotificationKind, NotificationPayload, Notifications};
use routines::RoutinesService;
use uuid::Uuid;

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

struct World {
    db: TestDb,
    clock: FixedClock,
    identity: IdentityService,
    items: ItemsService,
    notifications: Notifications,
    reminders: ForgottenReminders,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        // Friday 13 November 2026.
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 13, 9, 0, 0).unwrap());
        let shared: kernel::SharedClock = Arc::new(clock.clone());
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(pool.clone(), shared.clone(), access.clone());
        let items_handle = items.clone();
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
        let reminders = ForgottenReminders {
            pool,
            clock: shared,
            identity: identity.clone(),
            today,
            notifications: notifications.clone(),
        };
        Self {
            db,
            clock,
            identity,
            items: items_handle,
            notifications,
            reminders,
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

    async fn item(&self, owner: Uuid, title: &str) -> Uuid {
        self.items
            .create(
                owner,
                NewItem {
                    kind: "task".into(),
                    title: title.into(),
                    description: None,
                },
            )
            .await
            .unwrap()
            .id
    }

    async fn reminders_of(&self, user: Uuid) -> Vec<NotificationPayload> {
        self.notifications
            .list(user, None)
            .await
            .unwrap()
            .notifications
            .into_iter()
            .map(|n| n.payload)
            .filter(|p| matches!(p, NotificationPayload::ForgottenReminder { .. }))
            .collect()
    }

    async fn queued(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'forgotten.remind'")
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn h4_untouched_items_are_reminded_once_a_day() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Europe/Istanbul").await;
    w.item(ada, "Eski rapor").await;
    // Ten days later, 10:15 in Istanbul (default threshold: 7 days).
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 7, 15, 0).unwrap());
    w.item(ada, "Yeni iş").await;
    w.reminders.send(ada, day(23)).await.unwrap();
    w.reminders.send(ada, day(23)).await.unwrap();
    assert_eq!(
        w.reminders_of(ada).await,
        [NotificationPayload::ForgottenReminder {
            local_date: day(23),
            count: 1,
            titles: vec!["Eski rapor".into()],
        }],
        "once a day, only the forgotten one"
    );
}

#[tokio::test]
async fn bl3_nothing_forgotten_or_the_channel_off_sends_nothing() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Europe/Istanbul").await;
    let bob = w.user("bob@example.com", "Europe/Istanbul").await;
    w.item(ada, "Eski rapor").await;
    w.notifications
        .set_preference(ada, NotificationKind::ForgottenReminder, Vec::new())
        .await
        .unwrap();
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 7, 15, 0).unwrap());
    w.item(bob, "Yeni iş").await;
    w.reminders.send(ada, day(23)).await.unwrap();
    w.reminders.send(bob, day(23)).await.unwrap();
    assert!(w.reminders_of(ada).await.is_empty(), "Ada turned it off");
    assert!(
        w.reminders_of(bob).await.is_empty(),
        "nothing of Bob is forgotten"
    );
}

#[tokio::test]
async fn h4_queued_at_each_users_local_morning_once_a_day() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Europe/Istanbul").await;
    w.user("bob@example.com", "America/New_York").await;
    // 07:00 UTC: 10:00 in Istanbul, 02:00 in New York.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 23, 7, 0, 0).unwrap());
    assert_eq!(w.reminders.tick().await.unwrap(), 1);
    assert_eq!(w.reminders.tick().await.unwrap(), 0, "once a day");
    let user: Uuid = sqlx::query_scalar(
        "SELECT (payload->>'user_id')::uuid FROM jobs WHERE kind = 'forgotten.remind'",
    )
    .fetch_one(w.db.pool())
    .await
    .unwrap();
    assert_eq!(user, ada);
    // The next day, again.
    w.clock
        .set(Utc.with_ymd_and_hms(2026, 11, 24, 7, 0, 0).unwrap());
    assert_eq!(w.reminders.tick().await.unwrap(), 1);
    assert_eq!(w.queued().await, 2);
}
