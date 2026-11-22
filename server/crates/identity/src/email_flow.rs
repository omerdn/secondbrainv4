//! E-mail verification and password reset (T0.3.3, ARCHITECTURE §15).
//!
//! Both flows send a single-use link containing a 256-bit token; only its SHA-256 digest is
//! stored (`email_tokens`). A token is bound to the address it was sent to: if the account's
//! address changes, older tokens stop working. Issuing a new token of a purpose invalidates the
//! user's previous unused ones.
//!
//! - Verification: sent at registration and on request; valid for [`VERIFY_TTL`].
//! - Reset: requested by e-mail address; valid for [`RESET_TTL`]. The answer is always the same
//!   whether or not the address has an account (no account discovery). A successful reset
//!   signs the user out everywhere and also marks the address verified (the user proved access
//!   to the inbox).

use chrono::{DateTime, Duration, Utc};
use kernel::{AppError, EmailKind, EmailMessage, FieldError, FieldErrorCode, ValidationErrors};
use uuid::Uuid;

use crate::events::SessionsEnded;
use crate::model::UserId;
use crate::password;
use crate::service::{AuthContext, IdentityService, blocking};
use crate::token::{self, SessionToken};

/// Lifetime of a verification link.
pub const VERIFY_TTL: Duration = Duration::hours(24);
/// Lifetime of a password reset link.
pub const RESET_TTL: Duration = Duration::hours(1);

/// Marker for e-mail token ids.
pub enum EmailToken {}

/// What a token may be used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    VerifyEmail,
    ResetPassword,
}

impl Purpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify_email",
            Self::ResetPassword => "reset_password",
        }
    }

    fn ttl(self) -> Duration {
        match self {
            Self::VerifyEmail => VERIFY_TTL,
            Self::ResetPassword => RESET_TTL,
        }
    }
}

/// Stores a new token (invalidating older unused ones of the same purpose) and returns the
/// secret to put in the link.
pub(crate) async fn create_token(
    conn: &mut sqlx::PgConnection,
    user_id: UserId,
    purpose: Purpose,
    email: &str,
    now: DateTime<Utc>,
) -> Result<SessionToken, AppError> {
    sqlx::query(
        "UPDATE email_tokens SET used_at = $3
         WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL",
    )
    .bind(user_id.as_uuid())
    .bind(purpose.as_str())
    .bind(now)
    .execute(&mut *conn)
    .await
    .map_err(AppError::internal)?;

    let secret = SessionToken::generate().map_err(AppError::internal)?;
    sqlx::query(
        "INSERT INTO email_tokens (id, user_id, purpose, token_hash, email, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(kernel::Id::<EmailToken>::new_at(now).as_uuid())
    .bind(user_id.as_uuid())
    .bind(purpose.as_str())
    .bind(secret.hash().as_slice())
    .bind(email)
    .bind(now)
    .bind(now + purpose.ttl())
    .execute(&mut *conn)
    .await
    .map_err(AppError::internal)?;
    Ok(secret)
}

fn invalid_token() -> AppError {
    ValidationErrors::from(FieldError::new(
        "token",
        FieldErrorCode::Invalid,
        "the link is invalid or has expired",
    ))
    .into()
}

/// A token row that is unused, unexpired and still matches the account's address.
struct ValidToken {
    id: Uuid,
    user_id: UserId,
}

/// Looks up and locks a usable token inside `tx`.
async fn take_token(
    tx: &mut sqlx::PgConnection,
    presented: &str,
    purpose: Purpose,
    now: DateTime<Utc>,
) -> Result<ValidToken, AppError> {
    let hash = token::hash_presented(presented).ok_or_else(invalid_token)?;
    let row = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT t.id, t.user_id FROM email_tokens t
         JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = $1 AND t.purpose = $2 AND t.used_at IS NULL AND t.expires_at > $3
           AND lower(u.email) = lower(t.email)
         FOR UPDATE OF t",
    )
    .bind(hash.as_slice())
    .bind(purpose.as_str())
    .bind(now)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::internal)?;
    let (id, user_id) = row.ok_or_else(invalid_token)?;
    sqlx::query("UPDATE email_tokens SET used_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    Ok(ValidToken {
        id,
        user_id: UserId::from_uuid(user_id),
    })
}

async fn mark_verified(
    tx: &mut sqlx::PgConnection,
    user_id: UserId,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE users SET email_verified_at = $2 WHERE id = $1 AND email_verified_at IS NULL",
    )
    .bind(user_id.as_uuid())
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::internal)?;
    Ok(())
}

impl IdentityService {
    fn link(&self, path: &str, secret: &SessionToken) -> String {
        format!(
            "{}/{path}?token={}",
            self.inner().email.web_url.trim_end_matches('/'),
            secret.expose()
        )
    }

    async fn deliver(&self, message: EmailMessage) {
        let kind = message.kind.as_str();
        if let Err(err) = self.inner().email.sender.send(&message).await {
            tracing::error!(kind, error = %err, "e-mail delivery failed");
        }
    }

