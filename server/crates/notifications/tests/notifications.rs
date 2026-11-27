//! Notification center (T2.8.1, PRD BL1–BL3) against a real Postgres 17. Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{TimeZone, Utc};
use db::outbox::{DispatchConfig, Dispatcher};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{
    CreateInvitationRequest, InvitationRole, ItemsAccessContext, ItemsService, NewItem,
    SharingFuture, SharingPolicy,
};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use notifications::{Channel, NotificationKind, NotificationPayload, Notifications};
use routines::{CompleteRoutineRequest, CreateRoutineRequest, RoutinesService};
use uuid::Uuid;

/// Everyone may share with everyone (friendships come later).
struct Anyone;

impl SharingPolicy for Anyone {
    fn can_share(&self, _from: Uuid, _to: Uuid) -> SharingFuture<'_> {
        Box::pin(async { Ok(true) })
    }
}

struct World {
    db: TestDb,
    clock: Arc<FixedClock>,
    identity: IdentityService,
    items: ItemsService,
    routines: RoutinesService,
    notifications: Notifications,
    dispatcher: Dispatcher,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let clock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 27, 9, 0, 0).unwrap(),
        ));
        let identity = IdentityService::new(
            pool.clone(),
            clock.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "http://localhost:8081".into(),
            },
        );
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items = ItemsService::new(pool.clone(), clock.clone(), access.clone())
            .with_sharing(Arc::new(Anyone));
        let routines = RoutinesService::new(
            pool.clone(),
            clock.clone(),
            access.permissions(),
            items.clone(),
        );
        let notifications = Notifications::new(pool, clock.clone(), realtime::Notifier::default());
        let mut dispatcher = Dispatcher::new(DispatchConfig::default());
        notifications::register(
            &mut dispatcher,
            notifications.clone(),
            items.clone(),
            identity.clone(),
        );
        Self {
            db,
            clock,
            identity,
            items,
            routines,
            notifications,
            dispatcher,
        }
    }

    async fn user(&self, email: &str, name: &str) -> Uuid {
        let request = RegisterRequest {
            email: email.into(),
            password: "correct horse".into(),
            display_name: name.into(),
            client: ClientKind::Native,
            device: None,
        }
        .validate()
        .unwrap();
        let (user, _) = self.identity.register(request).await.unwrap();
        user.id.as_uuid()
    }

    async fn invite(&self, from: Uuid, to: Uuid, title: &str) -> (Uuid, Uuid) {
        let item = self
            .items
            .create(
                from,
                NewItem {
                    kind: "task".into(),
                    title: title.into(),
                    description: None,
                },
            )
            .await
            .unwrap()
            .id;
        let invitation = self
            .items
            .invite(
                from,
                item,
                CreateInvitationRequest {
                    to_user: to,
                    role: InvitationRole::Watcher,
                },
            )
            .await
            .unwrap()
            .id;
        (item, invitation)
    }

    async fn deliver(&self) {
        self.dispatcher
            .dispatch_batch(self.db.pool())
            .await
            .unwrap();
    }

    /// Delivers every event again, as a retry would.
    async fn redeliver_all(&self) {
        sqlx::query("UPDATE outbox SET dispatched_at = NULL")
            .execute(self.db.pool())
            .await
            .unwrap();
        self.deliver().await;
    }

    /// Delivers every `invitation.created` event again, as a retry would.
    async fn redeliver(&self) {
        sqlx::query(
            "UPDATE outbox SET dispatched_at = NULL WHERE event_type = 'invitation.created'",
        )
        .execute(self.db.pool())
        .await
        .unwrap();
        self.deliver().await;
    }
}

