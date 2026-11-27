//! `WorkSessions`: open, command and close personal work sessions (PRD W1–W3, ADR-0015,
//! ARCHITECTURE §11). The timer logic is [`crate::timer`]; this applies it under a row lock and
//! announces the new timer with a `session.timer` event.

use access::{PermissionService, Perms};
use identity::UserId;
use items::ItemsAccessContext;
use kernel::{AppError, FieldError, FieldErrorCode, SharedClock, ValidationErrors};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use chrono::{Duration, NaiveDate, TimeZone};
use chrono_tz::Tz;

use crate::model::{
    InviteRequest, JoinRequest, OpenRequest, PomodoroConfig, SessionInvitation, SessionSummary,
    WorkSession,
};
use crate::repo::{self, SessionRow};
use crate::stats::{self, SessionStats};
use crate::timer::{self, Command, LogChange, Timer, TimerError};

/// Name of the realtime event carrying a session's timer (ARCHITECTURE §11).
pub const TIMER_EVENT: &str = "session.timer";
/// Name of the realtime event telling a friend they were invited to a shared session (W6).
pub const INVITATION_EVENT: &str = "session.invitation";

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn timer_error(err: TimerError) -> AppError {
    match err {
        TimerError::InvalidPlan => ValidationErrors::from(FieldError::new(
            "planned_ms",
            FieldErrorCode::OutOfRange,
            err.to_string(),
        ))
        .into(),
        _ => AppError::Conflict(err.to_string()),
    }
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct WorkSessions {
    pool: PgPool,
    clock: SharedClock,
    permissions: PermissionService<ItemsAccessContext>,
    notifier: realtime::Notifier,
    /// Who may be invited to a shared session (V1: friends who did not block each other).
    sharing: std::sync::Arc<dyn items::SharingPolicy>,
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
            sharing: std::sync::Arc::new(items::NoSharing),
        }
    }

    /// The V1 check for inviting to shared sessions (`social::Friendships` in the app).
    #[must_use]
    pub fn with_sharing(mut self, sharing: std::sync::Arc<dyn items::SharingPolicy>) -> Self {
        self.sharing = sharing;
        self
    }

    async fn view(&self, row: &SessionRow, user: Uuid) -> Result<WorkSession, AppError> {
        let segments = repo::segments(&self.pool, row.id, user)
            .await
            .map_err(internal)?;
        let participants = repo::participants(&self.pool, row.id)
            .await
            .map_err(internal)?;
        let mut timer: crate::model::TimerView = row.timer().map_err(internal)?.into();
        if row.is_shared {
            // W6: the shared timer has no item; the caller sees their own.
            timer.item_id = participants
                .iter()
                .find(|p| p.user == user)
                .and_then(|p| p.item);
        }
        Ok(WorkSession {
            id: row.id,
            open: row.status == "open",
            shared: row.is_shared,
            created_by: row.created_by,
            participants: participants.iter().map(|p| p.user).collect(),
            timer,
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
                repo::open_segment(conn, key, (mode, item_id, 0), now)
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
                repo::open_segment(conn, key, (mode, item_id, 0), now)
                    .await
                    .map_err(internal)?;
            }
        }
        repo::save_timer(conn, row.id, timer, closed.then_some(now))
            .await
            .map_err(internal)?;
        self.announce(conn, row, timer, closed).await
    }

    /// `session.timer` to everyone in the session (ARCHITECTURE §11, W6). The timer itself
    /// travels in the event (the exception in ARCHITECTURE §10); a shared one without an item,
    /// since each participant has their own.
    async fn announce(
        &self,
        conn: &mut PgConnection,
        row: &SessionRow,
        timer: &Timer,
        closed: bool,
    ) -> Result<(), AppError> {
        let now = self.clock.now();
        let participants = repo::participants(&mut *conn, row.id)
            .await
            .map_err(internal)?;
        let mut view = crate::model::TimerView::from(*timer);
        if row.is_shared {
            view.item_id = None;
        }
        let event = realtime::Event {
            name: TIMER_EVENT.into(),
            data: serde_json::json!({
                "session_id": row.id,
                "open": !closed,
                "timer": view,
                "participants": participants.iter().map(|p| p.user).collect::<Vec<_>>(),
                "server_time": now,
            }),
        };
        let mut people: Vec<Uuid> = participants.iter().map(|p| p.user).collect();
        if !people.contains(&row.created_by) && !row.is_shared {
            people.push(row.created_by);
        }
        for person in people {
            self.notifier
                .publish(
                    &mut *conn,
                    &realtime::Channel::User(UserId::from_uuid(person)),
                    &event,
                )
                .await
                .map_err(internal)?;
        }
        Ok(())
    }

    /// Writes segment changes of a shared session.
    async fn apply_ops(
        &self,
        conn: &mut PgConnection,
        session: Uuid,
        ops: &[crate::shared::SegmentOp],
    ) -> Result<(), AppError> {
        let now = self.clock.now();
        for op in ops {
            match *op {
                crate::shared::SegmentOp::Open {
                    user,
                    mode,
                    item,
                    base_ms,
                } => repo::open_segment(conn, (session, user), (mode, item, base_ms), now)
                    .await
                    .map_err(internal)?,
                crate::shared::SegmentOp::Close { user, active_ms } => {
                    repo::close_segment(conn, (session, user), active_ms, now)
                        .await
                        .map_err(internal)?;
                }
            }
        }
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
        if row.is_shared {
            let participants = repo::participants(&mut *tx, id).await.map_err(internal)?;
            let outcome = crate::shared::apply(
                &current,
                command,
                actor,
                &participants,
                self.clock.now(),
                row.pomodoro_config.0.work_ms,
            )
            .map_err(timer_error)?;
            if let Some(item) = outcome.item {
                repo::set_item(&mut tx, (id, actor), item)
                    .await
                    .map_err(internal)?;
            }
            self.apply_ops(&mut tx, id, &outcome.ops).await?;
            if outcome.timer != current {
                repo::save_timer(&mut tx, id, &outcome.timer, None)
                    .await
                    .map_err(internal)?;
            }
            if outcome.timer != current || !outcome.ops.is_empty() || outcome.item.is_some() {
                self.announce(&mut tx, &row, &outcome.timer, false).await?;
            }
            tx.commit().await.map_err(internal)?;
            return self.get(actor, id).await;
        }
        let (next, log) = timer::apply(
            &current,
            command,
            self.clock.now(),
            row.pomodoro_config.0.work_ms,
        )
        .map_err(timer_error)?;
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
        if row.is_shared && row.status == "open" {
            // W6: closing ends it for everyone; each open segment ends with its own time.
            let current = row.timer().map_err(internal)?;
            let participants = repo::participants(&mut *tx, id).await.map_err(internal)?;
            let now = self.clock.now();
            let (next, _) = timer::close(&current, now);
            let ops: Vec<_> = participants
                .iter()
                .flat_map(|p| crate::shared::leave(&current, p, now))
                .collect();
            self.apply_ops(&mut tx, id, &ops).await?;
            repo::save_timer(&mut tx, id, &next, Some(now))
                .await
                .map_err(internal)?;
            self.announce(&mut tx, &row, &next, true).await?;
            tx.commit().await.map_err(internal)?;
            return self.get(actor, id).await;
        }
        if row.status == "open" {
            let (next, log) = timer::close(&row.timer().map_err(internal)?, self.clock.now());
            self.write(&mut tx, (&row, actor), &next, log, true).await?;
        }
        tx.commit().await.map_err(internal)?;
        self.get(actor, id).await
    }
}

