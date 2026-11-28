//! The Today screen (T2.7.1, PRD H1–H6) against a real Postgres 17. Requires Docker (or
//! `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use goals::{CreateGoalRequest, GoalsService};
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{ItemsAccessContext, ItemsService, UpdateItemRequest};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use notebooks::NotebooksService;
use notebooks::collab::Collab;
use notebooks::process::ProcessPages;
use routines::{CompleteRoutineRequest, CreateRoutineRequest, RoutinesService};
use rust_decimal::Decimal;
use sessions::{OpenRequest, WorkSessions};
use tasks::{CreateTaskRequest, TasksService};
use today::{DEFAULT_FORGOTTEN_DAYS, TodayService};
use tracking::{
    Aggregation, CreateEntryRequest, CreateTrackerRequest, Direction, EntryNoted, Period,
    TrackerEntries,
};
use uuid::Uuid;

/// Thursday 26 November 2026, noon in Istanbul.
fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 26, 9, 0, 0).unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn today() -> NaiveDate {
    day(26)
}

/// Stand-in for the teams module: who is in which team; members may do anything (T5.2.7).
#[derive(Clone, Default)]
struct FakeTeams(Arc<std::sync::Mutex<Vec<(Uuid, Uuid)>>>);

impl items::TeamDirectory for FakeTeams {
    fn teams_of(&self, user: Uuid) -> items::TeamsFuture<'_> {
        let teams = self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, u)| *u == user)
            .map(|(t, _)| *t)
            .collect();
        Box::pin(async move { Ok(teams) })
    }
    fn is_member(&self, team: Uuid, user: Uuid) -> items::TeamFuture<'_> {
        let member = self.0.lock().unwrap().contains(&(team, user));
        Box::pin(async move { Ok(member) })
    }
    fn may(&self, actor: Uuid, team: Uuid, _action: items::TeamAction) -> items::TeamFuture<'_> {
        self.is_member(team, actor)
    }
}

