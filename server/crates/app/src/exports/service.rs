//! Requesting, building and downloading exports (HS3, T4.3.2).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use kernel::{AppError, SharedClock};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use ts_rs::TS;
use uuid::Uuid;

use crate::exports::build::{Sources, markdown};

/// How long a download link works.
pub const LINK_LIFETIME: Duration = Duration::hours(24);

/// Every day at 05:00 UTC.
pub const CLEANUP_CRON: &str = "0 0 5 * * *";
pub const CLEANUP_SCHEDULE: &str = "exports-cleanup";

/// The two files of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "DataExportFile")]
pub enum ExportFile {
    Json,
    Markdown,
}

impl ExportFile {
    /// From the last path segment of a download link.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "routune.json" => Some(Self::Json),
            "routune.md" => Some(Self::Markdown),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Json => "routune.json",
            Self::Markdown => "routune.md",
        }
    }

    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json; charset=utf-8",
            Self::Markdown => "text/markdown; charset=utf-8",
        }
    }
}

/// Where an export stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "DataExportStatus")]
pub enum ExportStatus {
    Pending,
    Ready,
    Failed,
}

/// `GET`/`POST /account/exports`: the caller's latest export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct DataExport {
    pub id: Uuid,
    pub status: ExportStatus,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
    /// The links stop working then.
    #[ts(type = "string")]
    pub expires_at: DateTime<Utc>,
    /// API paths of the files, set once ready: `/exports/{token}/routune.json` and `.md`.
    pub json_path: Option<String>,
    pub markdown_path: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    status: String,
    token: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

fn status_of(name: &str) -> ExportStatus {
    match name {
        "ready" => ExportStatus::Ready,
        "failed" => ExportStatus::Failed,
        _ => ExportStatus::Pending,
    }
}

fn to_export(row: Row) -> DataExport {
    let status = status_of(&row.status);
    let path = |file: ExportFile| {
        (status == ExportStatus::Ready).then(|| format!("/exports/{}/{}", row.token, file.name()))
    };
    DataExport {
        id: row.id,
        status,
        created_at: row.created_at,
        expires_at: row.expires_at,
        json_path: path(ExportFile::Json),
        markdown_path: path(ExportFile::Markdown),
    }
}

/// A 256-bit random token for the download link.
fn new_token() -> Result<String, AppError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|err| AppError::internal(err.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// The job that builds one export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildExport {
    pub export_id: Uuid,
}

impl jobs::Job for BuildExport {
    const KIND: &'static str = "exports.build";
    const MAX_ATTEMPTS: u32 = 3;
}

/// The daily removal of expired exports.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanupExports {}

impl jobs::Job for CleanupExports {
    const KIND: &'static str = "exports.cleanup";
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct DataExports {
    pool: PgPool,
    clock: SharedClock,
    sources: Sources,
}

const COLUMNS: &str = "id, status, token, created_at, expires_at";

impl DataExports {
    #[must_use]
    pub const fn new(pool: PgPool, clock: SharedClock, sources: Sources) -> Self {
        Self {
            pool,
            clock,
            sources,
        }
    }

