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
        // Web Push on (T4.2.4); nothing is pushed until someone subscribes a browser.
        let notifications = Notifications::new(pool, clock.clone(), realtime::Notifier::default())
            .with_push_key("test-public-key");
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
            accepted_legal: identity::legal::current_acceptances(),
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
    // T3.7.1: an invitation waits for an answer, so it also goes by e-mail by default (BL1).
    assert_eq!(
        invitation.channels,
        [Channel::InApp, Channel::Push, Channel::Email]
    );
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
            activity_kind: Some("completed".into()),
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

// --- T3.7.1: invitation e-mails --------------------------------------------------------------

impl World {
    /// Runs the queued e-mail jobs with a recording sender; what was sent.
    async fn send_emails(&self) -> Vec<kernel::email::EmailMessage> {
        let sender = Arc::new(RecordingEmailSender::default());
        let mut worker = jobs::Worker::new(jobs::WorkerConfig::default());
        notifications::email::register_email(
            &mut worker,
            self.identity.clone(),
            sender.clone(),
            "http://localhost:8081".into(),
        );
        worker.run_batch(self.db.pool()).await.unwrap();
        sender.sent()
    }
}

#[tokio::test]
async fn bl1_an_invitation_is_mailed_once_in_turkish() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    w.invite(ada, bob, "Rapor").await;
    w.deliver().await;
    w.redeliver_all().await;
    let sent = w.send_emails().await;
    assert_eq!(sent.len(), 1, "once, even after a redelivery");
    assert_eq!(sent[0].to, "bob@example.com");
    assert_eq!(sent[0].kind, kernel::email::EmailKind::Invitation);
    assert_eq!(sent[0].subject, "Ada seni bir göreve davet etti");
    assert!(sent[0].text.contains("\"Rapor\""));
    assert!(sent[0].text.contains("http://localhost:8081/notifications"));
    // Nothing is left to send.
    assert!(w.send_emails().await.is_empty());
}

#[tokio::test]
async fn bl3_no_invitation_mail_without_the_email_channel() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    w.notifications
        .set_preference(bob, NotificationKind::Invitation, vec![Channel::InApp])
        .await
        .unwrap();
    w.invite(ada, bob, "Rapor").await;
    w.deliver().await;
    assert!(w.send_emails().await.is_empty());
    // The in-app notification still arrives.
    assert_eq!(
        w.notifications
            .list(bob, None)
            .await
            .unwrap()
            .notifications
            .len(),
        1
    );
}

#[tokio::test]
async fn bl1_a_friend_request_to_an_account_is_mailed_and_a_silenced_one_is_not() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    let social = social::Friendships::new(w.db.pool().clone(), w.clock.clone(), w.identity.clone());
    social.send(ada, "bob@example.com").await.unwrap();
    // An unknown address and a request Can's block silences send nothing (AK2).
    social.send(ada, "nobody@example.com").await.unwrap();
    social.block(can, ada).await.unwrap();
    social.send(ada, "can@example.com").await.unwrap();
    w.deliver().await;
    let sent = w.send_emails().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "bob@example.com");
    assert_eq!(sent[0].kind, kernel::email::EmailKind::FriendRequest);
    assert_eq!(sent[0].subject, "Ada seninle arkadaş olmak istiyor");
    assert!(sent[0].text.contains("http://localhost:8081/friends"));
}

// --- T4.2.4: Web Push ------------------------------------------------------------------------

