//! Sign-in with external identity providers (ADR-0016): Google (T0.3.4), Apple (T0.3.5).
//!
//! Providers only produce verified [`ProviderClaims`]; account lookup, creation and linking is
//! shared here.
//!
//! # Linking rules
//! 1. A known provider account (`auth_identities(provider, subject)`) signs in to its user.
//! 2. Otherwise the provider must vouch for the address (`email_verified`), or the sign-in is
//!    refused: an unverified address could belong to someone else.
//! 3. If an account with that address exists, the provider identity is linked to it. If that
//!    account's address was **not** verified, whoever registered it never proved owning the
//!    inbox, so its password and sessions are removed before linking ("pre-hijack" defence);
//!    the address becomes verified.
//! 4. Otherwise a new account is created with a verified address and no password.

pub mod google;

use chrono::{DateTime, Utc};
use kernel::AppError;
use uuid::Uuid;

use crate::model::{CurrentUser, MAX_DISPLAY_NAME_CHARS, UserId};
use crate::service::{
    IdentityService, IssuedSession, USER_COLUMNS, UserRow, insert_session, to_user,
};

/// An external identity provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Google,
    Apple,
}

impl Provider {
    /// Value of `auth_identities.provider`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Apple => "apple",
        }
    }
}

/// Facts about the user, already verified by the provider integration.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderClaims {
    /// Stable account id at the provider (`sub`).
    pub subject: String,
    /// Address reported by the provider (may be absent, e.g. Apple after the first sign-in).
    pub email: Option<String>,
    /// Whether the provider vouches for the address.
    pub email_verified: bool,
    /// Display name, if the provider sent one.
    pub name: Option<String>,
    /// Avatar URL, if any.
    pub picture: Option<String>,
}

/// Claims contain personal data: never in debug output.
impl std::fmt::Debug for ProviderClaims {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderClaims")
            .field("email_verified", &self.email_verified)
            .finish_non_exhaustive()
    }
}

/// Sign-in through a provider was refused (shown to the user as a generic failure).
fn refused(reason: &'static str) -> AppError {
    tracing::info!(reason, "provider sign-in refused");
    AppError::Unauthenticated
}

/// Display name for a new account: the provider's name, else the address's local part.
fn display_name(claims: &ProviderClaims, email: &str) -> String {
    let name = claims
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| email.split('@').next().unwrap_or(email));
    name.chars().take(MAX_DISPLAY_NAME_CHARS).collect()
}

impl IdentityService {
    /// Signs in (creating or linking the account per the module rules) and opens a session.
    pub async fn sign_in_with_provider(
        &self,
        provider: Provider,
        claims: ProviderClaims,
        device: Option<&str>,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        let now = self.now();
        let mut tx = self.pool().begin().await.map_err(AppError::internal)?;

        let known = sqlx::query_scalar::<_, Uuid>(
            "SELECT user_id FROM auth_identities WHERE provider = $1 AND provider_uid = $2",
        )
        .bind(provider.as_str())
        .bind(&claims.subject)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::internal)?;

        let user_id = match known {
            Some(id) => UserId::from_uuid(id),
            None => self.link_or_create(&mut tx, provider, &claims, now).await?,
        };

        let user = sqlx::query_as::<_, UserRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        )))
        .bind(user_id.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map(to_user)
        .map_err(AppError::internal)?;
        let session = insert_session(&mut tx, user.id, device, now).await?;
        tx.commit().await.map_err(AppError::internal)?;
        Ok((user, session))
    }

    async fn link_or_create(
        &self,
        tx: &mut sqlx::PgConnection,
        provider: Provider,
        claims: &ProviderClaims,
        now: DateTime<Utc>,
    ) -> Result<UserId, AppError> {
        let email = claims
            .email
            .as_deref()
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .ok_or_else(|| refused("no e-mail from provider"))?;
        if !claims.email_verified {
            return Err(refused("provider e-mail not verified"));
        }

        let existing = sqlx::query_as::<_, (Uuid, Option<DateTime<Utc>>)>(
            "SELECT id, email_verified_at FROM users WHERE lower(email) = lower($1) FOR UPDATE",
        )
        .bind(email)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::internal)?;

        let user_id = match existing {
            Some((id, verified_at)) => {
                if verified_at.is_none() {
                    // Pre-hijack defence: the unverified registrant loses password and sessions.
                    for sql in [
                        "DELETE FROM auth_identities WHERE user_id = $1 AND provider = 'password'",
                        "DELETE FROM sessions WHERE user_id = $1",
                    ] {
                        sqlx::query(sql)
                            .bind(id)
                            .execute(&mut *tx)
                            .await
                            .map_err(AppError::internal)?;
                    }
                    sqlx::query("UPDATE users SET email_verified_at = $2 WHERE id = $1")
                        .bind(id)
                        .bind(now)
                        .execute(&mut *tx)
                        .await
                        .map_err(AppError::internal)?;
                }
                UserId::from_uuid(id)
            }
            None => {
                let id = UserId::new_at(now);
                sqlx::query(
                    "INSERT INTO users (id, email, display_name, avatar_url, created_at, email_verified_at)
                     VALUES ($1, $2, $3, $4, $5, $5)",
                )
                .bind(id.as_uuid())
                .bind(email)
                .bind(display_name(claims, email))
                .bind(claims.picture.as_deref())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(AppError::internal)?;
                id
            }
        };

        let linked = sqlx::query(
            "INSERT INTO auth_identities (user_id, provider, provider_uid, created_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(user_id.as_uuid())
        .bind(provider.as_str())
        .bind(&claims.subject)
        .bind(now)
        .execute(&mut *tx)
        .await;
        match linked {
            Ok(_) => Ok(user_id),
            Err(err)
                if err.as_database_error().is_some_and(|e| {
                    e.constraint() == Some("auth_identities_user_id_provider_key")
                }) =>
            {
                Err(AppError::Conflict(
                    "this account is already linked to another account of this provider".into(),
                ))
            }
            Err(err) => Err(AppError::internal(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(name: Option<&str>) -> ProviderClaims {
        ProviderClaims {
            subject: "s".into(),
            email: Some("ada.l@example.test".into()),
            email_verified: true,
            name: name.map(Into::into),
            picture: None,
        }
    }

    #[test]
    fn display_name_falls_back_to_local_part() {
        assert_eq!(
            display_name(&claims(Some(" Ada ")), "ada.l@example.test"),
            "Ada"
        );
        assert_eq!(
            display_name(&claims(Some("  ")), "ada.l@example.test"),
            "ada.l"
        );
        assert_eq!(display_name(&claims(None), "ada.l@example.test"), "ada.l");
        let long = "x".repeat(300);
        assert_eq!(
            display_name(&claims(Some(&long)), "a@b.c").chars().count(),
            MAX_DISPLAY_NAME_CHARS
        );
    }

    #[test]
    fn debug_hides_personal_data() {
        let debug = format!("{:?}", claims(Some("Ada")));
        assert!(!debug.contains("ada.l") && !debug.contains("Ada"));
    }
}