    /// The caller's latest export whose link has not expired.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn latest(&self, user: Uuid) -> Result<Option<DataExport>, AppError> {
        let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM data_exports WHERE user_id = $1 AND expires_at > $2
             ORDER BY created_at DESC, id DESC LIMIT 1"
        )))
        .bind(user)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::internal)?;
        Ok(row.map(to_export))
    }

    /// Queues a new export, unless one is still being built (that one is returned).
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn request(&self, user: Uuid) -> Result<DataExport, AppError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(AppError::internal)?;
        // One request at a time per user (a double click, a retry).
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 7303))")
            .bind(user)
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
        let pending: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM data_exports
             WHERE user_id = $1 AND status = 'pending' AND expires_at > $2
             ORDER BY created_at DESC LIMIT 1"
        )))
        .bind(user)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::internal)?;
        if let Some(row) = pending {
            tx.commit().await.map_err(AppError::internal)?;
            return Ok(to_export(row));
        }
        let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO data_exports (id, user_id, status, token, created_at, expires_at)
             VALUES ($1, $2, 'pending', $3, $4, $5) RETURNING {COLUMNS}"
        )))
        .bind(Uuid::now_v7())
        .bind(user)
        .bind(new_token()?)
        .bind(now)
        .bind(now + LINK_LIFETIME)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::internal)?;
        jobs::enqueue(
            &mut tx,
            &BuildExport { export_id: row.id },
            jobs::EnqueueOptions {
                run_at: None,
                dedupe_key: Some(format!("export:{}", row.id)),
            },
        )
        .await
        .map_err(AppError::internal)?;
        tx.commit().await.map_err(AppError::internal)?;
        tracing::info!(target: "exports", "data export requested");
        Ok(to_export(row))
    }

    /// Builds a pending export. A user who is gone, or an export that is not pending any more,
    /// needs nothing. On the last attempt a failure marks it failed.
    ///
    /// # Errors
    /// Retryable errors.
    pub async fn build(&self, export_id: Uuid, last_attempt: bool) -> Result<(), AppError> {
        let user: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM data_exports WHERE id = $1 AND status = 'pending'",
        )
        .bind(export_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::internal)?;
        let Some(user) = user else { return Ok(()) };
        let now = self.clock.now();
        let built = match self.sources.collect(user, now).await {
            Ok(export) => {
                let text = markdown(&export);
                serde_json::to_string_pretty(&export)
                    .map(|json| (json, text))
                    .map_err(AppError::internal)
            }
            Err(err) => Err(err),
        };
        match built {
            Ok((json, text)) => {
                sqlx::query(
                    "UPDATE data_exports SET status = 'ready', json = $2, markdown = $3,
                     ready_at = $4 WHERE id = $1 AND status = 'pending'",
                )
                .bind(export_id)
                .bind(json)
                .bind(text)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(AppError::internal)?;
                tracing::info!(target: "exports", "data export ready");
                Ok(())
            }
            Err(err) if last_attempt || matches!(err, AppError::Unauthenticated) => {
                sqlx::query("UPDATE data_exports SET status = 'failed' WHERE id = $1")
                    .bind(export_id)
                    .execute(&self.pool)
                    .await
                    .map_err(AppError::internal)?;
                tracing::warn!(target: "exports", "data export failed");
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// One file of a ready export by its link's token, while the link works.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an unknown, unready or expired link.
    pub async fn download(&self, token: &str, file: ExportFile) -> Result<String, AppError> {
        let column = match file {
            ExportFile::Json => "json",
            ExportFile::Markdown => "markdown",
        };
        let body: Option<Option<String>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT {column} FROM data_exports
             WHERE token = $1 AND status = 'ready' AND expires_at > $2"
        )))
        .bind(token)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::internal)?;
        body.flatten().ok_or(AppError::NotFound)
    }

    /// Deletes exports whose links expired. How many.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn delete_expired(&self) -> Result<u64, AppError> {
        Ok(
            sqlx::query("DELETE FROM data_exports WHERE expires_at <= $1")
                .bind(self.clock.now())
                .execute(&self.pool)
                .await
                .map_err(AppError::internal)?
                .rows_affected(),
        )
    }
}

/// Registers the build job and the daily cleanup (call from `app::run`).
///
/// # Errors
/// An invalid schedule.
pub fn register(
    exports: DataExports,
    worker: &mut jobs::Worker,
    scheduler: &mut jobs::Scheduler,
) -> Result<(), jobs::ScheduleError> {
    let builder = exports.clone();
    worker.register(move |job: BuildExport, claimed| {
        let exports = builder.clone();
        async move {
            let last = claimed.attempts >= claimed.max_attempts;
            exports.build(job.export_id, last).await?;
            Ok(())
        }
    });
    worker.register(move |_: CleanupExports, _| {
        let exports = exports.clone();
        async move {
            let deleted = exports.delete_expired().await?;
            tracing::info!(target: "exports", deleted, "expired data exports deleted");
            Ok(())
        }
    });
    scheduler.add(CLEANUP_SCHEDULE, CLEANUP_CRON, CleanupExports {})?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn files_round_trip_and_nothing_else_is_served() {
        for file in [ExportFile::Json, ExportFile::Markdown] {
            assert_eq!(ExportFile::from_name(file.name()), Some(file));
        }
        assert_eq!(ExportFile::from_name("../etc/passwd"), None);
        assert_eq!(ExportFile::from_name("routune.txt"), None);
    }

    #[test]
    fn tokens_are_long_and_unique() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
    }

    #[test]
    fn only_a_ready_export_has_links() {
        let row = |status: &str| Row {
            id: Uuid::now_v7(),
            status: status.into(),
            token: "tok".into(),
            created_at: Utc::now(),
            expires_at: Utc::now(),
        };
        let ready = to_export(row("ready"));
        assert_eq!(
            ready.json_path.as_deref(),
            Some("/exports/tok/routune.json")
        );
        assert_eq!(
            ready.markdown_path.as_deref(),
            Some("/exports/tok/routune.md")
        );
        assert_eq!(to_export(row("pending")).json_path, None);
        assert_eq!(to_export(row("failed")).markdown_path, None);
    }
}