impl WorkSessions {
    /// Opens a shared session with the caller in it (W6). Friends join by invitation.
    ///
    /// # Errors
    /// As [`Self::open`].
    pub async fn open_shared(
        &self,
        actor: Uuid,
        request: OpenRequest,
    ) -> Result<WorkSession, AppError> {
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
        let id = kernel::Id::<()>::new_at(now).as_uuid();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        repo::insert_shared(&mut tx, id, actor, pomodoro, now)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.get(actor, id).await
    }

    /// The open shared sessions the caller is in, newest first.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn shared(&self, actor: Uuid) -> Result<Vec<WorkSession>, AppError> {
        let ids = repo::shared_of(&self.pool, actor).await.map_err(internal)?;
        let mut sessions = Vec::with_capacity(ids.len());
        for id in ids {
            sessions.push(self.get(actor, id).await?);
        }
        Ok(sessions)
    }

    /// Invites a friend to a shared session the caller is in (W6). Inviting again changes
    /// nothing. The friend hears of it with a `session.invitation` event.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a session the caller is not in, [`AppError::Conflict`] for a
    /// personal or closed one, [`AppError::Forbidden`] for someone who is not a friend, or
    /// [`AppError::Internal`].
    pub async fn invite(
        &self,
        actor: Uuid,
        id: Uuid,
        request: InviteRequest,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row = self.locked(&mut tx, actor, id).await?;
        if !row.is_shared {
            return Err(AppError::Conflict(
                "a personal session has no guests".into(),
            ));
        }
        let friend = request.user_id;
        if friend == actor || !self.sharing.can_share(actor, friend).await? {
            return Err(AppError::Forbidden);
        }
        let participants = repo::participants(&mut *tx, id).await.map_err(internal)?;
        if participants.iter().any(|p| p.user == friend) {
            return Ok(());
        }
        let now = self.clock.now();
        repo::invite(&mut tx, (id, friend), actor, now)
            .await
            .map_err(internal)?;
        let event = realtime::Event {
            name: INVITATION_EVENT.into(),
            data: serde_json::json!({ "session_id": id, "invited_by": actor }),
        };
        self.notifier
            .publish(
                &mut *tx,
                &realtime::Channel::User(UserId::from_uuid(friend)),
                &event,
            )
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// The caller's invitations to open shared sessions.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn invitations(&self, actor: Uuid) -> Result<Vec<SessionInvitation>, AppError> {
        Ok(repo::invitations(&self.pool, actor)
            .await
            .map_err(internal)?
            .into_iter()
            .map(|(session_id, invited_by, created_at)| SessionInvitation {
                session_id,
                invited_by,
                created_at,
            })
            .collect())
    }

    /// Joins a shared session one was invited to (W6). A running or paused timer starts the
    /// joiner's own segment now. Only friends of the inviter are invited, and the friendship
    /// must still hold.
    ///
    /// # Errors
    /// [`AppError::NotFound`] without an invitation (or for an item one cannot see),
    /// [`AppError::Forbidden`] when the inviter is no longer a friend, [`AppError::Conflict`] for
    /// a closed session, or [`AppError::Internal`].
    pub async fn join(
        &self,
        actor: Uuid,
        id: Uuid,
        request: JoinRequest,
    ) -> Result<WorkSession, AppError> {
        if let Some(item) = request.item_id {
            self.permissions
                .require(&self.pool, actor, item, Perms::VIEW)
                .await?;
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let invited_by: Option<Uuid> = sqlx::query_scalar(
            "SELECT invited_by FROM work_session_invitations WHERE session_id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        let invited_by = invited_by.ok_or(AppError::NotFound)?;
        if !self.sharing.can_share(invited_by, actor).await? {
            return Err(AppError::Forbidden);
        }
        let row = repo::lock_by_id(&mut *tx, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        if row.status != "open" {
            return Err(AppError::Conflict("the session is closed".into()));
        }
        repo::take_invitation(&mut tx, (id, actor))
            .await
            .map_err(internal)?;
        let now = self.clock.now();
        repo::join(&mut tx, (id, actor), request.item_id, now)
            .await
            .map_err(internal)?;
        let current = row.timer().map_err(internal)?;
        let ops = crate::shared::join(&current, actor, request.item_id, now);
        self.apply_ops(&mut tx, id, &ops).await?;
        self.announce(&mut tx, &row, &current, false).await?;
        tx.commit().await.map_err(internal)?;
        self.get(actor, id).await
    }

    /// Leaves a shared session: one's own segment ends; the others go on. The last one out
    /// closes it.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a session the caller is not in, [`AppError::Conflict`] for a
    /// personal one, or [`AppError::Internal`].
    pub async fn leave(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row = self.locked(&mut tx, actor, id).await?;
        if !row.is_shared {
            return Err(AppError::Conflict(
                "close a personal session instead".into(),
            ));
        }
        let current = row.timer().map_err(internal)?;
        let participants = repo::participants(&mut *tx, id).await.map_err(internal)?;
        let now = self.clock.now();
        if let Some(me) = participants.iter().find(|p| p.user == actor) {
            let ops = crate::shared::leave(&current, me, now);
            self.apply_ops(&mut tx, id, &ops).await?;
        }
        repo::leave(&mut tx, (id, actor), now)
            .await
            .map_err(internal)?;
        let last = participants.iter().all(|p| p.user == actor);
        if last {
            let (next, _) = timer::close(&current, now);
            repo::save_timer(&mut tx, id, &next, Some(now))
                .await
                .map_err(internal)?;
        } else {
            self.announce(&mut tx, &row, &current, false).await?;
        }
        tx.commit().await.map_err(internal)
    }
}

/// Longest statistics range, in days.
pub const MAX_STATS_DAYS: i64 = 366;
/// Sessions one history request returns.
const HISTORY_PAGE: i64 = 50;

/// The UTC instant a local day starts at (the earliest one around a DST change).
fn day_start(day: NaiveDate, timezone: Tz) -> chrono::DateTime<chrono::Utc> {
    let midnight = day.and_hms_opt(0, 0, 0).unwrap_or_default();
    timezone
        .from_local_datetime(&midnight)
        .earliest()
        .map_or_else(|| midnight.and_utc(), |t| t.with_timezone(&chrono::Utc))
}

impl WorkSessions {
    /// Work and break time per local day and per item (W5), in `timezone`, the user's.
    /// Defaults to the last 7 days up to today.
    ///
    /// # Errors
    /// [`AppError::Validation`] for a reversed or too long range, or [`AppError::Internal`].
    pub async fn stats(
        &self,
        actor: Uuid,
        (from, to): (Option<NaiveDate>, Option<NaiveDate>),
        timezone: Tz,
    ) -> Result<SessionStats, AppError> {
        let now = self.clock.now();
        let today = now.with_timezone(&timezone).date_naive();
        let to = to.unwrap_or(today);
        let from = from.unwrap_or(to - Duration::days(6));
        if from > to || (to - from).num_days() >= MAX_STATS_DAYS {
            return Err(ValidationErrors::from(FieldError::new(
                "from",
                FieldErrorCode::OutOfRange,
                format!("from must not be after to, at most {MAX_STATS_DAYS} days"),
            ))
            .into());
        }
        let window = (
            day_start(from, timezone),
            day_start(to + Duration::days(1), timezone),
        );
        let counted = repo::counted(&self.pool, actor, window, now)
            .await
            .map_err(internal)?;
        Ok(stats::summarize(&counted, from, to, timezone))
    }

    /// The caller's sessions, newest first (W5).
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn history(
        &self,
        actor: Uuid,
        before: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<SessionSummary>, AppError> {
        let rows = repo::history(&self.pool, actor, before, HISTORY_PAGE)
            .await
            .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|r| SessionSummary {
                id: r.id,
                open: r.status == "open",
                created_at: r.created_at,
                closed_at: r.closed_at,
                work_ms: r.work_ms,
                break_ms: r.break_ms,
                segments: r.segments,
            })
            .collect())
    }
}

/// Default pomodoro lengths, for clients.
#[must_use]
pub fn default_pomodoro() -> PomodoroConfig {
    PomodoroConfig::default()
}