mod web_push {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use notifications::push::{
        PushSubscriptionKeys, PushSubscriptionRequest, RecordingPushSender, VapidKey,
    };
    use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair};

    const CHROME: &str = "https://fcm.googleapis.com/fcm/send/chrome-1";
    const FIREFOX: &str = "https://updates.push.services.mozilla.com/wpush/v2/firefox-1";

    /// A browser's subscription with real keys.
    fn subscription(endpoint: &str) -> PushSubscriptionRequest {
        let browser = ES256KeyPair::generate();
        PushSubscriptionRequest {
            endpoint: endpoint.into(),
            keys: PushSubscriptionKeys {
                p256dh: URL_SAFE_NO_PAD
                    .encode(browser.public_key().public_key().to_bytes_uncompressed()),
                auth: URL_SAFE_NO_PAD.encode([9u8; 16]),
            },
        }
    }

    impl World {
        /// Runs the queued push jobs with `sender`.
        async fn send_pushes(&self, sender: &RecordingPushSender) {
            let mut worker = jobs::Worker::new(jobs::WorkerConfig::default());
            notifications::push::register_push(
                &mut worker,
                self.notifications.clone(),
                Arc::new(sender.clone()),
            );
            worker.run_batch(self.db.pool()).await.unwrap();
        }

        async fn subscriptions(&self, user: Uuid) -> i64 {
            sqlx::query_scalar("SELECT count(*) FROM push_subscriptions WHERE user_id = $1")
                .bind(user)
                .fetch_one(self.db.pool())
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn bl1_an_invitation_is_pushed_once_to_each_of_the_users_browsers() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        w.notifications
            .subscribe_push(bob, &subscription(CHROME))
            .await
            .unwrap();
        w.notifications
            .subscribe_push(bob, &subscription(FIREFOX))
            .await
            .unwrap();
        // Subscribing again (a page reload) keeps one row per browser.
        w.notifications
            .subscribe_push(bob, &subscription(CHROME))
            .await
            .unwrap();
        assert_eq!(w.subscriptions(bob).await, 2);
        w.invite(ada, bob, "Rapor").await;
        w.deliver().await;
        w.redeliver_all().await;
        let sender = RecordingPushSender::default();
        w.send_pushes(&sender).await;
        let mut sent = sender.sent();
        sent.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(sent.len(), 2, "once per browser, even after a redelivery");
        assert_eq!(sent[0].0, CHROME);
        assert_eq!(sent[1].0, FIREFOX);
        assert_eq!(sent[0].1.title, "Ada seni bir göreve davet etti");
        assert_eq!(sent[0].1.body, "Rapor");
        assert_eq!(sent[0].1.url, "/notifications");
        // Ada subscribed nothing and gets nothing.
        w.send_pushes(&sender).await;
        assert_eq!(sender.sent().len(), 2);
    }

    #[tokio::test]
    async fn bl3_push_follows_the_channel_preference() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let can = w.user("can@example.com", "Can").await;
        w.notifications
            .subscribe_push(bob, &subscription(CHROME))
            .await
            .unwrap();
        w.notifications
            .subscribe_push(can, &subscription(FIREFOX))
            .await
            .unwrap();
        // Bob: in the app only. Can: push only.
        w.notifications
            .set_preference(bob, NotificationKind::Invitation, vec![Channel::InApp])
            .await
            .unwrap();
        w.notifications
            .set_preference(can, NotificationKind::Invitation, vec![Channel::Push])
            .await
            .unwrap();
        w.invite(ada, bob, "Rapor").await;
        w.invite(ada, can, "Plan").await;
        w.deliver().await;
        let sender = RecordingPushSender::default();
        w.send_pushes(&sender).await;
        let sent = sender.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, FIREFOX);
        assert_eq!(sent[0].1.body, "Plan");
        assert_eq!(
            w.notifications
                .list(bob, None)
                .await
                .unwrap()
                .notifications
                .len(),
            1
        );
        assert!(
            w.notifications
                .list(can, None)
                .await
                .unwrap()
                .notifications
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_gone_subscription_is_forgotten() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        w.notifications
            .subscribe_push(bob, &subscription(CHROME))
            .await
            .unwrap();
        w.invite(ada, bob, "Rapor").await;
        w.deliver().await;
        let sender = RecordingPushSender::default();
        sender.mark_gone(CHROME);
        w.send_pushes(&sender).await;
        assert!(sender.sent().is_empty());
        assert_eq!(w.subscriptions(bob).await, 0);
        // The next notification queues nothing.
        w.invite(ada, bob, "Plan").await;
        w.deliver().await;
        let jobs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM jobs WHERE kind = 'notifications.send_push' AND completed_at IS NULL",
        )
        .fetch_one(w.db.pool())
        .await
        .unwrap();
        assert_eq!(jobs, 0);
    }

    #[tokio::test]
    async fn a_browser_notifies_only_whoever_subscribed_it_last() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        w.notifications
            .subscribe_push(ada, &subscription(CHROME))
            .await
            .unwrap();
        // Bob signs in on the same browser.
        w.notifications
            .subscribe_push(bob, &subscription(CHROME))
            .await
            .unwrap();
        assert_eq!(w.subscriptions(ada).await, 0);
        assert_eq!(w.subscriptions(bob).await, 1);
        // Ada cannot remove Bob's subscription, and learns nothing either way.
        w.notifications.unsubscribe_push(ada, CHROME).await.unwrap();
        assert_eq!(w.subscriptions(bob).await, 1);
        w.notifications.unsubscribe_push(bob, CHROME).await.unwrap();
        assert_eq!(w.subscriptions(bob).await, 0);
    }

    #[tokio::test]
    async fn only_browser_push_services_and_real_keys_are_stored() {
        let w = World::new().await;
        let bob = w.user("bob@example.com", "Bob").await;
        let mut internal = subscription("https://127.0.0.1/steal");
        assert!(matches!(
            w.notifications.subscribe_push(bob, &internal).await,
            Err(AppError::Validation(_))
        ));
        internal.endpoint = CHROME.into();
        internal.keys.auth = "x".into();
        assert!(matches!(
            w.notifications.subscribe_push(bob, &internal).await,
            Err(AppError::Validation(_))
        ));
        assert_eq!(w.subscriptions(bob).await, 0);
    }

    #[tokio::test]
    async fn without_vapid_no_key_is_offered_and_nothing_is_queued() {
        let w = World::new().await;
        assert_eq!(
            w.notifications.push_public_key().public_key.as_deref(),
            Some("test-public-key")
        );
        let off = Notifications::new(
            w.db.pool().clone(),
            w.clock.clone(),
            realtime::Notifier::default(),
        );
        assert_eq!(off.push_public_key().public_key, None);
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        off.subscribe_push(bob, &subscription(CHROME))
            .await
            .unwrap();
        off.deliver(
            bob,
            &NotificationPayload::Nudge {
                from_user: ada,
                from_name: "Ada".into(),
            },
            Uuid::now_v7(),
        )
        .await
        .unwrap();
        let sender = RecordingPushSender::default();
        w.send_pushes(&sender).await;
        assert!(sender.sent().is_empty());
        // A real key yields a 65-byte public key for `applicationServerKey`.
        let key = VapidKey::parse(&VapidKey::generate_private_key(), "mailto:a@b.c").unwrap();
        assert_eq!(URL_SAFE_NO_PAD.decode(key.public_key()).unwrap().len(), 65);
    }
}

