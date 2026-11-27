//! Data export endpoints (HS3, T4.3.2). Mount inside `idempotency::protect`.
//!
//! | Method | Path | Response |
//! |---|---|---|
//! | GET  | `/account/exports` | 200 [`DataExport`] or `null`: the caller's latest unexpired export |
//! | POST | `/account/exports` | 202 [`DataExport`]: queued (or the one still being built) |
//! | GET  | `/exports/{token}/routune.json` | 200 the JSON file; no sign-in, the token is the link (24 h) |
//! | GET  | `/exports/{token}/routune.md` | 200 the Markdown file; 404 once expired or unknown |

use axum::extract::{FromRef, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use identity::IdentityService;
use identity::http::Authenticated;
use kernel::AppError;

use crate::exports::service::{DataExport, DataExports, ExportFile};

#[derive(Clone)]
pub struct ExportsHttpState {
    pub exports: DataExports,
    pub identity: IdentityService,
}

impl FromRef<ExportsHttpState> for IdentityService {
    fn from_ref(state: &ExportsHttpState) -> Self {
        state.identity.clone()
    }
}

pub fn router(state: ExportsHttpState) -> Router {
    Router::new()
        .route("/account/exports", get(latest).post(request))
        .route("/exports/{token}/{file}", get(download))
        .with_state(state)
}

async fn latest(
    State(state): State<ExportsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Option<DataExport>>, AppError> {
    state.exports.latest(auth.user_id.as_uuid()).await.map(Json)
}

async fn request(
    State(state): State<ExportsHttpState>,
    Authenticated(auth): Authenticated,
) -> Result<(StatusCode, Json<DataExport>), AppError> {
    let export = state.exports.request(auth.user_id.as_uuid()).await?;
    Ok((StatusCode::ACCEPTED, Json(export)))
}

async fn download(
    State(state): State<ExportsHttpState>,
    Path((token, file)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let file = ExportFile::from_name(&file).ok_or(AppError::NotFound)?;
    let body = state.exports.download(&token, file).await?;
    Ok((
        [
            (header::CONTENT_TYPE, file.content_type().to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", file.name()),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
            (header::REFERRER_POLICY, "no-referrer".to_owned()),
        ],
        body,
    )
        .into_response())
}