struct World {
    db: TestDb,
    access: AccessEngine<ItemsAccessContext>,
    teams: FakeTeams,
    clock: Arc<FixedClock>,
    identity: IdentityService,
    items: ItemsService,
    tasks: TasksService,
    routines: RoutinesService,
    goals: GoalsService,
    process: ProcessPages,
    entries: TrackerEntries,
    sessions: WorkSessions,
    today: TodayService,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let clock = Arc::new(FixedClock::new(t0()));
        let identity = IdentityService::new(
            pool.clone(),
            clock.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "http://localhost:8081".into(),
            },
        );
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let teams = FakeTeams::default();
        let items = ItemsService::new(pool.clone(), clock.clone(), access.clone())
            .with_teams(Arc::new(teams.clone()));
        let notebooks =
            NotebooksService::new(pool.clone(), clock.clone(), access.clone(), items.clone());
        let collab = Collab::new(pool.clone(), clock.clone());
        let process = ProcessPages::new(
            pool.clone(),
            notebooks,
            (items.clone(), identity.clone()),
            collab,
            &access,
        );
        let entries = TrackerEntries::new(
            pool.clone(),
            clock.clone(),
            access.permissions(),
            items.clone(),
            identity.clone(),
        )
        .with_notes(Arc::new(process.clone()));
        let tasks = TasksService::new(
            pool.clone(),
            clock.clone(),
            access.permissions(),
            items.clone(),
        );
        let routines = RoutinesService::new(
            pool.clone(),
            clock.clone(),
            access.permissions(),
            items.clone(),
        );
        let goals = GoalsService::new(
            pool.clone(),
            clock.clone(),
            access.permissions(),
            items.clone(),
        )
        .with_progress("task", Arc::new(tasks::TaskProgress::new(pool.clone())));
        let sessions = WorkSessions::new(
            pool.clone(),
            clock.clone(),
            access.permissions(),
            realtime::Notifier::default(),
        );
        let today = TodayService::new(
            pool,
            clock.clone(),
            items.clone(),
            tasks.clone(),
            routines.clone(),
            goals.clone(),
            process.clone(),
            sessions.clone(),
        );
        Self {
            db,
            access,
            teams,
            clock,
            identity,
            items,
            tasks,
            routines,
            goals,
            process,
            entries,
            sessions,
            today,
        }
    }

    async fn user(&self, email: &str) -> Uuid {
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
        user.id.as_uuid()
    }

    async fn task(&self, owner: Uuid, title: &str, due: Option<NaiveDate>) -> Uuid {
        self.task_between(owner, title, None, due).await
    }

    async fn task_between(
        &self,
        owner: Uuid,
        title: &str,
        start: Option<NaiveDate>,
        due: Option<NaiveDate>,
    ) -> Uuid {
        self.tasks
            .create(
                owner,
                CreateTaskRequest {
                    team: None,
                    title: title.into(),
                    description: None,
                    start_date: start,
                    due_date: due,
                    subtasks: None,
                },
            )
            .await
            .unwrap()
            .item
            .id
    }

    async fn routine(&self, owner: Uuid, title: &str, rrule: &str) -> Uuid {
        self.routine_in(owner, title, rrule, "Europe/Istanbul")
            .await
    }

    async fn routine_in(&self, owner: Uuid, title: &str, rrule: &str, zone: &str) -> Uuid {
        self.routines
            .create(
                owner,
                CreateRoutineRequest {
                    team: None,
                    title: title.into(),
                    description: None,
                    timezone: zone.into(),
                    rrule: rrule.into(),
                    daily_target: Some(2),
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
            .id
    }

    async fn goal(&self, owner: Uuid, title: &str, start: NaiveDate, end: NaiveDate) -> Uuid {
        self.goals
            .create(
                owner,
                CreateGoalRequest {
                    team: None,
                    title: title.into(),
                    description: None,
                    start_date: Some(start),
                    end_date: Some(end),
                },
            )
            .await
            .unwrap()
            .item
            .id
    }

    async fn screen(&self, user: Uuid) -> today::Today {
        self.today.today(user, Some(today())).await.unwrap()
    }
}

fn titles(items: impl IntoIterator<Item = String>) -> Vec<String> {
    items.into_iter().collect()
}

#[tokio::test]
async fn h1_shows_routines_scheduled_today_with_what_was_done() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let daily = w.routine(ada, "Su iç", "FREQ=DAILY").await;
    w.routine(ada, "Haftalık rapor", "FREQ=WEEKLY;BYDAY=MO")
        .await;
    w.routines
        .complete(ada, daily, CompleteRoutineRequest::default())
        .await
        .unwrap();
    let screen = w.screen(ada).await;
    assert_eq!(
        screen.routines.len(),
        1,
        "Monday's routine is not due on a Thursday"
    );
    let routine = &screen.routines[0];
    assert_eq!(routine.item.id, daily);
    assert_eq!(routine.timezone, "Europe/Istanbul");
    assert_eq!(
        (routine.day.date, routine.day.done, routine.day.target),
        (today(), 1, 2)
    );
    assert!(!routine.day.complete);
}

#[tokio::test]
async fn h2_splits_unfinished_tasks_into_overdue_and_upcoming() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    w.task(ada, "Geciken", Some(day(20))).await;
    w.task(ada, "Bugün", Some(today())).await;
    w.task(ada, "Üç gün sonra", Some(day(29))).await;
    w.task(
        ada,
        "Uzak",
        Some(NaiveDate::from_ymd_opt(2026, 12, 20).unwrap()),
    )
    .await;
    w.task(ada, "Tarihsiz", None).await;
    let done = w.task(ada, "Bitmiş", Some(day(21))).await;
    let subtask = w.tasks.task(ada, done).await.unwrap().subtasks[0].id;
    w.tasks.complete_subtask(ada, done, subtask).await.unwrap();

    let screen = w.screen(ada).await;
    assert_eq!(
        titles(screen.overdue.iter().map(|t| t.item.title.clone())),
        ["Geciken"]
    );
    assert_eq!(
        titles(screen.upcoming.iter().map(|t| t.item.title.clone())),
        ["Bugün", "Üç gün sonra"]
    );
    assert_eq!((screen.overdue[0].done, screen.overdue[0].total), (0, 1));
}

