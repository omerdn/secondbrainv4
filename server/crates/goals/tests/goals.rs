//! Goals and goal links (T2.3.1, PRD D1-D3, V4, V9) against a real Postgres 17. Requires Docker
//! (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use access::{AccessConfig, AccessEngine, Relation, RelationTuple};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use db::testing::TestDb;
use goals::{
    CreateGoalRequest, Goal, GoalProgressQuery, GoalsService, LinkGoalRequest, UpdateGoalRequest,
};
use items::{ItemsAccessContext, ItemsService, NewItem};
use kernel::{AppError, FixedClock};
use rust_decimal::Decimal;
use tracking::{DateRange, Progress, ProgressFuture, ProgressProvider};
use uuid::Uuid;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 12, d).unwrap()
}

fn request(start: Option<NaiveDate>, end: Option<NaiveDate>) -> CreateGoalRequest {
    CreateGoalRequest {
        title: "Kitap".into(),
        description: None,
        start_date: start,
        end_date: end,
    }
}

fn weight(w: &str) -> LinkGoalRequest {
    LinkGoalRequest {
        weight: Some(w.parse().unwrap()),
    }
}

/// Answers with fixed figures per item and remembers the ranges it was asked about.
#[derive(Default)]
struct Fixed {
    figures: Mutex<HashMap<Uuid, Progress>>,
    asked: Mutex<Vec<(Uuid, DateRange, NaiveDate)>>,
}

impl Fixed {
    fn set(&self, item: Uuid, done: f64, total: f64, expected_to_date: f64) {
        self.figures.lock().unwrap().insert(
            item,
            Progress {
                done,
                total,
                expected_to_date,
            },
        );
    }
}

impl ProgressProvider for Fixed {
    fn progress(&self, item: Uuid, range: DateRange, today: NaiveDate) -> ProgressFuture<'_> {
        self.asked.lock().unwrap().push((item, range, today));
        let found = self.figures.lock().unwrap().get(&item).copied();
        Box::pin(async move { found.ok_or(AppError::NotFound) })
    }
}

struct World {
    db: TestDb,
    fixed: Arc<Fixed>,
    access: AccessEngine<ItemsAccessContext>,
    items: ItemsService,
    goals: GoalsService,
    ali: Uuid,
    ayse: Uuid,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let clock = Arc::new(FixedClock::new(now()));
        let items = ItemsService::new(db.pool().clone(), clock.clone(), access.clone());
        let fixed = Arc::new(Fixed::default());
        let goals = GoalsService::new(
            db.pool().clone(),
            clock,
            access.permissions(),
            items.clone(),
        )
        .with_progress("task", fixed.clone())
        .with_progress("routine", fixed.clone());
        Self {
            db,
            fixed,
            access,
            items,
            goals,
            ali: Uuid::now_v7(),
            ayse: Uuid::now_v7(),
        }
    }

    async fn goal(&self) -> Goal {
        self.goals
            .create(self.ali, request(Some(day(1)), Some(day(31))))
            .await
            .unwrap()
    }

    async fn item(&self, owner: Uuid, kind: &str) -> Uuid {
        self.items
            .create(
                owner,
                NewItem {
                    kind: kind.into(),
                    title: format!("a {kind}"),
                    description: None,
                },
            )
            .await
            .unwrap()
            .id
    }

    async fn grant(&self, tuple: RelationTuple) {
        let mut tx = self.db.pool().begin().await.unwrap();
        self.access.grant(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }
}

fn kinds(goal: &Goal) -> Vec<(&str, bool, String)> {
    goal.links
        .iter()
        .map(|l| (l.item.kind.as_str(), l.contributes, l.weight.to_string()))
        .collect()
}

#[tokio::test]
async fn d1_start_and_end_dates_are_required() {
    let w = World::new().await;
    for (start, end) in [
        (None, Some(day(2))),
        (Some(day(1)), None),
        (None, None),
        (Some(day(3)), Some(day(2))),
    ] {
        assert!(matches!(
            w.goals.create(w.ali, request(start, end)).await,
            Err(AppError::Validation(_))
        ));
    }
    let goal = w.goal().await;
    assert_eq!((goal.start_date, goal.end_date), (day(1), day(31)));
    assert_eq!(goal.item.kind, "goal");
    assert!(goal.links.is_empty());
    assert_eq!(w.goals.goal(w.ali, goal.item.id).await.unwrap(), goal);
}

#[tokio::test]
async fn d1_dates_move_but_never_clear_or_cross() {
    let w = World::new().await;
    let id = w.goal().await.item.id;
    let moved = w
        .goals
        .update(
            w.ali,
            id,
            UpdateGoalRequest {
                start_date: None,
                end_date: Some(day(10)),
            },
        )
        .await
        .unwrap();
    assert_eq!((moved.start_date, moved.end_date), (day(1), day(10)));
    let crossed = UpdateGoalRequest {
        start_date: Some(day(11)),
        end_date: None,
    };
    assert!(matches!(
        w.goals.update(w.ali, id, crossed).await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        w.goals
            .update(w.ali, id, UpdateGoalRequest::default())
            .await,
        Err(AppError::Validation(_))
    ));
}

