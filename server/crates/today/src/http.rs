//! Today endpoints (T2.7.1). Mount inside `idempotency::protect`.
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | GET | `/today?today=` | 200 [`Today`] (H1–H6) |
//! | GET | `/calendar?from=&to=` | 200 [`Calendar`] (C1); at most 62 days |
//! | GET | `/today/preferences` | 200 [`HomePreferences`] |
//! | PUT | `/today/preferences` | 200 [`HomePreferences`]; body [`UpdateHomePreferences`] (H4) |

use axum::extract::{FromRef, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;

use crate::calendar::{Calendar, CalendarQuery};
use crate::model::{HomePreferences, Today, TodayQuery, UpdateHomePreferences};
use crate::service::TodayService;

#[derive(Clone)]
pub struct HomeHttpState {
    pub today: TodayService,
    pub identity: IdentityService,
}

impl FromRef<HomeHttpState> for IdentityService {
    fn from_ref(state: &HomeHttpState) -> Self {
        state.identity.clone()
    }
}

pub fn router(state: HomeHttpState) -> Router {
    Router::new()
        .route("/today", get(read_today))
        .route("/calendar", get(read_calendar))
        .route(
            "/today/preferences",
            get(read_preferences).put(update_preferences),
        )
        .with_state(state)
}

async fn read_today(
    State(state): State<HomeHttpState>,
    Authenticated(auth): Authenticated,
    Query(query): Query<TodayQuery>,
) -> Result<Json<Today>, AppError> {
    state
        .today
        .today(auth.user_id.as_uuid(), query.today)
        .await
        .map(Json)
}

async fn read_preferences(
    State(state): State<HomeHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<HomePreferences>, AppError> {
    state
        .today
        .preferences(auth.user_id.as_uuid())
        .await
        .map(Json)
}

async fn update_preferences(
    State(state): State<HomeHttpState>,
    Authenticated(auth): Authenticated,
    Json(request): Json<UpdateHomePreferences>,
) -> Result<Json<HomePreferences>, AppError> {
    state
        .today
        .set_forgotten_days(auth.user_id.as_uuid(), request.forgotten_days)
        .await
        .map(Json)
}

async fn read_calendar(
    State(state): State<HomeHttpState>,
    Authenticated(auth): Authenticated,
    Query(query): Query<CalendarQuery>,
) -> Result<Json<Calendar>, AppError> {
    state
        .today
        .calendar(auth.user_id.as_uuid(), query.from, query.to)
        .await
        .map(Json)
}