#[tokio::test]
async fn h3_shows_running_goals_with_progress_and_pace() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let running = w
        .goal(
            ada,
            "Kitap yılı",
            day(1),
            NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
        )
        .await;
    w.goal(
        ada,
        "Bitti",
        NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
    )
    .await;
    w.goal(
        ada,
        "Gelecek",
        NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2027, 1, 31).unwrap(),
    )
    .await;
    let task = w.task(ada, "Oku", Some(day(30))).await;
    w.goals
        .link(ada, running, task, goals::LinkGoalRequest::default())
        .await
        .unwrap();
    let screen = w.screen(ada).await;
    assert_eq!(screen.goals.len(), 1);
    let goal = &screen.goals[0];
    assert_eq!(goal.item.id, running);
    assert!(goal.actual.abs() < f64::EPSILON, "nothing is done yet");
    assert!(goal.expected >= 0.0 && goal.pace >= 0.0);
}

#[tokio::test]
async fn h4_forgotten_items_follow_an_adjustable_threshold() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let old = w.task(ada, "Eski", None).await;
    let touched = w.task(ada, "Dokunulan", None).await;
    assert_eq!(
        w.today.preferences(ada).await.unwrap().forgotten_days,
        DEFAULT_FORGOTTEN_DAYS
    );
    // 8 days later; one task is edited (a touch), the other is not.
    w.clock.advance(Duration::days(8));
    w.items
        .update(
            ada,
            touched,
            UpdateItemRequest {
                title: Some("Dokunulan!".into()),
                description: None,
            }
            .validate()
            .unwrap(),
        )
        .await
        .unwrap();
    let screen = w.today.today(ada, Some(day(28))).await;
    // 8 days later the server's day is the 4th of December.
    assert!(matches!(screen, Err(AppError::Validation(_))));
    let screen = w
        .today
        .today(ada, Some(NaiveDate::from_ymd_opt(2026, 12, 4).unwrap()))
        .await
        .unwrap();
    assert_eq!(
        screen.forgotten.iter().map(|i| i.id).collect::<Vec<_>>(),
        [old]
    );
    assert_eq!(screen.forgotten_days, 7);

    // A longer threshold: nothing is forgotten yet.
    w.today.set_forgotten_days(ada, 10).await.unwrap();
    let screen = w.today.today(ada, None).await.unwrap();
    assert!(screen.forgotten.is_empty());
    assert_eq!(screen.forgotten_days, 10);
    for bad in [0, 366] {
        assert!(matches!(
            w.today.set_forgotten_days(ada, bad).await,
            Err(AppError::Validation(_))
        ));
    }
    assert_eq!(w.today.preferences(ada).await.unwrap().forgotten_days, 10);
}

#[tokio::test]
async fn h5_resume_lists_recent_items_with_their_status_lines() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let tracker = w
        .entries
        .create_tracker(
            ada,
            CreateTrackerRequest {
                team: None,
                title: "Gitar".into(),
                description: None,
                unit: "dk".into(),
                target_value: Decimal::from(30),
                period: Period::Day,
                aggregation: Aggregation::Sum,
                direction: Direction::Increase,
            },
        )
        .await
        .unwrap()
        .item
        .id;
    w.clock.advance(Duration::minutes(1));
    let task = w.task(ada, "Rapor", None).await;
    w.clock.advance(Duration::minutes(1));
    w.entries
        .add(
            ada,
            tracker,
            CreateEntryRequest {
                value: Decimal::from(20),
                occurred_at: None,
                note: Some("Barre'da kaldım".into()),
            },
        )
        .await
        .unwrap();
    let payloads: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM outbox WHERE event_type = 'tracking.entry_noted' ORDER BY id",
    )
    .fetch_all(w.db.pool())
    .await
    .unwrap();
    for payload in payloads {
        let event: EntryNoted = serde_json::from_value(payload).unwrap();
        w.process.add_note(&event, &w.entries).await.unwrap();
    }

    let screen = w.screen(ada).await;
    let resume: Vec<(Uuid, Option<String>)> = screen
        .resume
        .iter()
        .map(|r| (r.item.id, r.line.as_ref().map(|l| l.text.clone())))
        .collect();
    // The tracker was touched last (its entry), then the task.
    assert_eq!(resume[0].0, tracker);
    assert!(resume[0].1.as_deref().unwrap().contains("Barre'da kaldım"));
    assert!(resume.contains(&(task, None)));
}