#[tokio::test]
async fn d2_tasks_routines_trackers_and_pages_link_and_pages_do_not_count() {
    let w = World::new().await;
    let goal = w.goal().await.item.id;
    for kind in ["task", "routine", "tracker", "page"] {
        let item = w.item(w.ali, kind).await;
        w.goals
            .link(w.ali, goal, item, LinkGoalRequest::default())
            .await
            .unwrap();
    }
    let read = w.goals.goal(w.ali, goal).await.unwrap();
    let mut linked = kinds(&read);
    linked.sort();
    assert_eq!(
        linked,
        vec![
            ("page", false, "1".into()),
            ("routine", true, "1".into()),
            ("task", true, "1".into()),
            ("tracker", true, "1".into()),
        ]
    );
    // Neither a goal nor a notebook can be linked.
    for kind in ["goal", "notebook"] {
        let other = w.item(w.ali, kind).await;
        assert!(matches!(
            w.goals
                .link(w.ali, goal, other, LinkGoalRequest::default())
                .await,
            Err(AppError::Validation(_))
        ));
    }
}

#[tokio::test]
async fn d3_weights_are_set_kept_and_reset_with_the_link() {
    let w = World::new().await;
    let goal = w.goal().await.item.id;
    let task = w.item(w.ali, "task").await;
    let page = w.item(w.ali, "page").await;
    let linked = w
        .goals
        .link(w.ali, goal, task, weight("2.50"))
        .await
        .unwrap();
    assert_eq!(linked.links[0].weight, Decimal::new(25, 1));
    // Linking again without a weight keeps it; the link is not duplicated.
    let again = w
        .goals
        .link(w.ali, goal, task, LinkGoalRequest::default())
        .await
        .unwrap();
    assert_eq!(again.links.len(), 1);
    assert_eq!(again.links[0].weight, Decimal::new(25, 1));
    for bad in ["0", "-1", "100.01", "1.005"] {
        assert!(matches!(
            w.goals.link(w.ali, goal, task, weight(bad)).await,
            Err(AppError::Validation(_))
        ));
    }
    // A page takes no weight (D2).
    assert!(matches!(
        w.goals.link(w.ali, goal, page, weight("2")).await,
        Err(AppError::Validation(_))
    ));
    let removed = w.goals.unlink(w.ali, goal, task).await.unwrap();
    assert!(removed.links.is_empty());
    // Removing twice is harmless, and a new link starts from 1.
    w.goals.unlink(w.ali, goal, task).await.unwrap();
    let relinked = w
        .goals
        .link(w.ali, goal, task, LinkGoalRequest::default())
        .await
        .unwrap();
    assert_eq!(relinked.links[0].weight, Decimal::ONE);
}

#[tokio::test]
async fn v9_links_to_items_the_viewer_cannot_see_are_not_shown_or_counted() {
    let w = World::new().await;
    let goal = w.goal().await.item.id;
    let private = w.item(w.ali, "task").await;
    w.goals
        .link(w.ali, goal, private, weight("3"))
        .await
        .unwrap();
    w.grant(RelationTuple::new(goal, Relation::Watcher, w.ayse))
        .await;
    let seen = w.goals.goal(w.ayse, goal).await.unwrap();
    assert!(seen.links.is_empty());
    // Once Ayşe can see the task, the link and its weight appear.
    w.grant(RelationTuple::new(private, Relation::Watcher, w.ayse))
        .await;
    let seen = w.goals.goal(w.ayse, goal).await.unwrap();
    assert_eq!(kinds(&seen), vec![("task", true, "3".into())]);
}

