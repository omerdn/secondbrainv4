//! Active goals with their progress (PRD H3, T2.7.1): goals whose range holds today.

use access::Perms;
use chrono::NaiveDate;
use items::Item;
use kernel::AppError;
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::progress::GoalProgressQuery;
use crate::repo;
use crate::service::GoalsService;

/// A goal running today, with its actual progress and pace (H3).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ActiveGoal {
    pub item: Item,
    #[ts(type = "string")]
    pub start_date: NaiveDate,
    #[ts(type = "string")]
    pub end_date: NaiveDate,
    /// 0 to 1 over the whole range.
    pub actual: f64,
    /// Share expected by today, 0 to 1.
    pub expected: f64,
    /// `actual / expected`; 1 is on plan.
    pub pace: f64,
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl GoalsService {
    /// Active goals `actor` can view whose range holds `today`, ending soonest first, at most
    /// `limit`. `today` is the caller's local day, within a day of the server's (D4).
    ///
    /// # Errors
    /// [`AppError::Validation`] for a `today` too far away, or [`AppError::Internal`].
    pub async fn active(
        &self,
        actor: Uuid,
        today: NaiveDate,
        limit: usize,
    ) -> Result<Vec<ActiveGoal>, AppError> {
        let today = crate::progress::check_today(Some(today), self.clock.now().date_naive())?;
        let visible = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        let rows = repo::running_goals(&self.pool, &visible, today)
            .await
            .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let items = self.items.visible_items(actor, &ids).await?;
        let mut goals = Vec::new();
        for row in rows {
            if goals.len() >= limit {
                break;
            }
            let Some(item) = items.iter().find(|item| {
                item.id == row.item_id && item.archived_at.is_none() && item.deleted_at.is_none()
            }) else {
                continue;
            };
            let progress = self
                .progress(actor, row.item_id, GoalProgressQuery { today: Some(today) })
                .await?;
            goals.push(ActiveGoal {
                item: item.clone(),
                start_date: row.start_date,
                end_date: row.end_date,
                actual: progress.actual,
                expected: progress.expected,
                pace: progress.pace,
            });
        }
        Ok(goals)
    }
}

/// A goal whose range touches a calendar range (PRD C1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct GoalSpan {
    pub item: Item,
    #[ts(type = "string")]
    pub start_date: NaiveDate,
    #[ts(type = "string")]
    pub end_date: NaiveDate,
}

impl GoalsService {
    /// Active goals `actor` can view whose range overlaps `from..=to`, starting first, at most
    /// `limit` (C1).
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn overlapping(
        &self,
        actor: Uuid,
        from: NaiveDate,
        to: NaiveDate,
        limit: usize,
    ) -> Result<Vec<GoalSpan>, AppError> {
        let visible = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        let rows = repo::overlapping_goals(&self.pool, &visible, from, to)
            .await
            .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let items = self.items.visible_items(actor, &ids).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                items
                    .iter()
                    .find(|item| {
                        item.id == row.item_id
                            && item.archived_at.is_none()
                            && item.deleted_at.is_none()
                    })
                    .map(|item| GoalSpan {
                        item: item.clone(),
                        start_date: row.start_date,
                        end_date: row.end_date,
                    })
            })
            .take(limit)
            .collect())
    }
}
