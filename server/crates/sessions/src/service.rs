//! `WorkSessions`: open, command and close personal work sessions (PRD W1–W3, ADR-0015,
//! ARCHITECTURE §11). The timer logic is [`crate::timer`]; this applies it under a row lock and
//! announces the new timer with a `session.timer` event.

use access::{PermissionService, Perms};
use identity::UserId;
use items::ItemsAccessContext;
use kernel::{AppError, FieldError, FieldErrorCode, SharedClock, ValidationErrors};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::model::{OpenRequest, PomodoroConfig, WorkSession};
use crate::repo::{self, SessionRow};
use crate::timer::{self, Command, LogChange, Timer, TimerError};

/// Name of the realtime event carrying a session's timer (ARCHITECTURE §11).
pub const TIMER_EVENT: &str = "session.timer";

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct WorkSessions {
    pool: PgPool,
    clock: SharedClock,
    permissions: PermissionService<ItemsAccessContext>,
    notifier: realtime::Notifier,
}

impl std::fmt::Debug for WorkSessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkSessions").finish_non_exhaustive()
    }
}

impl WorkSessions {
    #[must_use]
    pub fn new(
        pool: PgPool,
        clock: SharedClock,
        permissions: PermissionService<ItemsAccessContext>,
        notifier: realtime::Notifier,
    ) -> Self {
        Self {
            pool,
            clock,
            permissions,
            notifier,
        }
    }

    async fn view(&self, row: &SessionRow, user: Uuid) -> Result<WorkSession, AppError> {
        let segments = repo::segments(&self.pool, row.id, user)
            .await
            .map_err(internal)?;
        Ok(WorkSession {
            id: row.id,
            open: row.status == "open",
            timer: row.timer().map_err(internal)?.into(),
            pomodoro: row.pomodoro_config.0,
            segments,
            created_at: row.created_at,
            closed_at: row.closed_at,
            server_time: self.clock.now(),
        })
    }

