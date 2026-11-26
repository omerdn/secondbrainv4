//! `GET /goals/{id}/progress` (PRD D3-D6, T2.3.2). The items' progress comes from the modules
//! that own them through `ProgressProvider`, registered by kind (ADR-0006); goals never read
//! their tables (ADR-0007).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Days, NaiveDate};
use items::Item;
use kernel::{AppError, FieldErrorCode};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use tracking::{DateRange, Progress, ProgressProvider};
use ts_rs::TS;
use uuid::Uuid;

use crate::calc::{Part, goal_progress};
use crate::model::invalid;
use crate::service::GoalsService;

/// Providers by item kind (`task`, `routine`, `tracker`).
pub type Providers = HashMap<String, Arc<dyn ProgressProvider>>;

/// `GET /goals/{id}/progress` query.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct GoalProgressQuery {
    /// The caller's local day; the server's UTC day when absent. At most one day away from it,
    /// so every time zone fits.
    pub today: Option<NaiveDate>,
}

/// One linked item's part of the goal (D6).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct GoalItemProgress {
    pub item: Item,
    /// Decimal string (D3).
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub weight: Decimal,
    /// The item's own figures over the goal's range (ARCHITECTURE §7.2).
    pub done: f64,
    pub total: f64,
    pub expected_to_date: f64,
    /// `done / total` and `expected_to_date / total`, 0 to 1.
    pub actual: f64,
    pub expected: f64,
    /// Its weight over the weights that count, 0 to 1.
    pub share: f64,
    /// What it adds to the goal's actual: `share × actual`.
    pub contribution: f64,
    /// Nothing to do in the range yet: left out and shown as "not started" (§7.3).
    pub not_started: bool,
}

/// A goal's progress: two indicators (D4) and the breakdown (D6). Only items the caller can see
/// take part (V9).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct GoalProgress {
    #[ts(type = "string")]
    pub today: NaiveDate,
    /// Weighted actual over the whole range, 0 to 1 ("%30").
    pub actual: f64,
    /// Weighted share expected by today, 0 to 1.
    pub expected: f64,
    /// `actual / expected`; 1 is on plan ("Planın %95'indesin").
    pub pace: f64,
    /// Linked items that count, oldest link first; pages are left out (D2).
    pub items: Vec<GoalItemProgress>,
}

/// `today` if it is within a day of `utc_today`, so no time zone is turned away.
///
/// # Errors
/// [`AppError::Validation`] otherwise.
pub fn check_today(today: Option<NaiveDate>, utc_today: NaiveDate) -> Result<NaiveDate, AppError> {
    let Some(today) = today else {
        return Ok(utc_today);
    };
    let first = utc_today.checked_sub_days(Days::new(1));
    let last = utc_today.checked_add_days(Days::new(1));
    match (first, last) {
        (Some(first), Some(last)) if (first..=last).contains(&today) => Ok(today),
        _ => Err(invalid(
            "today",
            FieldErrorCode::OutOfRange,
            "must be within a day of the server's date",
        )),
    }
}

const NOT_STARTED: Progress = Progress {
    done: 0.0,
    total: 0.0,
    expected_to_date: 0.0,
};

impl GoalsService {
    /// The goal's progress as of `today` (VIEW). An item whose module has no record of it
    /// counts as not started.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Validation`] for a bad `today`, or
    /// [`AppError::Internal`].
    pub async fn progress(
        &self,
        actor: Uuid,
        id: Uuid,
        query: GoalProgressQuery,
    ) -> Result<GoalProgress, AppError> {
        let goal = self.goal(actor, id).await?;
        let today = check_today(query.today, self.clock.now().date_naive())?;
        let range = DateRange::new(goal.start_date, goal.end_date).ok_or_else(|| {
            invalid(
                "end_date",
                FieldErrorCode::OutOfRange,
                "the goal is too long to measure",
            )
        })?;
        let mut linked = Vec::new();
        let mut parts = Vec::new();
        for link in goal.links.into_iter().filter(|l| l.contributes) {
            let progress = match self.providers.get(&link.item.kind) {
                Some(provider) => match provider.progress(link.item.id, range, today).await {
                    Ok(progress) => progress,
                    Err(AppError::NotFound) => NOT_STARTED,
                    Err(err) => return Err(err),
                },
                None => NOT_STARTED,
            };
            parts.push(Part {
                weight: link.weight.to_f64().unwrap_or(1.0),
                progress,
            });
            linked.push((link.item, link.weight, progress));
        }
        let result = goal_progress(&parts);
        let items = linked
            .into_iter()
            .zip(result.parts)
            .map(|((item, weight, progress), part)| GoalItemProgress {
                item,
                weight,
                done: progress.done,
                total: progress.total,
                expected_to_date: progress.expected_to_date,
                actual: part.actual,
                expected: part.expected,
                share: part.share,
                contribution: part.contribution,
                not_started: part.not_started,
            })
            .collect();
        Ok(GoalProgress {
            today,
            actual: result.actual,
            expected: result.expected,
            pace: result.pace,
            items,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn today_may_differ_from_utc_by_one_day() {
        assert_eq!(check_today(None, day(26)).unwrap(), day(26));
        for d in [25, 26, 27] {
            assert_eq!(check_today(Some(day(d)), day(26)).unwrap(), day(d));
        }
        for d in [24, 28] {
            assert!(matches!(
                check_today(Some(day(d)), day(26)),
                Err(AppError::Validation(_))
            ));
        }
    }
}