#[tokio::test]
async fn an_invitation_becomes_the_invitees_notification() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let (item, invitation) = w.invite(ada, bob, "Ortak liste").await;
    w.deliver().await;

    let list = w.notifications.list(bob, None).await.unwrap();
    assert_eq!(list.unread, 1);
    assert_eq!(list.notifications.len(), 1);
    assert_eq!(
        list.notifications[0].payload,
        NotificationPayload::Invitation {
            invitation_id: invitation,
            item_id: item,
            item_kind: "task".into(),
            item_title: "Ortak liste".into(),
            from_user: ada,
            from_name: "Ada".into(),
            role: InvitationRole::Watcher,
        }
    );
    // The inviter gets nothing.
    assert!(
        w.notifications
            .list(ada, None)
            .await
            .unwrap()
            .notifications
            .is_empty()
    );
}

#[tokio::test]
async fn the_same_event_never_makes_two_notifications() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    w.invite(ada, bob, "Ortak liste").await;
    w.deliver().await;
    w.redeliver().await;
    // Delivering the same event by hand twice: still one.
    let payload = NotificationPayload::Invitation {
        invitation_id: Uuid::now_v7(),
        item_id: Uuid::now_v7(),
        item_kind: "task".into(),
        item_title: "X".into(),
        from_user: ada,
        from_name: "Ada".into(),
        role: InvitationRole::Watcher,
    };
    let event = Uuid::now_v7();
    assert!(
        w.notifications
            .deliver(bob, &payload, event)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        w.notifications
            .deliver(bob, &payload, event)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        w.notifications
            .list(bob, None)
            .await
            .unwrap()
            .notifications
            .len(),
        2
    );
}

#[tokio::test]
async fn an_answered_invitation_makes_no_notification() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let (_, invitation) = w.invite(ada, bob, "Ortak liste").await;
    w.items.decline_invitation(bob, invitation).await.unwrap();
    w.deliver().await;
    assert!(
        w.notifications
            .list(bob, None)
            .await
            .unwrap()
            .notifications
            .is_empty()
    );
}

#[tokio::test]
async fn bl3_channels_are_chosen_per_type() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let prefs = w.notifications.preferences(bob).await.unwrap();
    assert_eq!(prefs.len(), NotificationKind::ALL.len());
    let invitation = prefs
        .iter()
        .find(|p| p.kind == NotificationKind::Invitation)
        .unwrap();
    assert!(invitation.default);
    assert_eq!(invitation.channels, [Channel::InApp, Channel::Push]);
    let summary = prefs
        .iter()
        .find(|p| p.kind == NotificationKind::WeeklySummary)
        .unwrap();
    assert!(summary.channels.contains(&Channel::Email));

    // Bob turns invitations off in the app: no notification; push stays his choice.
    let set = w
        .notifications
        .set_preference(
            bob,
            NotificationKind::Invitation,
            vec![Channel::Push, Channel::Push],
        )
        .await
        .unwrap();
    assert_eq!(set.channels, [Channel::Push]);
    w.invite(ada, bob, "Sessiz").await;
    w.deliver().await;
    assert!(
        w.notifications
            .list(bob, None)
            .await
            .unwrap()
            .notifications
            .is_empty()
    );
    let prefs = w.notifications.preferences(bob).await.unwrap();
    let invitation = prefs
        .iter()
        .find(|p| p.kind == NotificationKind::Invitation)
        .unwrap();
    assert!(!invitation.default);
    // Another user's preferences are untouched.
    assert!(
        w.notifications
            .preferences(ada)
            .await
            .unwrap()
            .iter()
            .all(|p| p.default)
    );
}

