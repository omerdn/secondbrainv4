//! Account and session operations (T0.3.2). Only this module touches the identity tables.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use db::PgPool;
use kernel::{AppError, SharedClock, SharedEmailSender};
use uuid::Uuid;

use crate::events::SessionsEnded;
use crate::model::{Credentials, CurrentUser, NewAccount, SessionId, SessionInfo, UserId};
use crate::password;
use crate::token::{self, SessionToken};

/// A session expires after this long without use; each use extends it (sliding expiry).
pub const SESSION_TTL: Duration = Duration::days(30);
/// `last_seen_at` / `expires_at` are rewritten at most this often per session, so
/// authenticated reads do not turn into a write on every request.
pub const TOUCH_INTERVAL: Duration = Duration::minutes(5);

/// The caller behind a valid session token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthContext {
    /// Signed-in user.
    pub user_id: UserId,
    /// Session the request came with.
    pub session_id: SessionId,
}

/// A session created by register or login.
#[derive(Debug)]
pub struct IssuedSession {
    /// Secret for the client; shown once.
    pub token: SessionToken,
    /// Expiry if unused.
    pub expires_at: DateTime<Utc>,
}

/// Validity of a stored session at `now` (pure; see tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Expired: the token no longer authenticates.
    Expired,
    /// Valid. When `touch` is true, `last_seen_at` and `expires_at` should be moved forward.
    Valid { touch: bool },
}

/// Decides whether a session is still valid and whether its activity should be recorded.
pub fn session_state(
    now: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> SessionState {
    if now >= expires_at {
        SessionState::Expired
    } else {
        SessionState::Valid {
            touch: now - last_seen_at >= TOUCH_INTERVAL,
        }
    }
}

/// Identity operations. Cheap to clone; share one per process.
#[derive(Clone)]
pub struct IdentityService {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
    pub(crate) email: EmailSettings,
    pub(crate) google: Option<crate::oauth::google::Google>,
    pub(crate) apple: Option<crate::oauth::apple::Apple>,
}

/// How identity e-mails are delivered.
#[derive(Clone)]
pub struct EmailSettings {
    /// Delivery driver.
    pub sender: SharedEmailSender,
    /// Public URL of the web app, used in e-mail links (e.g. `https://app.example.com`).
    pub web_url: String,
}

impl std::fmt::Debug for IdentityService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityService").finish_non_exhaustive()
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    avatar_url: Option<String>,
    timezone: String,
    locale: String,
    created_at: DateTime<Utc>,
    email_verified_at: Option<DateTime<Utc>>,
}

pub(crate) fn to_user(row: UserRow) -> CurrentUser {
    CurrentUser {
        id: UserId::from_uuid(row.id),
        email: row.email,
        display_name: row.display_name,
        avatar_url: row.avatar_url,
        timezone: row.timezone,
        locale: row.locale,
        created_at: row.created_at,
        email_verified: row.email_verified_at.is_some(),
    }
}

#[derive(sqlx::FromRow)]
struct LoginRow {
    #[sqlx(flatten)]
    user: UserRow,
    password_hash: Option<String>,
}

pub(crate) const USER_COLUMNS: &str =
    "id, email, display_name, avatar_url, timezone, locale, created_at, email_verified_at";

pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(AppError::internal)
}

impl IdentityService {
    /// Creates the service.
    pub fn new(pool: PgPool, clock: SharedClock, email: EmailSettings) -> Self {
        Self {
            inner: Arc::new(Inner {
                pool,
                clock,
                email,
                google: None,
                apple: None,
            }),
        }
    }

    /// Enables Google sign-in (T0.3.4). Without it the Google endpoints answer 404.
    pub fn with_google(self, google: crate::oauth::google::Google) -> Self {
        let inner = &self.inner;
        Self {
            inner: Arc::new(Inner {
                pool: inner.pool.clone(),
                clock: inner.clock.clone(),
                email: inner.email.clone(),
                google: Some(google),
                apple: inner.apple.clone(),
            }),
        }
    }