// --- T4.3.7: assignment notifications ---------------------------------------------------------

mod assignments {
    use super::*;

    impl World {
        fn tasks(&self) -> tasks::TasksService {
            let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
            tasks::TasksService::new(
                self.db.pool().clone(),
                self.clock.clone(),
                access.permissions(),
                self.items.clone(),
            )
        }

        /// Ada's task with one subtask; Bob accepted to work on it. (task, subtask)
        async fn shared_task(&self, ada: Uuid, bob: Uuid) -> (Uuid, Uuid) {
            let view = self
                .tasks()
                .create(
                    ada,
                    tasks::CreateTaskRequest {
                        title: "Taşınma".into(),
                        description: None,
                        start_date: None,
                        due_date: None,
                        subtasks: Some(vec![tasks::CreateSubtaskRequest {
                            title: "Kutuları al".into(),
                        }]),
                    },
                )
                .await
                .unwrap();
            let task = view.item.id;
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
            (task, view.subtasks[0].id)
        }

        async fn assignments_of(&self, user: Uuid) -> Vec<NotificationPayload> {
            self.notifications
                .list(user, None)
                .await
                .unwrap()
                .notifications
                .into_iter()
                .map(|n| n.payload)
                .filter(|p| matches!(p, NotificationPayload::Assignment { .. }))
                .collect()
        }
    }