#[tokio::test]
async fn only_the_recipient_sees_and_reads_a_notification() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let cem = w.user("cem@example.com", "Cem").await;
    w.invite(ada, bob, "Bir").await;
    w.invite(ada, bob, "İki").await;
    w.deliver().await;
    let list = w.notifications.list(bob, None).await.unwrap();
    assert_eq!(list.unread, 2);
    let first = list.notifications[0].id;

    // Someone else's notification: not found, and nothing changes.
    assert!(matches!(
        w.notifications.mark_read(cem, first).await,
        Err(AppError::NotFound)
    ));
    assert!(
        w.notifications
            .list(cem, None)
            .await
            .unwrap()
            .notifications
            .is_empty()
    );
    assert_eq!(w.notifications.list(bob, None).await.unwrap().unread, 2);

    let read = w.notifications.mark_read(bob, first).await.unwrap();
    assert!(read.read_at.is_some());
    assert_eq!(w.notifications.list(bob, None).await.unwrap().unread, 1);
    assert_eq!(w.notifications.mark_all_read(bob).await.unwrap(), 1);
    assert_eq!(w.notifications.list(bob, None).await.unwrap().unread, 0);
}

fn day(d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

impl World {
    async fn routine(&self, owner: Uuid, title: &str, rrule: &str, times: &[&str]) -> Uuid {
        self.routines
            .create(
                owner,
                CreateRoutineRequest {
                    title: title.into(),
                    description: None,
                    timezone: "Europe/Istanbul".into(),
                    rrule: rrule.into(),
                    daily_target: None,
                    start_date: Some(day(1)),
                    end_date: None,
                    reminder_times: Some(times.iter().map(|t| (*t).to_owned()).collect()),
                    completion_mode: None,
                    rotation: None,
                },
            )
            .await
            .unwrap()
            .item
            .id
    }

    /// Moves the clock to `hh:mm` UTC on 27 November 2026 (a Friday).
    fn at(&self, hour: u32, minute: u32) {
        self.clock
            .set(Utc.with_ymd_and_hms(2026, 11, 27, hour, minute, 0).unwrap());
    }
}

#[tokio::test]
async fn r13_a_reminder_goes_out_once_at_the_routines_local_time() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let routine = w.routine(ada, "Su iç", "FREQ=DAILY", &["08:00"]).await;
    // 04:59 UTC is 07:59 in Istanbul: not yet.
    w.at(4, 59);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 0);
    // 08:02 in Istanbul: due; the next sweep sends nothing more.
    w.at(5, 2);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 0);
    w.at(5, 3);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 0);

    w.deliver().await;
    w.redeliver_all().await;
    let list = w.notifications.list(ada, None).await.unwrap();
    assert_eq!(list.notifications.len(), 1);
    assert_eq!(
        list.notifications[0].payload,
        NotificationPayload::RoutineReminder {
            routine_id: routine,
            title: "Su iç".into(),
            local_date: day(27),
            time: "08:00".into(),
        }
    );
}

#[tokio::test]
async fn r13_no_reminder_on_a_day_off_or_a_complete_day() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    // Mondays only; the 27th is a Friday.
    w.routine(ada, "Haftalık", "FREQ=WEEKLY;BYDAY=MO", &["08:00"])
        .await;
    let done = w.routine(ada, "Yapıldı", "FREQ=DAILY", &["08:00"]).await;
    w.at(4, 0);
    w.routines
        .complete(ada, done, CompleteRoutineRequest::default())
        .await
        .unwrap();
    w.at(5, 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 0);
}

#[tokio::test]
async fn bl3_reminders_follow_the_channel_preference() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    w.routine(ada, "Su iç", "FREQ=DAILY", &["08:00", "09:00"])
        .await;
    w.notifications
        .set_preference(ada, NotificationKind::RoutineReminder, vec![Channel::Push])
        .await
        .unwrap();
    w.at(5, 0);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 1);
    w.deliver().await;
    assert!(
        w.notifications
            .list(ada, None)
            .await
            .unwrap()
            .notifications
            .is_empty()
    );
    // Back on in the app: the 09:00 one arrives.
    w.notifications
        .set_preference(ada, NotificationKind::RoutineReminder, vec![Channel::InApp])
        .await
        .unwrap();
    w.at(6, 0);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 1);
    w.deliver().await;
    assert_eq!(
        w.notifications
            .list(ada, None)
            .await
            .unwrap()
            .notifications
            .len(),
        1
    );
}