    /// Enables Sign in with Apple (T0.3.5). Without it the Apple endpoints answer 404.
    pub fn with_apple(self, apple: crate::oauth::apple::Apple) -> Self {
        let inner = &self.inner;
        Self {
            inner: Arc::new(Inner {
                pool: inner.pool.clone(),
                clock: inner.clock.clone(),
                email: inner.email.clone(),
                google: inner.google.clone(),
                apple: Some(apple),
            }),
        }
    }

    pub(crate) fn inner(&self) -> &Inner {
        &self.inner
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.inner.pool
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.inner.clock.now()
    }

    /// Creates an account with a password identity, signs it in and sends a verification
    /// e-mail (a delivery failure is logged; the user can ask for another one).
    ///
    /// Errors: [`AppError::Conflict`] if the e-mail is already registered (any letter case).
    pub async fn register(
        &self,
        account: NewAccount,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        let NewAccount {
            email,
            password,
            display_name,
            device,
        } = account;
        let password_hash = blocking(move || password::hash(&password))
            .await?
            .map_err(AppError::internal)?;

        let now = self.now();
        let user_id = UserId::new_at(now);
        let mut tx = self.pool().begin().await.map_err(AppError::internal)?;
        let inserted = sqlx::query_as::<_, UserRow>(sqlx::AssertSqlSafe(format!(
            "INSERT INTO users (id, email, display_name, created_at) VALUES ($1, $2, $3, $4)
             RETURNING {USER_COLUMNS}"
        )))
        .bind(user_id.as_uuid())
        .bind(&email)
        .bind(&display_name)
        .bind(now)
        .fetch_one(&mut *tx)
        .await;
        let user = match inserted {
            Ok(row) => to_user(row),
            Err(err) if is_unique_violation(&err, "users_email_lower_key") => {
                return Err(AppError::Conflict(
                    "an account with this e-mail already exists".into(),
                ));
            }
            Err(err) => return Err(AppError::internal(err)),
        };

        sqlx::query(
            "INSERT INTO auth_identities (user_id, provider, provider_uid, password_hash, created_at)
             VALUES ($1, 'password', $1::text, $2, $3)",
        )
        .bind(user_id.as_uuid())
        .bind(&password_hash)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;

        // The request was validated with every current legal text accepted (HS5, T4.3.3).
        self.record_consent(&mut tx, user.id, now).await?;
        let session = insert_session(&mut tx, user.id, device.as_deref(), now).await?;
        let verification = crate::email_flow::create_token(
            &mut tx,
            user.id,
            crate::email_flow::Purpose::VerifyEmail,
            &user.email,
            now,
        )
        .await?;
        tx.commit().await.map_err(AppError::internal)?;
        self.send_verification(&user.email, &verification).await;
        Ok((user, session))
    }

    /// Signs in with e-mail and password.
    ///
    /// Errors: [`AppError::Unauthenticated`] for an unknown e-mail, an account without a
    /// password, or a wrong password — indistinguishable to the caller, including in timing.
    pub async fn login(
        &self,
        credentials: Credentials,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        let Credentials {
            email,
            password,
            device,
        } = credentials;

        let found = sqlx::query_as::<_, LoginRow>(
            "SELECT u.id, u.email, u.display_name, u.avatar_url, u.timezone, u.locale, u.created_at,
                    u.email_verified_at, i.password_hash
             FROM users u
             LEFT JOIN auth_identities i ON i.user_id = u.id AND i.provider = 'password'
             WHERE lower(u.email) = lower($1)",
        )
        .bind(&email)
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?;

        let (user, password_hash) = match found {
            Some(row) => (Some(to_user(row.user)), row.password_hash),
            None => (None, None),
        };
        let matches = blocking(move || match password_hash {
            Some(hash) => password::verify(&password, &hash),
            None => {
                password::verify_dummy(&password);
                false
            }
        })
        .await?;
        let user = match (user, matches) {
            (Some(user), true) => user,
            _ => return Err(AppError::Unauthenticated),
        };

        // HS5 (T4.3.3): no session before the current legal texts are accepted.
        self.open_session_if_consented(user, device.as_deref())
            .await
    }

