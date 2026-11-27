//! Legal texts and consent (PRD HS5, HS1, ARCHITECTURE §15, T4.3.3).
//!
//! The privacy policy, the KVKK notice and the terms of use are versioned files under
//! `crates/identity/legal/` (`<kind>.v<version>.md`); [`CURRENT`] names the version in force.
//! Raising a version there is the whole release step: everyone accepts again at their next
//! sign-in.
//!
//! - Registering with a password requires accepting every current text in the same request
//!   ([`check_acceptance`]); no account exists otherwise.
//! - A sign-in (password, Google, Apple) of someone who has not accepted the current texts opens
//!   no session. It answers [`AppError::ConsentRequired`] with a ticket instead, and
//!   `POST /auth/consent` with that ticket and the acceptance finishes it. A provider sign-in of
//!   someone without an account keeps the provider's facts in the ticket, so the account is only
//!   created once they accept.
//! - Sessions opened before a new version keep working until they end.

use chrono::{DateTime, Duration, Utc};
use kernel::{AppError, FieldError, FieldErrorCode};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{CurrentUser, UserId};
use crate::oauth::{Provider, ProviderClaims};
use crate::service::{IdentityService, IssuedSession, insert_session};
use crate::token::{self, SessionToken};

/// How long a held sign-in waits for consent.
pub const TICKET_TTL: Duration = Duration::minutes(15);

/// A legal text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "LegalDocumentKind")]
pub enum LegalKind {
    Privacy,
    Kvkk,
    Terms,
}

impl LegalKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Privacy => "privacy",
            Self::Kvkk => "kvkk",
            Self::Terms => "terms",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        CURRENT
            .iter()
            .map(|doc| doc.kind)
            .find(|kind| kind.as_str() == value)
    }
}

/// One text in force.
#[derive(Debug, Clone, Copy)]
pub struct Current {
    pub kind: LegalKind,
    pub version: i32,
    pub title: &'static str,
    pub body: &'static str,
}

/// The texts in force. Raise `version` (and add the new file) to ask everyone again.
pub const CURRENT: [Current; 3] = [
    Current {
        kind: LegalKind::Privacy,
        version: 1,
        title: "Gizlilik Politikası",
        body: include_str!("../legal/privacy.v1.md"),
    },
    Current {
        kind: LegalKind::Kvkk,
        version: 1,
        title: "KVKK Aydınlatma Metni",
        body: include_str!("../legal/kvkk.v1.md"),
    },
    Current {
        kind: LegalKind::Terms,
        version: 1,
        title: "Kullanım Koşulları",
        body: include_str!("../legal/terms.v1.md"),
    },
];

/// `GET /legal`: a text in force, without its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct LegalDocumentSummary {
    pub kind: LegalKind,
    pub version: i32,
    pub title: String,
}

/// `GET /legal/{kind}`: a text in force (Markdown).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct LegalDocument {
    pub kind: LegalKind,
    pub version: i32,
    pub title: String,
    pub body: String,
}

/// A text and version the user accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct LegalAcceptance {
    pub kind: LegalKind,
    pub version: i32,
}

/// `POST /auth/consent`: finishes a held sign-in.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ConsentRequest {
    /// From the `consent_required` error.
    pub ticket: String,
    pub accepted: Vec<LegalAcceptance>,
    pub client: crate::model::ClientKind,
}

/// Every text in force, without bodies.
#[must_use]
pub fn summaries() -> Vec<LegalDocumentSummary> {
    CURRENT
        .iter()
        .map(|doc| LegalDocumentSummary {
            kind: doc.kind,
            version: doc.version,
            title: doc.title.to_owned(),
        })
        .collect()
}

/// One text in force.
#[must_use]
pub fn document(kind: LegalKind) -> Option<LegalDocument> {
    CURRENT
        .iter()
        .find(|doc| doc.kind == kind)
        .map(|doc| LegalDocument {
            kind: doc.kind,
            version: doc.version,
            title: doc.title.to_owned(),
            body: doc.body.to_owned(),
        })
}

/// What accepting everything in force looks like (clients and tests).
#[must_use]
pub fn current_acceptances() -> Vec<LegalAcceptance> {
    CURRENT
        .iter()
        .map(|doc| LegalAcceptance {
            kind: doc.kind,
            version: doc.version,
        })
        .collect()
}