    pub(crate) async fn send_verification(&self, email: &str, secret: &SessionToken) {
        let link = self.link("verify-email", secret);
        self.deliver(EmailMessage {
            kind: EmailKind::VerifyEmail,
            to: email.to_owned(),
            subject: "E-posta adresini doğrula".into(),
            text: format!(
                "Merhaba,\n\nE-posta adresini doğrulamak için bu bağlantıyı aç:\n{link}\n\n\
                 Bağlantı 24 saat geçerlidir. Bu isteği sen yapmadıysan bu e-postayı yok sayabilirsin.\n"
            ),
        })
        .await;
    }

    /// Confirms the address with a token from a verification e-mail.
    ///
    /// Errors: 422 on field `token` if it is malformed, unknown, used, expired or was sent to a
    /// different address than the account's current one.
    pub async fn verify_email(&self, presented: &str) -> Result<(), AppError> {
        let now = self.now();
        let mut tx = self.pool().begin().await.map_err(AppError::internal)?;
        let token = take_token(&mut tx, presented, Purpose::VerifyEmail, now).await?;
        mark_verified(&mut tx, token.user_id, now).await?;
        tx.commit().await.map_err(AppError::internal)?;
        tracing::info!(token_id = %token.id, "e-mail verified");
        Ok(())
    }

    /// Sends a new verification e-mail to the signed-in user, unless already verified.
    pub async fn resend_verification(&self, auth: AuthContext) -> Result<(), AppError> {
        let user = self.current_user(auth.user_id).await?;
        if user.email_verified {
            return Ok(());
        }
        let now = self.now();
        let mut conn = self.pool().acquire().await.map_err(AppError::internal)?;
        let secret =
            create_token(&mut conn, user.id, Purpose::VerifyEmail, &user.email, now).await?;
        drop(conn);
        self.send_verification(&user.email, &secret).await;
        Ok(())
    }

    /// Starts a password reset. Always succeeds from the caller's point of view; an e-mail is
    /// sent only if the address belongs to an account with a password.
    pub async fn forgot_password(&self, email: &str) -> Result<(), AppError> {
        let found = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT u.id, u.email FROM users u
             JOIN auth_identities i ON i.user_id = u.id AND i.provider = 'password'
             WHERE lower(u.email) = lower($1)",
        )
        .bind(email)
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?;
        let Some((user_id, address)) = found else {
            return Ok(());
        };

        let now = self.now();
        let mut conn = self.pool().acquire().await.map_err(AppError::internal)?;
        let secret = create_token(
            &mut conn,
            UserId::from_uuid(user_id),
            Purpose::ResetPassword,
            &address,
            now,
        )
        .await?;
        drop(conn);

        // Delivered in the background so the response time does not depend on the provider
        // (and therefore does not reveal whether the account exists).
        let service = self.clone();
        let link = self.link("reset-password", &secret);
        tokio::spawn(async move {
            service
                .deliver(EmailMessage {
                    kind: EmailKind::ResetPassword,
                    to: address,
                    subject: "Şifre sıfırlama".into(),
                    text: format!(
                        "Merhaba,\n\nYeni bir şifre belirlemek için bu bağlantıyı aç:\n{link}\n\n\
                         Bağlantı 1 saat geçerlidir. Bu isteği sen yapmadıysan bu e-postayı yok \
                         sayabilirsin; şifren değişmez.\n"
                    ),
                })
                .await;
        });
        Ok(())
    }

    /// Sets a new password with a token from a reset e-mail, ends all of the user's sessions and
    /// marks the address verified.
    ///
    /// Errors: 422 on field `token` (see [`IdentityService::verify_email`]).
    pub async fn reset_password(
        &self,
        presented: &str,
        new_password: String,
    ) -> Result<(), AppError> {
        // Reject bad tokens before spending time on hashing.
        if token::hash_presented(presented).is_none() {
            return Err(invalid_token());
        }
        let password_hash = blocking(move || password::hash(&new_password))
            .await?
            .map_err(AppError::internal)?;

        let now = self.now();
        let mut tx = self.pool().begin().await.map_err(AppError::internal)?;
        let token = take_token(&mut tx, presented, Purpose::ResetPassword, now).await?;
        let updated = sqlx::query(
            "UPDATE auth_identities SET password_hash = $2
             WHERE user_id = $1 AND provider = 'password'",
        )
        .bind(token.user_id.as_uuid())
        .bind(&password_hash)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?
        .rows_affected();
        if updated == 0 {
            return Err(invalid_token());
        }
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(token.user_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
        db::outbox::publish(
            &mut tx,
            &SessionsEnded {
                user_id: token.user_id,
                session_id: None,
            },
        )
        .await
        .map_err(AppError::internal)?;
        mark_verified(&mut tx, token.user_id, now).await?;
        tx.commit().await.map_err(AppError::internal)?;
        tracing::info!(token_id = %token.id, "password reset");
        Ok(())
    }
}