    /// Resolves a presented token. Records activity (sliding expiry) at most every
    /// [`TOUCH_INTERVAL`].
    ///
    /// Errors: [`AppError::Unauthenticated`] if the token is malformed, unknown or expired.
    pub async fn authenticate(&self, presented: &str) -> Result<AuthContext, AppError> {
        let hash = token::hash_presented(presented).ok_or(AppError::Unauthenticated)?;
        let row = sqlx::query_as::<_, (Uuid, Uuid, DateTime<Utc>, DateTime<Utc>)>(
            "SELECT id, user_id, last_seen_at, expires_at FROM sessions WHERE token_hash = $1",
        )
        .bind(hash.as_slice())
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?;
        let (session_id, user_id, last_seen_at, expires_at) =
            row.ok_or(AppError::Unauthenticated)?;

        let now = self.now();
        match session_state(now, last_seen_at, expires_at) {
            SessionState::Expired => return Err(AppError::Unauthenticated),
            SessionState::Valid { touch: true } => {
                sqlx::query(
                    "UPDATE sessions SET last_seen_at = $2, expires_at = $3
                     WHERE id = $1 AND last_seen_at < $2",
                )
                .bind(session_id)
                .bind(now)
                .bind(now + SESSION_TTL)
                .execute(self.pool())
                .await
                .map_err(AppError::internal)?;
            }
            SessionState::Valid { touch: false } => {}
        }
        Ok(AuthContext {
            user_id: UserId::from_uuid(user_id),
            session_id: SessionId::from_uuid(session_id),
        })
    }

    /// The caller's own account.
    pub async fn current_user(&self, user_id: UserId) -> Result<CurrentUser, AppError> {
        sqlx::query_as::<_, UserRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        )))
        .bind(user_id.as_uuid())
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?
        .map(to_user)
        .ok_or(AppError::Unauthenticated)
    }

    /// The user with this e-mail address (case-insensitive), if any. For flows that address a
    /// person by e-mail (friend requests, AK1); callers must not reveal whether one exists.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn user_id_by_email(&self, email: &str) -> Result<Option<uuid::Uuid>, AppError> {
        sqlx::query_scalar("SELECT id FROM users WHERE lower(email) = lower($1)")
            .bind(email.trim())
            .fetch_optional(self.pool())
            .await
            .map_err(AppError::internal)
    }

    /// Display names of `ids`, for screens that show who is affected (PRD V7). Unknown ids are
    /// absent from the result. Callers decide who may see them.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn display_names(
        &self,
        ids: &[uuid::Uuid],
    ) -> Result<std::collections::HashMap<uuid::Uuid, String>, AppError> {
        let rows: Vec<(uuid::Uuid, String)> =
            sqlx::query_as("SELECT id, display_name FROM users WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(self.pool())
                .await
                .map_err(AppError::internal)?;
        Ok(rows.into_iter().collect())
    }

    /// The user's active sessions, newest first.
    pub async fn list_sessions(&self, auth: AuthContext) -> Result<Vec<SessionInfo>, AppError> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Option<String>,
                DateTime<Utc>,
                DateTime<Utc>,
                DateTime<Utc>,
            ),
        >(
            "SELECT id, device, created_at, last_seen_at, expires_at FROM sessions
             WHERE user_id = $1 AND expires_at > $2
             ORDER BY created_at DESC, id DESC",
        )
        .bind(auth.user_id.as_uuid())
        .bind(self.now())
        .fetch_all(self.pool())
        .await
        .map_err(AppError::internal)?;
        Ok(rows
            .into_iter()
            .map(|(id, device, created_at, last_seen_at, expires_at)| {
                let id = SessionId::from_uuid(id);
                SessionInfo {
                    id,
                    device,
                    created_at,
                    last_seen_at,
                    expires_at,
                    current: id == auth.session_id,
                }
            })
            .collect())
    }

    /// Ends one of the user's own sessions (the current one is allowed: that is a logout).
    ///
    /// Errors: [`AppError::NotFound`] if the session does not exist or belongs to someone
    /// else (PRD V9: never reveal other users' objects).
    pub async fn revoke_session(
        &self,
        user_id: UserId,
        session_id: SessionId,
    ) -> Result<(), AppError> {
        let mut tx = self.pool().begin().await.map_err(AppError::internal)?;
        let deleted = sqlx::query("DELETE FROM sessions WHERE id = $1 AND user_id = $2")
            .bind(session_id.as_uuid())
            .bind(user_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?
            .rows_affected();
        if deleted == 0 {
            return Err(AppError::NotFound);
        }
        db::outbox::publish(
            &mut tx,
            &SessionsEnded {
                user_id,
                session_id: Some(session_id),
            },
        )
        .await
        .map_err(AppError::internal)?;
        tx.commit().await.map_err(AppError::internal)?;
        Ok(())
    }

    /// Whether a session still exists and has not expired. Read-only: unlike
    /// [`Self::authenticate`] it does not extend the session. Used by long-lived connections
    /// (`/ws`) to notice expiry.
    pub async fn session_active(&self, session_id: SessionId) -> Result<bool, AppError> {
        let expires_at =
            sqlx::query_scalar::<_, DateTime<Utc>>("SELECT expires_at FROM sessions WHERE id = $1")
                .bind(session_id.as_uuid())
                .fetch_optional(self.pool())
                .await
                .map_err(AppError::internal)?;
        Ok(expires_at.is_some_and(|expires_at| self.now() < expires_at))
    }
}