    #[tokio::test]
    async fn bl2_the_assigned_person_is_told_once_and_self_assignment_is_quiet() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let (task, subtask) = w.shared_task(ada, bob).await;
        w.tasks().assign(ada, task, subtask, bob).await.unwrap();
        w.tasks().assign(ada, task, subtask, ada).await.unwrap();
        w.deliver().await;
        w.redeliver_all().await;
        let got = w.assignments_of(bob).await;
        assert_eq!(got.len(), 1, "once, even after a redelivery");
        let NotificationPayload::Assignment {
            item_id,
            item_title,
            subtask_title,
            from_name,
            ..
        } = &got[0]
        else {
            unreachable!()
        };
        assert_eq!(*item_id, task);
        assert_eq!(item_title, "Taşınma");
        assert_eq!(subtask_title.as_deref(), Some("Kutuları al"));
        assert_eq!(from_name, "Ada");
        assert!(
            w.assignments_of(ada).await.is_empty(),
            "no notice of one's own act"
        );
    }

    #[tokio::test]
    async fn v9_no_assignment_notice_about_a_task_one_no_longer_sees() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let (task, subtask) = w.shared_task(ada, bob).await;
        w.tasks().assign(ada, task, subtask, bob).await.unwrap();
        // Bob is removed before the notification is made.
        w.items.remove_member(ada, task, bob).await.unwrap();
        w.deliver().await;
        assert!(w.assignments_of(bob).await.is_empty());
    }

    #[tokio::test]
    async fn bl3_assignment_notices_follow_the_channel_choice() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let (task, subtask) = w.shared_task(ada, bob).await;
        w.notifications
            .set_preference(bob, NotificationKind::Assignment, Vec::new())
            .await
            .unwrap();
        w.tasks().assign(ada, task, subtask, bob).await.unwrap();
        w.deliver().await;
        assert!(w.assignments_of(bob).await.is_empty());
    }
}

// --- T4.3.8: sharing change notifications -----------------------------------------------------

mod sharing_changes {
    use super::*;
    use notifications::SharingChangeKind;

    impl World {
        async fn sharing_changes_of(&self, user: Uuid) -> Vec<NotificationPayload> {
            self.notifications
                .list(user, None)
                .await
                .unwrap()
                .notifications
                .into_iter()
                .map(|n| n.payload)
                .filter(|p| matches!(p, NotificationPayload::SharingChange { .. }))
                .collect()
        }

        /// Ada's task with a description; Bob accepted to watch it.
        async fn watched_task(&self, ada: Uuid, bob: Uuid) -> Uuid {
            let item = self
                .items
                .create(
                    ada,
                    NewItem {
                        kind: "task".into(),
                        title: "Doğum günü".into(),
                        description: Some("Gizli plan: pasta".into()),
                    },
                )
                .await
                .unwrap()
                .id;
            let invitation = self
                .items
                .invite(
                    ada,
                    item,
                    CreateInvitationRequest {
                        to_user: bob,
                        role: InvitationRole::Watcher,
                    },
                )
                .await
                .unwrap()
                .id;
            self.items.accept_invitation(bob, invitation).await.unwrap();
            item
        }
    }

