//! Search endpoint (T2.7.5).
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | GET | `/search?q=` | 200 [`SearchHit`]`[]`, best first (AR1, AR2) |

use axum::extract::{FromRef, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;

use crate::service::{SearchHit, SearchQuery, SearchService};

#[derive(Clone)]
pub struct SearchHttpState {
    pub search: SearchService,
    pub identity: IdentityService,
}

impl FromRef<SearchHttpState> for IdentityService {
    fn from_ref(state: &SearchHttpState) -> Self {
        state.identity.clone()
    }
}

pub fn router(state: SearchHttpState) -> Router {
    Router::new()
        .route("/search", get(search))
        .with_state(state)
}

async fn search(
    State(state): State<SearchHttpState>,
    Authenticated(auth): Authenticated,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHit>>, AppError> {
    state
        .search
        .search(auth.user_id.as_uuid(), &query.q)
        .await
        .map(Json)
}