#[tokio::test]
async fn h6_shows_the_open_work_session() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    assert!(w.screen(ada).await.session.is_none());
    let session = w.sessions.open(ada, OpenRequest::default()).await.unwrap();
    let glance = w.screen(ada).await.session.unwrap();
    assert_eq!(glance.id, session.id);
    assert_eq!(glance.server_time, t0());
    w.sessions.close(ada, session.id).await.unwrap();
    assert!(w.screen(ada).await.session.is_none());
}

#[tokio::test]
async fn only_what_the_caller_can_view_appears() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let bob = w.user("bob@example.com").await;
    w.task(bob, "Bob'un gecikeni", Some(day(20))).await;
    w.task(bob, "Bob'un yaklaşanı", Some(day(27))).await;
    w.routine(bob, "Bob'un rutini", "FREQ=DAILY").await;
    w.goal(bob, "Bob'un hedefi", day(1), day(30)).await;
    w.sessions.open(bob, OpenRequest::default()).await.unwrap();
    // Ada's archived task is not active either.
    let archived = w.task(ada, "Arşivde", Some(day(20))).await;
    w.items.set_archived(ada, archived, true).await.unwrap();
    w.clock.advance(Duration::hours(1));

    let screen = w.screen(ada).await;
    assert!(screen.routines.is_empty());
    assert!(screen.overdue.is_empty());
    assert!(screen.upcoming.is_empty());
    assert!(screen.goals.is_empty());
    assert!(screen.forgotten.is_empty());
    assert!(screen.resume.is_empty());
    assert!(screen.session.is_none());

    // Bob's own screen has his.
    let bobs = w.screen(bob).await;
    assert_eq!(bobs.overdue.len(), 1);
    assert_eq!(bobs.upcoming.len(), 1);
    assert_eq!(bobs.routines.len(), 1);
    assert_eq!(bobs.goals.len(), 1);
    assert!(bobs.session.is_some());
}

#[tokio::test]
async fn the_callers_day_must_be_close_to_the_servers() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    assert_eq!(w.today.today(ada, None).await.unwrap().today, today());
    assert_eq!(
        w.today.today(ada, Some(day(27))).await.unwrap().today,
        day(27)
    );
    assert!(matches!(
        w.today.today(ada, Some(day(28))).await,
        Err(AppError::Validation(_))
    ));
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
async fn c1_calendar_holds_tasks_whose_dates_touch_the_range() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    // The range: the week of 23-29 November.
    w.task_between(ada, "Biten", None, Some(day(23))).await;
    w.task_between(ada, "Başlayan", Some(day(29)), None).await;
    w.task_between(ada, "Kapsayan", Some(day(10)), Some(date(2026, 12, 10)))
        .await;
    w.task_between(ada, "Önceki", Some(day(1)), Some(day(22)))
        .await;
    w.task_between(ada, "Sonraki", Some(day(30)), None).await;
    w.task_between(ada, "Tarihsiz", None, None).await;
    let calendar = w.today.calendar(ada, day(23), day(29)).await.unwrap();
    let mut titles: Vec<&str> = calendar
        .tasks
        .iter()
        .map(|t| t.item.title.as_str())
        .collect();
    titles.sort_unstable();
    assert_eq!(titles, ["Başlayan", "Biten", "Kapsayan"]);
}

