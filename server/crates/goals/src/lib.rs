//! `goals`: Goals, weights and progress calculation (ADR-0010).
//!
//! See ARCHITECTURE §5.7, §7.3 and PRD D1–D6. A goal is an `items` row plus a detail row here;
//! links between items live in `items` (B1) and are only reached through
//! [`items::ItemsService`] (ADR-0007). This crate keeps the weight of each link (D3).

mod calc;
pub mod http;
mod model;
mod progress;
pub mod repo;
mod service;

pub use calc::{GoalResult, Part, PartResult, goal_progress, ratio};
pub use model::{
    CONTRIBUTING_KINDS, CreateGoalRequest, Goal, GoalLink, LINKABLE_KINDS, LinkGoalRequest,
    UpdateGoalRequest, WEIGHT_MAX, check_dates, check_weight, contributes,
};
pub use progress::{GoalItemProgress, GoalProgress, GoalProgressQuery, Providers, check_today};
pub use service::GoalsService;
