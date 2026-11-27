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
    identity: IdentityService,
    items: ItemsService,
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
        let items =
            ItemsService::new(pool.clone(), clock.clone(), access).with_sharing(Arc::new(Anyone));
        let notifications = Notifications::new(pool, clock, realtime::Notifier::default());
        let mut dispatcher = Dispatcher::new(DispatchConfig::default());
        notifications::register(
            &mut dispatcher,
            notifications.clone(),
            items.clone(),
            identity.clone(),
        );
        Self {
            db,
            identity,
            items,
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