#[tokio::test]
async fn c1_routine_occurrences_come_from_the_plan_in_each_zone() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    w.routine(ada, "Pazartesi", "FREQ=WEEKLY;BYDAY=MO").await;
    // New York leaves DST on Sunday 1 November 2026: still one occurrence a day.
    w.routine_in(ada, "Günlük NY", "FREQ=DAILY", "America/New_York")
        .await;
    // Far ahead of UTC: its days are its own calendar dates all the same.
    w.routine_in(ada, "Günlük Kiritimati", "FREQ=DAILY", "Pacific/Kiritimati")
        .await;
    w.routine(ada, "Ayın 30'u", "FREQ=MONTHLY;BYMONTHDAY=30")
        .await;

    let calendar = w
        .today
        .calendar(ada, date(2026, 10, 30), date(2026, 11, 3))
        .await
        .unwrap();
    let dates = |title: &str| {
        calendar
            .routines
            .iter()
            .find(|r| r.item.title == title)
            .map(|r| r.dates.clone())
    };
    let every_day: Vec<NaiveDate> = date(2026, 10, 30).iter_days().take(5).collect();
    // The routines started on 1 November: nothing before it.
    assert_eq!(dates("Günlük NY").unwrap(), every_day[2..].to_vec());
    assert_eq!(dates("Günlük Kiritimati").unwrap(), every_day[2..].to_vec());
    assert_eq!(dates("Pazartesi").unwrap(), [date(2026, 11, 2)]);
    assert_eq!(
        calendar
            .routines
            .iter()
            .find(|r| r.item.title == "Günlük NY")
            .unwrap()
            .timezone,
        "America/New_York"
    );
    // No occurrence in the range: left out. Both ends are included.
    assert!(dates("Ayın 30'u").is_none());
    let edge = w.today.calendar(ada, day(30), day(30)).await.unwrap();
    // 30 November 2026 is a Monday: every routine occurs on it.
    assert_eq!(edge.routines.len(), 4);
    assert!(edge.routines.iter().all(|r| r.dates == [day(30)]));
}

#[tokio::test]
async fn c1_goal_ranges_that_overlap_the_range() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    w.goal(ada, "Kasım", day(1), day(30)).await;
    w.goal(ada, "Aralık", date(2026, 12, 1), date(2026, 12, 31))
        .await;
    w.goal(ada, "Ekim", date(2026, 10, 1), date(2026, 10, 31))
        .await;
    let calendar = w
        .today
        .calendar(ada, day(30), date(2026, 12, 2))
        .await
        .unwrap();
    let titles: Vec<&str> = calendar
        .goals
        .iter()
        .map(|g| g.item.title.as_str())
        .collect();
    assert_eq!(titles, ["Kasım", "Aralık"]);
    assert_eq!(calendar.goals[0].end_date, day(30));
}

#[tokio::test]
async fn c1_calendar_shows_only_what_the_caller_can_view() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let bob = w.user("bob@example.com").await;
    w.task_between(bob, "Bob", Some(day(24)), None).await;
    w.routine(bob, "Bob'un rutini", "FREQ=DAILY").await;
    w.goal(bob, "Bob'un hedefi", day(1), day(30)).await;
    let archived = w.task_between(ada, "Arşivde", Some(day(24)), None).await;
    w.items.set_archived(ada, archived, true).await.unwrap();
    let calendar = w.today.calendar(ada, day(23), day(29)).await.unwrap();
    assert!(calendar.tasks.is_empty());
    assert!(calendar.routines.is_empty());
    assert!(calendar.goals.is_empty());
    let bobs = w.today.calendar(bob, day(23), day(29)).await.unwrap();
    assert_eq!(
        (bobs.tasks.len(), bobs.routines.len(), bobs.goals.len()),
        (1, 1, 1)
    );
}

#[tokio::test]
async fn c1_calendar_range_is_checked() {
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    assert!(matches!(
        w.today.calendar(ada, day(26), day(25)).await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        w.today.calendar(ada, day(1), date(2027, 1, 2)).await,
        Err(AppError::Validation(_))
    ));
    // 62 days, both ends included: the longest range.
    assert!(
        w.today
            .calendar(ada, day(1), date(2027, 1, 1))
            .await
            .is_ok()
    );
}

// --- team dashboard (T5.2.7, TK6, V9) ------------------------------------------------------------