    #[tokio::test]
    async fn v9_someone_removed_learns_only_the_title_once() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let item = w.watched_task(ada, bob).await;
        w.items.remove_member(ada, item, bob).await.unwrap();
        w.deliver().await;
        w.redeliver_all().await;
        let got = w.sharing_changes_of(bob).await;
        assert_eq!(got.len(), 1, "once, even after a redelivery");
        assert_eq!(
            got[0],
            NotificationPayload::SharingChange {
                change: SharingChangeKind::Removed,
                item_id: None,
                item_title: "Doğum günü".into(),
                from_user: ada,
                from_name: "Ada".into(),
            }
        );
        let raw = serde_json::to_string(&got[0]).unwrap();
        assert!(!raw.contains("pasta") && !raw.contains(&item.to_string()));
    }

    #[tokio::test]
    async fn leaving_is_quiet_and_the_channel_choice_is_followed() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let can = w.user("can@example.com", "Can").await;
        let left = w.watched_task(ada, bob).await;
        w.items.remove_member(bob, left, bob).await.unwrap();
        let removed = w.watched_task(ada, can).await;
        w.notifications
            .set_preference(can, NotificationKind::SharingChange, Vec::new())
            .await
            .unwrap();
        w.items.remove_member(ada, removed, can).await.unwrap();
        w.deliver().await;
        assert!(
            w.sharing_changes_of(bob).await.is_empty(),
            "Bob left himself"
        );
        assert!(
            w.sharing_changes_of(can).await.is_empty(),
            "Can turned it off"
        );
        assert!(w.sharing_changes_of(ada).await.is_empty());
    }

    #[tokio::test]
    async fn bl2_a_new_owner_is_told() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let item = w.watched_task(ada, bob).await;
        w.items.transfer_ownership(ada, item, bob).await.unwrap();
        w.deliver().await;
        assert_eq!(
            w.sharing_changes_of(bob).await,
            [NotificationPayload::SharingChange {
                change: SharingChangeKind::BecameOwner,
                item_id: Some(item),
                item_title: "Doğum günü".into(),
                from_user: ada,
                from_name: "Ada".into(),
            }]
        );
    }
}

// --- T3.5.4: reactions to tracker entries -----------------------------------------------------

mod entry_reactions {
    use super::*;
    use rust_decimal::Decimal;
    use tracking::{
        Aggregation, CreateEntryRequest, CreateTrackerRequest, Direction, Period, TrackerEntries,
    };

    impl World {
        fn trackers(&self) -> TrackerEntries {
            let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
            TrackerEntries::new(
                self.db.pool().clone(),
                self.clock.clone(),
                access.permissions(),
                self.items.clone(),
                self.identity.clone(),
            )
        }

        /// Bob's tracker that Ada watches, with one entry of Bob's. The entry's activity id.
        async fn bobs_entry(&self, ada: Uuid, bob: Uuid) -> (Uuid, Uuid) {
            let trackers = self.trackers();
            let tracker = trackers
                .create_tracker(
                    bob,
                    CreateTrackerRequest {
                        title: "Koşu".into(),
                        description: None,
                        unit: "km".into(),
                        target_value: Decimal::from(20),
                        period: Period::Week,
                        aggregation: Aggregation::Sum,
                        direction: Direction::Increase,
                    },
                )
                .await
                .unwrap()
                .item
                .id;
            let invitation = self
                .items
                .invite(
                    bob,
                    tracker,
                    CreateInvitationRequest {
                        to_user: ada,
                        role: InvitationRole::Watcher,
                    },
                )
                .await
                .unwrap()
                .id;
            self.items.accept_invitation(ada, invitation).await.unwrap();
            trackers
                .add(
                    bob,
                    tracker,
                    CreateEntryRequest {
                        value: Decimal::from(5),
                        occurred_at: None,
                        note: None,
                    },
                )
                .await
                .unwrap();
            let activity: Uuid = sqlx::query_scalar(
                "SELECT id FROM activity_events WHERE item_id = $1 AND type = 'recorded'",
            )
            .bind(tracker)
            .fetch_one(self.db.pool())
            .await
            .unwrap();
            (tracker, activity)
        }
    }