    /// The caller's open session, opened now if there is none (W1). Opening twice returns the
    /// same session.
    ///
    /// # Errors
    /// [`AppError::Validation`] for pomodoro lengths outside a minute to four hours, or
    /// [`AppError::Internal`].
    pub async fn open(&self, actor: Uuid, request: OpenRequest) -> Result<WorkSession, AppError> {
        let pomodoro = request.pomodoro.unwrap_or_default();
        if !pomodoro.is_valid() {
            return Err(ValidationErrors::from(FieldError::new(
                "pomodoro",
                FieldErrorCode::OutOfRange,
                "every length is between 1 minute and 4 hours",
            ))
            .into());
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        repo::insert(
            &mut tx,
            kernel::Id::<()>::new_at(now).as_uuid(),
            actor,
            pomodoro,
            now,
        )
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        let row = repo::open_of(&self.pool, actor)
            .await
            .map_err(internal)?
            .ok_or_else(|| AppError::Internal("open session vanished".into()))?;
        self.view(&row, actor).await
    }

    /// The caller's open session.
    ///
    /// # Errors
    /// [`AppError::NotFound`] when none is open, or [`AppError::Internal`].
    pub async fn current(&self, actor: Uuid) -> Result<WorkSession, AppError> {
        let row = repo::open_of(&self.pool, actor)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        self.view(&row, actor).await
    }

    /// A session the caller takes part in.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for anyone else's, or [`AppError::Internal`].
    pub async fn get(&self, actor: Uuid, id: Uuid) -> Result<WorkSession, AppError> {
        let row = repo::get(&self.pool, id, actor, false)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        self.view(&row, actor).await
    }

    async fn locked(
        &self,
        conn: &mut PgConnection,
        actor: Uuid,
        id: Uuid,
    ) -> Result<SessionRow, AppError> {
        let row = repo::get(&mut *conn, id, actor, true)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        if row.status != "open" {
            return Err(AppError::Conflict("the session is closed".into()));
        }
        Ok(row)
    }

    async fn write(
        &self,
        conn: &mut PgConnection,
        (row, actor): (&SessionRow, Uuid),
        timer: &Timer,
        log: LogChange,
        closed: bool,
    ) -> Result<(), AppError> {
        let now = self.clock.now();
        let key = (row.id, actor);
        match log {
            LogChange::None => {}
            LogChange::Open { mode, item_id } => {
                repo::open_segment(conn, key, mode, item_id, now)
                    .await
                    .map_err(internal)?;
            }
            LogChange::Close { active_ms } => {
                repo::close_segment(conn, key, active_ms, now)
                    .await
                    .map_err(internal)?;
            }
            LogChange::CloseAndOpen {
                active_ms,
                mode,
                item_id,
            } => {
                repo::close_segment(conn, key, active_ms, now)
                    .await
                    .map_err(internal)?;
                repo::open_segment(conn, key, mode, item_id, now)
                    .await
                    .map_err(internal)?;
            }
        }
        repo::save_timer(conn, row.id, timer, closed.then_some(now))
            .await
            .map_err(internal)?;
        // The timer itself travels in the event (the exception in ARCHITECTURE §10).
        let event = realtime::Event {
            name: TIMER_EVENT.into(),
            data: serde_json::json!({
                "session_id": row.id,
                "open": !closed,
                "timer": crate::model::TimerView::from(*timer),
                "server_time": now,
            }),
        };
        self.notifier
            .publish(
                &mut *conn,
                &realtime::Channel::User(UserId::from_uuid(row.created_by)),
                &event,
            )
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// Applies a timer command (W2, W3); repeating one whose effect holds changes nothing.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a session or item the caller cannot see, [`AppError::Conflict`]
    /// for a closed session or a command that does not fit the timer, [`AppError::Validation`]
    /// for a bad plan, or [`AppError::Internal`].
    pub async fn command(
        &self,
        actor: Uuid,
        id: Uuid,
        command: Command,
    ) -> Result<WorkSession, AppError> {
        let item = match command {
            Command::Start { item_id, .. } | Command::SwitchItem { item_id } => item_id,
            _ => None,
        };
        if let Some(item) = item {
            // V9: an item the caller cannot see is as good as missing.
            self.permissions
                .require(&self.pool, actor, item, Perms::VIEW)
                .await?;
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row = self.locked(&mut tx, actor, id).await?;
        let current = row.timer().map_err(internal)?;
        let (next, log) = timer::apply(
            &current,
            command,
            self.clock.now(),
            row.pomodoro_config.0.work_ms,
        )
        .map_err(|err| match err {
            TimerError::InvalidPlan => ValidationErrors::from(FieldError::new(
                "planned_ms",
                FieldErrorCode::OutOfRange,
                err.to_string(),
            ))
            .into(),
            _ => AppError::Conflict(err.to_string()),
        })?;
        if (next, log) != (current, LogChange::None) {
            self.write(&mut tx, (&row, actor), &next, log, false)
                .await?;
        }
        tx.commit().await.map_err(internal)?;
        self.get(actor, id).await
    }

    /// Closes the session (W1): the timer stops and the open segment ends. Closing again
    /// changes nothing.
    ///
    /// # Errors
    /// [`AppError::NotFound`] or [`AppError::Internal`].
    pub async fn close(&self, actor: Uuid, id: Uuid) -> Result<WorkSession, AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row = repo::get(&mut *tx, id, actor, true)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        if row.status == "open" {
            let (next, log) = timer::close(&row.timer().map_err(internal)?, self.clock.now());
            self.write(&mut tx, (&row, actor), &next, log, true).await?;
        }
        tx.commit().await.map_err(internal)?;
        self.get(actor, id).await
    }
}

/// Default pomodoro lengths, for clients.
#[must_use]
pub fn default_pomodoro() -> PomodoroConfig {
    PomodoroConfig::default()
}
