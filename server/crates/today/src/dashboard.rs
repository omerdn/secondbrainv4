//! The team dashboard (PRD TK6, T5.2.7): a team-filtered view of active goals, how routines were
//! kept lately and overdue tasks. Every part comes from the module that owns it, already limited
//! to what the caller can see (V9); this only keeps the team's items. Someone outside the team
//! gets empty lists, the same as for a team that does not exist.

use chrono::NaiveDate;
use goals::ActiveGoal;
use kernel::AppError;
use routines::RoutineAdherence;
use serde::Serialize;
use tasks::DueTask;
use ts_rs::TS;
use uuid::Uuid;

use crate::service::TodayService;

/// Routine adherence is measured over the last this many days, today included.
pub const ADHERENCE_DAYS: u32 = 7;

/// How far each list is read before it is narrowed to the team.
const SCAN_LIMIT: usize = 500;

/// `GET /teams/{id}/dashboard`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct TeamDashboard {
    #[ts(type = "string")]
    pub today: NaiveDate,
    pub goals: Vec<ActiveGoal>,
    pub routines: Vec<RoutineAdherence>,
    /// Days the adherence covers.
    pub adherence_days: u32,
    pub overdue: Vec<DueTask>,
}

impl TodayService {
    /// The dashboard of `team` as `actor` sees it on their day `today` (TK6, V9).
    ///
    /// # Errors
    /// [`AppError::Validation`] for a `today` too far from the server's, or
    /// [`AppError::Internal`].
    pub async fn team_dashboard(
        &self,
        actor: Uuid,
        team: Uuid,
        today: Option<NaiveDate>,
    ) -> Result<TeamDashboard, AppError> {
        let today = goals::check_today(today, self.clock.now().date_naive())?;
        let owned = items::Owner::Team { team_id: team };
        let goals = self
            .goals
            .active(actor, today, SCAN_LIMIT)
            .await?
            .into_iter()
            .filter(|goal| goal.item.owner == owned)
            .collect();
        let yesterday = today.pred_opt().unwrap_or(today);
        let limit = i64::try_from(SCAN_LIMIT).unwrap_or(i64::MAX);
        let overdue = self
            .tasks
            .due_by(actor, None, yesterday, limit)
            .await?
            .into_iter()
            .filter(|task| task.item.owner == owned)
            .collect();
        let routines = self
            .routines
            .team_adherence(actor, team, today, ADHERENCE_DAYS)
            .await?;
        Ok(TeamDashboard {
            today,
            goals,
            routines,
            adherence_days: ADHERENCE_DAYS,
            overdue,
        })
    }
}