    #[tokio::test]
    async fn ak4_a_friend_reacts_to_an_entry_and_the_recorder_is_told() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let (tracker, activity) = w.bobs_entry(ada, bob).await;
        w.items
            .react(
                ada,
                activity,
                items::ReactRequest {
                    emoji: "💪".into()
                },
            )
            .await
            .unwrap();
        w.deliver().await;
        w.redeliver_all().await;
        let got = w.reactions_of(bob).await;
        assert_eq!(
            got,
            [NotificationPayload::Reaction {
                activity_id: activity,
                item_id: tracker,
                item_kind: "tracker".into(),
                item_title: "Koşu".into(),
                from_user: ada,
                from_name: "Ada".into(),
                emoji: "💪".into(),
                activity_kind: Some("recorded".into()),
            }]
        );
    }

    #[tokio::test]
    async fn ak4_v9_no_reaction_to_ones_own_entry_or_an_unseen_one() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let can = w.user("can@example.com", "Can").await;
        let (_, activity) = w.bobs_entry(ada, bob).await;
        assert!(matches!(
            w.items
                .react(
                    bob,
                    activity,
                    items::ReactRequest {
                        emoji: "👏".into()
                    }
                )
                .await,
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            w.items
                .react(
                    can,
                    activity,
                    items::ReactRequest {
                        emoji: "👏".into()
                    }
                )
                .await,
            Err(AppError::NotFound)
        ));
    }
}

// --- T5.1.1: team invitations (TK2) -----------------------------------------------------------

mod team_invitations {
    use super::*;
    use teams::{CreateTeamRequest, InviteToTeamRequest, TeamRole, TeamsService};

    impl World {
        fn teams(&self) -> TeamsService {
            TeamsService::new(
                self.db.pool().clone(),
                self.clock.clone(),
                AccessEngine::new(ItemsAccessContext, AccessConfig::default()),
                self.identity.clone(),
            )
        }

        async fn deliver_teams(&self) {
            let mut dispatcher = Dispatcher::new(DispatchConfig::default());
            notifications::register_teams(
                &mut dispatcher,
                self.notifications.clone(),
                self.teams(),
                self.identity.clone(),
            );
            dispatcher.dispatch_batch(self.db.pool()).await.unwrap();
        }

        async fn team_of(&self, admin: Uuid) -> Uuid {
            self.teams()
                .create(
                    admin,
                    CreateTeamRequest {
                        name: "Ofis".into(),
                        color: "#4f46e5".into(),
                        logo_url: None,
                    },
                )
                .await
                .unwrap()
                .id
        }
    }

    #[tokio::test]
    async fn tk2_the_invited_person_is_told_and_a_declined_one_is_not() {
        let w = World::new().await;
        let ada = w.user("ada@example.com", "Ada").await;
        let bob = w.user("bob@example.com", "Bob").await;
        let can = w.user("can@example.com", "Can").await;
        let team = w.team_of(ada).await;
        for (to, role) in [(bob, TeamRole::Manager), (can, TeamRole::Member)] {
            w.teams()
                .invite(ada, team, InviteToTeamRequest { user_id: to, role })
                .await
                .unwrap();
        }
        // Can declines before the notification is made.
        w.teams().decline(can, team).await.unwrap();
        w.deliver_teams().await;
        let bobs = w.notifications.list(bob, None).await.unwrap().notifications;
        assert_eq!(bobs.len(), 1);
        assert_eq!(
            bobs[0].payload,
            NotificationPayload::TeamInvitation {
                team_id: team,
                team_name: "Ofis".into(),
                role: TeamRole::Manager,
                from_user: ada,
                from_name: "Ada".into(),
            }
        );
        assert!(
            w.notifications
                .list(can, None)
                .await
                .unwrap()
                .notifications
                .is_empty()
        );
    }
}