#[tokio::test]
async fn linking_needs_edit_on_the_goal_and_sight_of_the_item() {
    let w = World::new().await;
    let goal = w.goal().await.item.id;
    let hers = w.item(w.ayse, "task").await;
    // Ali cannot see Ayşe's task: it is not found, not forbidden (V9).
    assert!(matches!(
        w.goals
            .link(w.ali, goal, hers, LinkGoalRequest::default())
            .await,
        Err(AppError::NotFound)
    ));
    // Seeing the goal is not enough to add to it.
    w.grant(RelationTuple::new(goal, Relation::Watcher, w.ayse))
        .await;
    assert!(matches!(
        w.goals
            .link(w.ayse, goal, hers, LinkGoalRequest::default())
            .await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        w.goals
            .update(
                w.ayse,
                goal,
                UpdateGoalRequest {
                    start_date: Some(day(2)),
                    end_date: None
                }
            )
            .await,
        Err(AppError::Forbidden)
    ));
    // A stranger learns nothing.
    let stranger = Uuid::now_v7();
    assert!(matches!(
        w.goals.goal(stranger, goal).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.goals
            .link(stranger, goal, hers, LinkGoalRequest::default())
            .await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.goals.unlink(stranger, goal, hers).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn an_item_of_another_kind_is_not_a_goal_and_a_trashed_goal_is_frozen() {
    let w = World::new().await;
    let task = w.item(w.ali, "task").await;
    assert!(matches!(
        w.goals.goal(w.ali, task).await,
        Err(AppError::NotFound)
    ));
    let goal = w.goal().await.item.id;
    w.items.set_trashed(w.ali, goal, true).await.unwrap();
    assert!(matches!(
        w.goals
            .link(w.ali, goal, task, LinkGoalRequest::default())
            .await,
        Err(AppError::Conflict(_))
    ));
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn on(today: NaiveDate) -> GoalProgressQuery {
    GoalProgressQuery { today: Some(today) }
}

#[tokio::test]
async fn d3_d4_d6_progress_is_the_weighted_mean_with_its_breakdown() {
    let w = World::new().await;
    let goal = w.goal().await.item.id;
    let task = w.item(w.ali, "task").await;
    let routine = w.item(w.ali, "routine").await;
    let page = w.item(w.ali, "page").await;
    let tracker = w.item(w.ali, "tracker").await;
    w.goals.link(w.ali, goal, task, weight("1")).await.unwrap();
    w.goals
        .link(w.ali, goal, routine, weight("3"))
        .await
        .unwrap();
    w.goals
        .link(w.ali, goal, page, LinkGoalRequest::default())
        .await
        .unwrap();
    w.goals
        .link(w.ali, goal, tracker, LinkGoalRequest::default())
        .await
        .unwrap();
    // The task is half done and on plan; the routine is a quarter done but should be at half.
    w.fixed.set(task, 1.0, 2.0, 1.0);
    w.fixed.set(routine, 5.0, 20.0, 10.0);
    // NOW is 26 November; the goal runs through December, so the caller's day is accepted
    // only near the server's.
    let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let p = w.goals.progress(w.ali, goal, on(today)).await.unwrap();
    assert!(close(p.actual, (0.5 + 3.0 * 0.25) / 4.0));
    assert!(close(p.expected, 0.5));
    assert!(close(p.pace, p.actual / 0.5));
    // D6: each item is explained; the page is left out (D2) and the tracker, which has no
    // provider here, has not started.
    let by_kind: HashMap<&str, &goals::GoalItemProgress> =
        p.items.iter().map(|i| (i.item.kind.as_str(), i)).collect();
    assert_eq!(p.items.len(), 3);
    assert!(!by_kind.contains_key("page"));
    assert!(by_kind["tracker"].not_started);
    assert!(close(by_kind["routine"].share, 0.75));
    assert!(close(by_kind["routine"].contribution, 0.75 * 0.25));
    assert!(close(by_kind["task"].actual, 0.5));
    assert_eq!(by_kind["routine"].weight, Decimal::from(3));
    let sum: f64 = p.items.iter().map(|i| i.contribution).sum();
    assert!(close(sum, p.actual));
    // D5: every provider is asked about the goal's whole range, and about the caller's day.
    let asked = w.fixed.asked.lock().unwrap().clone();
    assert!(asked.iter().all(|(_, range, asked_on)| {
        (range.start(), range.end()) == (day(1), day(31)) && *asked_on == today
    }));
}

#[tokio::test]
async fn v9_hidden_items_do_not_move_the_progress() {
    let w = World::new().await;
    let goal = w.goal().await.item.id;
    let shown = w.item(w.ali, "task").await;
    let hidden = w.item(w.ali, "task").await;
    w.goals.link(w.ali, goal, shown, weight("1")).await.unwrap();
    w.goals
        .link(w.ali, goal, hidden, weight("9"))
        .await
        .unwrap();
    w.fixed.set(shown, 1.0, 1.0, 1.0);
    w.fixed.set(hidden, 0.0, 1.0, 1.0);
    w.grant(RelationTuple::new(goal, Relation::Watcher, w.ayse))
        .await;
    w.grant(RelationTuple::new(shown, Relation::Watcher, w.ayse))
        .await;
    let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let hers = w.goals.progress(w.ayse, goal, on(today)).await.unwrap();
    assert_eq!(hers.items.len(), 1);
    assert!(close(hers.actual, 1.0));
    let his = w.goals.progress(w.ali, goal, on(today)).await.unwrap();
    assert!(close(his.actual, 0.1));
    assert!(matches!(
        w.goals
            .progress(Uuid::now_v7(), goal, GoalProgressQuery::default())
            .await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn progress_takes_the_callers_day_within_a_day_of_the_server() {
    let w = World::new().await;
    let goal = w.goal().await.item.id;
    let p = w
        .goals
        .progress(w.ali, goal, GoalProgressQuery::default())
        .await
        .unwrap();
    assert_eq!(p.today, now().date_naive());
    assert_eq!((p.actual, p.expected, p.pace), (0.0, 0.0, 1.0));
    let far = NaiveDate::from_ymd_opt(2026, 11, 29).unwrap();
    assert!(matches!(
        w.goals.progress(w.ali, goal, on(far)).await,
        Err(AppError::Validation(_))
    ));
}