pub(crate) async fn insert_session(
    tx: &mut sqlx::PgConnection,
    user_id: UserId,
    device: Option<&str>,
    now: DateTime<Utc>,
) -> Result<IssuedSession, AppError> {
    let token = SessionToken::generate().map_err(AppError::internal)?;
    let expires_at = now + SESSION_TTL;
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, device, created_at, last_seen_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $5, $6)",
    )
    .bind(SessionId::new_at(now).as_uuid())
    .bind(user_id.as_uuid())
    .bind(token.hash().as_slice())
    .bind(device)
    .bind(now)
    .bind(expires_at)
    .execute(tx)
    .await
    .map_err(AppError::internal)?;
    Ok(IssuedSession { token, expires_at })
}

fn is_unique_violation(err: &sqlx::Error, constraint: &str) -> bool {
    err.as_database_error()
        .is_some_and(|e| e.is_unique_violation() && e.constraint() == Some(constraint))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;

    use super::*;

    fn t(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn expiry_boundary_is_exclusive() {
        assert_eq!(session_state(t(10), t(0), t(10)), SessionState::Expired);
        assert_eq!(
            session_state(t(9), t(0), t(10)),
            SessionState::Valid { touch: false }
        );
    }

    #[test]
    fn activity_is_recorded_after_the_touch_interval() {
        let interval = TOUCH_INTERVAL.num_seconds();
        let far = t(10 * interval);
        assert_eq!(
            session_state(t(interval - 1), t(0), far),
            SessionState::Valid { touch: false }
        );
        assert_eq!(
            session_state(t(interval), t(0), far),
            SessionState::Valid { touch: true }
        );
    }

    proptest! {
        #[test]
        fn expired_iff_now_reached_expiry(now in 0i64..100_000, seen in 0i64..100_000, exp in 0i64..100_000) {
            let state = session_state(t(now), t(seen), t(exp));
            prop_assert_eq!(state == SessionState::Expired, now >= exp);
            if let SessionState::Valid { touch } = state {
                prop_assert_eq!(touch, now - seen >= TOUCH_INTERVAL.num_seconds());
            }
        }
    }
}
