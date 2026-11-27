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

impl World {
    /// Ada's daily 08:00 routine shared with `people` as assignees, in `mode`.
    async fn shared_routine(
        &self,
        owner: Uuid,
        people: &[Uuid],
        mode: routines::counts::CompletionMode,
        rotation: Option<routines::rotation::RoutineRotation>,
    ) -> Uuid {
        let id = self
            .routines
            .create(
                owner,
                CreateRoutineRequest {
                    title: "Bulaşık".into(),
                    description: None,
                    timezone: "Europe/Istanbul".into(),
                    rrule: "FREQ=DAILY".into(),
                    daily_target: None,
                    start_date: Some(day(1)),
                    end_date: None,
                    reminder_times: Some(vec!["08:00".into()]),
                    completion_mode: Some(mode),
                    rotation,
                },
            )
            .await
            .unwrap()
            .item
            .id;
        for &person in people {
            let invitation = self
                .items
                .invite(
                    owner,
                    id,
                    CreateInvitationRequest {
                        to_user: person,
                        role: InvitationRole::Assignee,
                    },
                )
                .await
                .unwrap()
                .id;
            self.items
                .accept_invitation(person, invitation)
                .await
                .unwrap();
        }
        id
    }

    /// Whether `user` has a routine reminder in the app.
    async fn reminded(&self, user: Uuid) -> bool {
        self.notifications
            .list(user, None)
            .await
            .unwrap()
            .notifications
            .iter()
            .any(|n| matches!(n.payload, NotificationPayload::RoutineReminder { .. }))
    }
}

#[tokio::test]
async fn t3_3_4_a_shared_pool_reminds_every_responsible_person_once() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    w.shared_routine(
        ada,
        &[bob, can],
        routines::counts::CompletionMode::Pool,
        None,
    )
    .await;
    w.at(5, 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 2);
    assert_eq!(
        w.routines.send_due_reminders().await.unwrap(),
        0,
        "once per person"
    );
    w.deliver().await;
    assert!(w.reminded(bob).await && w.reminded(can).await);
    assert!(!w.reminded(ada).await, "the owner is not responsible here");
}

#[tokio::test]
async fn t3_3_4_a_complete_pool_day_reminds_nobody() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    let id = w
        .shared_routine(
            ada,
            &[bob, can],
            routines::counts::CompletionMode::Pool,
            None,
        )
        .await;
    w.at(4, 0);
    w.routines
        .complete(can, id, CompleteRoutineRequest::default())
        .await
        .unwrap();
    w.at(5, 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 0);
}

#[tokio::test]
async fn t3_3_4_each_person_mode_reminds_only_those_behind() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    let id = w
        .shared_routine(
            ada,
            &[bob, can],
            routines::counts::CompletionMode::Individual,
            None,
        )
        .await;
    w.at(4, 0);
    w.routines
        .complete(bob, id, CompleteRoutineRequest::default())
        .await
        .unwrap();
    w.at(5, 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 1);
    w.deliver().await;
    assert!(w.reminded(can).await);
    assert!(!w.reminded(bob).await, "Bob already did his part");
}

#[tokio::test]
async fn t3_3_4_a_rotation_reminds_the_person_on_duty_and_follows_swaps() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    let id = w
        .shared_routine(
            ada,
            &[bob, can],
            routines::counts::CompletionMode::Pool,
            Some(routines::rotation::RoutineRotation::EveryOccurrence),
        )
        .await;
    w.at(4, 0);
    let schedule = w
        .routines
        .rotation(
            ada,
            id,
            routines::rotation::RotationQuery {
                from: Some(day(27)),
                to: Some(day(28)),
            },
        )
        .await
        .unwrap();
    let today = schedule.assignments[0].user_id;
    let tomorrow = schedule.assignments[1].user_id;
    assert_ne!(today, tomorrow);
    // Swap today's turn with tomorrow's: tomorrow's person is on duty today.
    w.routines
        .swap(
            today,
            id,
            routines::rotation::SwapRequest {
                first: day(27),
                second: day(28),
            },
        )
        .await
        .unwrap();
    w.at(5, 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 1);
    w.deliver().await;
    assert!(w.reminded(tomorrow).await);
    assert!(!w.reminded(today).await);
}

#[tokio::test]
async fn t3_3_4_v9_someone_who_lost_access_is_not_reminded() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    let id = w
        .shared_routine(
            ada,
            &[bob, can],
            routines::counts::CompletionMode::Pool,
            None,
        )
        .await;
    w.items.remove_member(ada, id, can).await.unwrap();
    w.at(5, 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 1);
    w.deliver().await;
    assert!(w.reminded(bob).await);
    assert!(!w.reminded(can).await);
}

#[tokio::test]
async fn t3_3_4_bl3_each_person_has_their_own_channels() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    w.shared_routine(
        ada,
        &[bob, can],
        routines::counts::CompletionMode::Pool,
        None,
    )
    .await;
    w.notifications
        .set_preference(bob, NotificationKind::RoutineReminder, vec![Channel::Push])
        .await
        .unwrap();
    w.at(5, 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 2);
    w.deliver().await;
    assert!(!w.reminded(bob).await, "Bob turned in-app reminders off");
    assert!(w.reminded(can).await);
}

