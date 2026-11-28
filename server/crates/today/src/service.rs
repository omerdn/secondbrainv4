//! `TodayService`: the Today screen from the modules that own its parts (PRD H1–H6, ADR-0007).

use chrono::{Days, Duration, NaiveDate};
use goals::GoalsService;
use items::{ItemsService, Touched};
use kernel::{AppError, SharedClock};
use notebooks::process::ProcessPages;
use routines::RoutinesService;
use sessions::WorkSessions;
use sqlx::PgPool;
use tasks::TasksService;
use uuid::Uuid;

use crate::model::{
    DEFAULT_FORGOTTEN_DAYS, ForgottenDays, HomePreferences, ResumeItem, SessionGlance, Today,
};
use crate::repo;

/// Most entries in one list of the screen.
pub const LIST_LIMIT: usize = 20;
/// Tasks due today and in the next this many days are upcoming (H2).
pub const UPCOMING_DAYS: u64 = 7;
/// What can be forgotten (H4): things one works on, not notebooks or pages.
pub const FORGOTTEN_KINDS: &[&str] = &["task", "routine", "tracker", "goal"];
/// What one picks up again (H5): the things status lines are written for. Pages are left out;
/// a process page is touched with each note and would push its own item down.
pub const RESUME_KINDS: &[&str] = &["task", "routine", "tracker", "goal"];
/// Entries in the "continue where you left off" list.
const RESUME_LIMIT: i64 = 10;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct TodayService {
    pool: PgPool,
    pub(crate) clock: SharedClock,
    items: ItemsService,
    pub(crate) tasks: TasksService,
    pub(crate) routines: RoutinesService,
    pub(crate) goals: GoalsService,
    process: ProcessPages,
    sessions: WorkSessions,
}

impl TodayService {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        pool: PgPool,
        clock: SharedClock,
        items: ItemsService,
        tasks: TasksService,
        routines: RoutinesService,
        goals: GoalsService,
        process: ProcessPages,
        sessions: WorkSessions,
    ) -> Self {
        Self {
            pool,
            clock,
            items,
            tasks,
            routines,
            goals,
            process,
            sessions,
        }
    }

    /// The user's settings, defaults when never set.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn preferences(&self, actor: Uuid) -> Result<HomePreferences, AppError> {
        let days = repo::forgotten_days(&self.pool, actor)
            .await
            .map_err(internal)?
            .unwrap_or(DEFAULT_FORGOTTEN_DAYS);
        Ok(HomePreferences {
            forgotten_days: days,
        })
    }

    /// Sets the forgotten threshold (H4).
    ///
    /// # Errors
    /// [`AppError::Validation`] outside 1 to 365, or [`AppError::Internal`].
    pub async fn set_forgotten_days(
        &self,
        actor: Uuid,
        days: i32,
    ) -> Result<HomePreferences, AppError> {
        let days = ForgottenDays::new(days)?;
        repo::set_forgotten_days(&self.pool, actor, days.get(), self.clock.now())
            .await
            .map_err(internal)?;
        Ok(HomePreferences {
            forgotten_days: days.get(),
        })
    }

    /// The Today screen for `actor` on their local day `today` (the server's UTC day when
    /// none), within a day of the server's.
    ///
    /// # Errors
    /// [`AppError::Validation`] for a `today` too far away, or [`AppError::Internal`].
    pub async fn today(&self, actor: Uuid, today: Option<NaiveDate>) -> Result<Today, AppError> {
        let now = self.clock.now();
        let today = goals::check_today(today, now.date_naive())?;
        let limit = i64::try_from(LIST_LIMIT).unwrap_or(i64::MAX);
        let yesterday = today.pred_opt().unwrap_or(today);
        let horizon = today
            .checked_add_days(Days::new(UPCOMING_DAYS))
            .unwrap_or(today);
        let forgotten_days = self.preferences(actor).await?.forgotten_days;
        let routines = self.routines.today(actor, LIST_LIMIT).await?;
        let overdue = self.tasks.due_by(actor, None, yesterday, limit).await?;
        let upcoming = self
            .tasks
            .due_by(actor, Some(today), horizon, limit)
            .await?;
        let goals = self.goals.active(actor, today, LIST_LIMIT).await?;
        let forgotten = self
            .items
            .by_touch(
                actor,
                FORGOTTEN_KINDS,
                Touched::Before(now - Duration::days(i64::from(forgotten_days))),
                limit,
            )
            .await?;
        let recent = self
            .items
            .by_touch(actor, RESUME_KINDS, Touched::Latest, RESUME_LIMIT)
            .await?;
        let ids: Vec<Uuid> = recent.iter().map(|item| item.id).collect();
        let mut lines = self.process.latest_lines(actor, &ids).await?;
        let resume = recent
            .into_iter()
            .map(|item| {
                let line = lines
                    .iter()
                    .position(|line| line.item_id == item.id)
                    .map(|at| lines.swap_remove(at).line);
                ResumeItem { item, line }
            })
            .collect();
        let session = match self.sessions.current(actor).await {
            Ok(session) => Some(SessionGlance {
                id: session.id,
                timer: session.timer,
                server_time: session.server_time,
            }),
            Err(AppError::NotFound) => None,
            Err(err) => return Err(err),
        };
        Ok(Today {
            today,
            routines,
            overdue,
            upcoming,
            goals,
            forgotten,
            forgotten_days,
            resume,
            session,
        })
    }
}