impl World {
    /// A task due on the 24th, placed in a team.
    async fn team_task(
        &self,
        owner: Uuid,
        title: &str,
        team: Option<items::TeamPlacementRequest>,
    ) -> Uuid {
        self.tasks
            .create(
                owner,
                CreateTaskRequest {
                    team,
                    title: title.into(),
                    description: None,
                    start_date: None,
                    due_date: Some(day(24)),
                    subtasks: None,
                },
            )
            .await
            .unwrap()
            .item
            .id
    }

    /// `members` join a new team: the fake directory and the access relations (T5.1.1).
    async fn team(&self, members: &[(Uuid, access::Relation)]) -> Uuid {
        let team = Uuid::now_v7();
        let mut tx = self.db.pool().begin().await.unwrap();
        for &(user, role) in members {
            self.teams.0.lock().unwrap().push((team, user));
            self.access
                .grant(&mut tx, access::RelationTuple::new(team, role, user))
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        team
    }
}

fn placed(
    team: Uuid,
    mode: items::TeamVisibilityMode,
    assignees: &[Uuid],
) -> Option<items::TeamPlacementRequest> {
    Some(items::TeamPlacementRequest {
        team_id: team,
        visibility: Some(mode),
        assignees: Some(assignees.to_vec()),
        watchers: None,
    })
}

#[tokio::test]
async fn tk6_team_dashboard_shows_only_the_teams_visible_items() {
    use items::TeamVisibilityMode::{All, Assignees};
    let w = World::new().await;
    let ada = w.user("ada@example.com").await;
    let bob = w.user("bob@example.com").await;
    let cem = w.user("cem@example.com").await;
    let team = w
        .team(&[
            (ada, access::Relation::TeamAdmin),
            (bob, access::Relation::TeamMember),
        ])
        .await;
    let late = w.team_task(ada, "Geç kalan", placed(team, All, &[])).await;
    let hidden = w
        .team_task(ada, "Gizli", placed(team, Assignees, &[ada]))
        .await;
    let personal = w.task(ada, "Kişisel", Some(day(24))).await;
    let goal = w
        .goals
        .create(
            ada,
            CreateGoalRequest {
                team: placed(team, All, &[]),
                title: "Çeyrek".into(),
                description: None,
                start_date: Some(day(20)),
                end_date: Some(day(30)),
            },
        )
        .await
        .unwrap()
        .item
        .id;
    let routine = w
        .routines
        .create(
            ada,
            CreateRoutineRequest {
                team: placed(team, All, &[]),
                title: "Stand-up".into(),
                description: None,
                timezone: "UTC".into(),
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
    for date in [day(24), day(25), day(26)] {
        w.routines
            .complete(
                ada,
                routine,
                routines::CompleteRoutineRequest {
                    date: Some(date),
                    count: None,
                },
            )
            .await
            .unwrap();
    }

    let board = w
        .today
        .team_dashboard(bob, team, Some(today()))
        .await
        .unwrap();
    assert_eq!(board.today, today());
    assert_eq!(
        board.overdue.iter().map(|t| t.item.id).collect::<Vec<_>>(),
        vec![late],
        "not the hidden task nor anyone's personal one"
    );
    assert_eq!(board.goals.len(), 1);
    assert_eq!(board.goals[0].item.id, goal);
    assert_eq!(board.adherence_days, 7);
    assert_eq!(board.routines.len(), 1);
    assert_eq!(board.routines[0].item.id, routine);
    assert_eq!(
        (board.routines[0].scheduled, board.routines[0].complete),
        (7, 3)
    );

    // Ada sees the task assigned to her too; her personal task is never the team's.
    let mine = w
        .today
        .team_dashboard(ada, team, Some(today()))
        .await
        .unwrap();
    let ids: Vec<Uuid> = mine.overdue.iter().map(|t| t.item.id).collect();
    assert!(ids.contains(&hidden) && ids.contains(&late) && !ids.contains(&personal));

    // Outsiders and unknown teams look the same: empty.
    for (who, which) in [(cem, team), (bob, Uuid::now_v7())] {
        let empty = w
            .today
            .team_dashboard(who, which, Some(today()))
            .await
            .unwrap();
        assert!(empty.goals.is_empty() && empty.routines.is_empty() && empty.overdue.is_empty());
    }
}