#[tokio::test]
async fn t3_3_4_reminders_already_sent_before_the_change_are_not_repeated() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let id = w.routine(ada, "Su iç", "FREQ=DAILY", &["08:00"]).await;
    // As the old sweep left it: one row per reminder, nothing per person.
    sqlx::query(
        "INSERT INTO routine_reminders_sent (routine_id, local_date, reminder_time, sent_at)
         VALUES ($1, $2, '08:00', now())",
    )
    .bind(id)
    .bind(day(27))
    .execute(w.db.pool())
    .await
    .unwrap();
    sqlx::query("DELETE FROM routine_reminders_sent_to")
        .execute(w.db.pool())
        .await
        .unwrap();
    sqlx::raw_sql(
        include_str!("../../../migrations/20261127000800_routine_reminders_sent_to.sql")
            .split("-- copy-sent-reminders")
            .nth(1)
            .unwrap(),
    )
    .execute(w.db.pool())
    .await
    .unwrap();
    w.at(5, 1);
    assert_eq!(w.routines.send_due_reminders().await.unwrap(), 0);
}

#[tokio::test]
async fn ak3_a_nudge_becomes_the_friends_notification() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let social = social::Friendships::new(w.db.pool().clone(), w.clock.clone(), w.identity.clone());
    social.send(ada, "bob@example.com").await.unwrap();
    let request = social.incoming(bob).await.unwrap()[0].id;
    social.accept(bob, request).await.unwrap();

    social.nudge(ada, bob).await.unwrap();
    w.deliver().await;
    w.redeliver_all().await;
    let list = w.notifications.list(bob, None).await.unwrap();
    assert_eq!(list.notifications.len(), 1, "once, even after a redelivery");
    assert_eq!(
        list.notifications[0].payload,
        NotificationPayload::Nudge {
            from_user: ada,
            from_name: "Ada".into(),
        }
    );
    assert!(
        w.notifications
            .list(ada, None)
            .await
            .unwrap()
            .notifications
            .is_empty()
    );
}

impl World {
    /// Ada's task shared with Bob as an assignee; Bob completes it. The completion event.
    async fn completion(&self, ada: Uuid, bob: Uuid) -> (Uuid, Uuid) {
        let task = self
            .items
            .create(
                ada,
                NewItem {
                    kind: "task".into(),
                    title: "Rapor".into(),
                    description: None,
                },
            )
            .await
            .unwrap()
            .id;
        let invitation = self
            .items
            .invite(
                ada,
                task,
                CreateInvitationRequest {
                    to_user: bob,
                    role: InvitationRole::Assignee,
                },
            )
            .await
            .unwrap()
            .id;
        self.items.accept_invitation(bob, invitation).await.unwrap();
        let mut tx = self.db.pool().begin().await.unwrap();
        self.items
            .record_completed(&mut tx, bob, task)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let page = kernel::PageRequest::from_params(kernel::PageParams {
            limit: Some(50),
            cursor: None,
        })
        .unwrap();
        let id = self
            .items
            .item_activity(ada, task, &page)
            .await
            .unwrap()
            .items
            .into_iter()
            .find(|a| a.kind == items::ActivityKind::Completed)
            .unwrap()
            .id;
        (task, id)
    }

    async fn reactions_of(&self, user: Uuid) -> Vec<NotificationPayload> {
        self.notifications
            .list(user, None)
            .await
            .unwrap()
            .notifications
            .into_iter()
            .map(|n| n.payload)
            .filter(|p| matches!(p, NotificationPayload::Reaction { .. }))
            .collect()
    }
}

#[tokio::test]
async fn ak4_a_reaction_becomes_the_completers_notification() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let (task, completion) = w.completion(ada, bob).await;
    let react = |e: &str| items::ReactRequest { emoji: e.into() };
    w.items.react(ada, completion, react("🎉")).await.unwrap();
    // The same emoji again changes nothing and makes no second notification.
    w.items.react(ada, completion, react("🎉")).await.unwrap();
    w.deliver().await;
    w.redeliver_all().await;
    assert_eq!(
        w.reactions_of(bob).await,
        [NotificationPayload::Reaction {
            activity_id: completion,
            item_id: task,
            item_kind: "task".into(),
            item_title: "Rapor".into(),
            from_user: ada,
            from_name: "Ada".into(),
            emoji: "🎉".into(),
        }]
    );
    assert!(w.reactions_of(ada).await.is_empty());
}

#[tokio::test]
async fn ak4_v9_no_reaction_notification_about_an_item_one_no_longer_sees() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let (task, completion) = w.completion(ada, bob).await;
    w.items
        .react(
            ada,
            completion,
            items::ReactRequest {
                emoji: "👏".into()
            },
        )
        .await
        .unwrap();
    // Bob leaves the task before the notification is made.
    w.items.remove_member(ada, task, bob).await.unwrap();
    w.deliver().await;
    assert!(w.reactions_of(bob).await.is_empty());
}