/// Whether `accepted` covers the current version of every text (HS5).
///
/// # Errors
/// A field error on `field` naming what is missing.
pub fn check_acceptance(field: &str, accepted: &[LegalAcceptance]) -> Result<(), FieldError> {
    let missing: Vec<&str> = CURRENT
        .iter()
        .filter(|doc| {
            !accepted
                .iter()
                .any(|a| a.kind == doc.kind && a.version == doc.version)
        })
        .map(|doc| doc.kind.as_str())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(FieldError::new(
            field,
            FieldErrorCode::Required,
            format!(
                "the current legal texts must be accepted: {}",
                missing.join(", ")
            ),
        ))
    }
}

/// What a ticket holds.
enum Held {
    User(UserId),
    Provider(Provider, ProviderClaims),
}

#[derive(sqlx::FromRow)]
struct TicketRow {
    user_id: Option<Uuid>,
    provider: Option<String>,
    claims: Option<serde_json::Value>,
    device: Option<String>,
}

impl IdentityService {
    /// Whether `user` accepted the current version of every text.
    pub(crate) async fn has_current_consent(
        &self,
        conn: &mut PgConnection,
        user: UserId,
    ) -> Result<bool, AppError> {
        let kinds: Vec<&str> = CURRENT.iter().map(|doc| doc.kind.as_str()).collect();
        let versions: Vec<i32> = CURRENT.iter().map(|doc| doc.version).collect();
        let accepted: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM legal_consents c
             JOIN unnest($2::text[], $3::int[]) AS cur(document, version)
               ON c.document = cur.document AND c.version = cur.version
             WHERE c.user_id = $1",
        )
        .bind(user.as_uuid())
        .bind(&kinds)
        .bind(&versions)
        .fetch_one(&mut *conn)
        .await
        .map_err(AppError::internal)?;
        Ok(usize::try_from(accepted).unwrap_or(0) == CURRENT.len())
    }

    /// Records that `user` accepted every text in force at `now`.
    pub(crate) async fn record_consent(
        &self,
        conn: &mut PgConnection,
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        for doc in CURRENT {
            sqlx::query(
                "INSERT INTO legal_consents (user_id, document, version, accepted_at)
                 VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            )
            .bind(user.as_uuid())
            .bind(doc.kind.as_str())
            .bind(doc.version)
            .bind(now)
            .execute(&mut *conn)
            .await
            .map_err(AppError::internal)?;
        }
        Ok(())
    }

    /// Holds a sign-in for consent: stores the ticket and returns the error that carries it.
    async fn hold(&self, held: Held, device: Option<&str>) -> AppError {
        match self.issue_ticket(held, device).await {
            Ok(ticket) => AppError::ConsentRequired { ticket },
            Err(err) => err,
        }
    }

    async fn issue_ticket(&self, held: Held, device: Option<&str>) -> Result<String, AppError> {
        let token = SessionToken::generate().map_err(|err| AppError::internal(err.to_string()))?;
        let now = self.now();
        let (user, provider, claims) = match held {
            Held::User(user) => (Some(user.as_uuid()), None, None),
            Held::Provider(provider, claims) => (
                None,
                Some(provider.as_str()),
                Some(serde_json::to_value(&claims).map_err(AppError::internal)?),
            ),
        };
        sqlx::query(
            "INSERT INTO consent_tickets (token_hash, user_id, provider, claims, device, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(token.hash().as_slice())
        .bind(user)
        .bind(provider)
        .bind(claims)
        .bind(device)
        .bind(now)
        .bind(now + TICKET_TTL)
        .execute(self.pool())
        .await
        .map_err(AppError::internal)?;
        tracing::info!(target: "identity", "sign-in held for legal consent");
        Ok(token.expose().to_owned())
    }

    /// For a sign-in that passed its credential check: the session if `user` accepted the
    /// current texts, else the consent ticket.
    pub(crate) async fn open_session_if_consented(
        &self,
        user: CurrentUser,
        device: Option<&str>,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        let now = self.now();
        let mut tx = self.pool().begin().await.map_err(AppError::internal)?;
        if !self.has_current_consent(&mut tx, user.id).await? {
            drop(tx);
            return Err(self.hold(Held::User(user.id), device).await);
        }
        let session = insert_session(&mut tx, user.id, device, now).await?;
        tx.commit().await.map_err(AppError::internal)?;
        Ok((user, session))
    }

    /// For a provider sign-in of someone without an account, or whose account has not accepted
    /// the current texts: the consent ticket.
    pub(crate) async fn hold_provider_sign_in(
        &self,
        provider: Provider,
        claims: ProviderClaims,
        device: Option<&str>,
    ) -> AppError {
        self.hold(Held::Provider(provider, claims), device).await
    }

    /// Finishes a held sign-in once the user accepted every current text (HS5): records the
    /// consent (creating or linking a provider account first) and opens the session. A ticket
    /// works once.
    ///
    /// # Errors
    /// [`AppError::Validation`] when a text is not accepted; [`AppError::Unauthenticated`] for
    /// an unknown, used or expired ticket; what the provider sign-in may refuse.
    pub async fn complete_consent(
        &self,
        request: &ConsentRequest,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        check_acceptance("accepted", &request.accepted).map_err(kernel::ValidationErrors::from)?;
        let hash = token::hash_presented(&request.ticket).ok_or(AppError::Unauthenticated)?;
        let now = self.now();
        let row: Option<TicketRow> = sqlx::query_as(
            "DELETE FROM consent_tickets WHERE token_hash = $1 AND expires_at > $2
             RETURNING user_id, provider, claims, device",
        )
        .bind(hash.as_slice())
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?;
        let row = row.ok_or(AppError::Unauthenticated)?;
        let held = match (row.user_id, row.provider.as_deref(), row.claims) {
            (Some(user), _, _) => Held::User(UserId::from_uuid(user)),
            (None, Some(name), Some(claims)) => {
                let provider = match name {
                    "google" => Provider::Google,
                    "apple" => Provider::Apple,
                    _ => return Err(AppError::Unauthenticated),
                };
                let claims: ProviderClaims =
                    serde_json::from_value(claims).map_err(AppError::internal)?;
                Held::Provider(provider, claims)
            }
            _ => return Err(AppError::Unauthenticated),
        };
        let mut tx = self.pool().begin().await.map_err(AppError::internal)?;
        let user_id = match held {
            Held::User(user) => user,
            Held::Provider(provider, claims) => {
                self.provider_account(&mut tx, provider, &claims, now)
                    .await?
            }
        };
        self.record_consent(&mut tx, user_id, now).await?;
        let user = self.user_in(&mut tx, user_id).await?;
        let session = insert_session(&mut tx, user.id, row.device.as_deref(), now).await?;
        tx.commit().await.map_err(AppError::internal)?;
        tracing::info!(target: "identity", "legal consent recorded");
        Ok((user, session))
    }

    /// Deletes expired tickets (daily cleanup).
    pub(crate) async fn delete_expired_tickets(&self, now: DateTime<Utc>) -> Result<u64, AppError> {
        Ok(
            sqlx::query("DELETE FROM consent_tickets WHERE expires_at <= $1")
                .bind(now)
                .execute(self.pool())
                .await
                .map_err(AppError::internal)?
                .rows_affected(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs5_every_current_text_must_be_accepted() {
        assert!(check_acceptance("a", &current_acceptances()).is_ok());
        let mut partial = current_acceptances();
        partial.pop();
        let err = check_acceptance("a", &partial).unwrap_err();
        assert!(err.message.contains("terms"));
        // An older version does not count.
        let old: Vec<LegalAcceptance> = current_acceptances()
            .into_iter()
            .map(|a| LegalAcceptance {
                version: a.version - 1,
                ..a
            })
            .collect();
        assert!(check_acceptance("a", &old).is_err());
        assert!(check_acceptance("a", &[]).is_err());
    }

    #[test]
    fn texts_are_served_by_kind() {
        assert_eq!(summaries().len(), 3);
        for doc in CURRENT {
            assert_eq!(LegalKind::parse(doc.kind.as_str()), Some(doc.kind));
            let served = document(doc.kind).unwrap();
            assert!(served.body.starts_with("# "));
            assert!(doc.version > 0);
        }
        assert_eq!(LegalKind::parse("cookies"), None);
    }
}
