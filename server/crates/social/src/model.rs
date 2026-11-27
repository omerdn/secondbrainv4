//! Friendship DTOs and rules (PRD AK1, AK2).

use chrono::{DateTime, Duration, Utc};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Friend requests one person may send in 24 hours (AK2).
pub const REQUESTS_PER_DAY: i64 = 20;
/// How long a declined request keeps the sender from asking the same address again (AK2).
pub const DECLINE_COOLDOWN: Duration = Duration::days(30);
const EMAIL_MAX_CHARS: usize = 254;

/// The day a nudge counts for (AK3): the sender's own calendar day, so "once a day" follows the
/// sender's midnight wherever the recipient lives.
#[must_use]
pub fn nudge_day(now: DateTime<Utc>, timezone: chrono_tz::Tz) -> chrono::NaiveDate {
    now.with_timezone(&timezone).date_naive()
}

/// A friend request reached someone with an account (AK1): never one to an unknown address
/// or one the recipient's block silenced (AK2). Published in the request's transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendRequestSent {
    pub request_id: Uuid,
    pub from_user: Uuid,
    pub to_user: Uuid,
}

impl kernel::DomainEvent for FriendRequestSent {
    const EVENT_TYPE: &'static str = "social.friend_request_sent";
}

/// Someone nudged a friend (AK3); published in the nudge's transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nudged {
    pub from_user: Uuid,
    pub to_user: Uuid,
}

impl kernel::DomainEvent for Nudged {
    const EVENT_TYPE: &'static str = "social.nudged";
}

/// The address as stored and compared: trimmed and lower-cased.
///
/// # Errors
/// [`AppError::Validation`] for something that is not an e-mail address.
pub fn normalize_email(raw: &str) -> Result<String, AppError> {
    let email = raw.trim().to_lowercase();
    let valid = email.chars().count() <= EMAIL_MAX_CHARS
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
        && !email.chars().any(char::is_whitespace);
    if valid {
        Ok(email)
    } else {
        Err(ValidationErrors::from(FieldError::new(
            "email",
            FieldErrorCode::Invalid,
            "not an e-mail address",
        ))
        .into())
    }
}

/// `POST /friend-requests` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "SendFriendRequestRequest")]
pub struct SendFriendRequest {
    pub email: String,
}

/// A request the caller sent; whether the address has an account is not said (AK2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "OutgoingFriendRequest")]
pub struct OutgoingRequest {
    pub id: Uuid,
    pub to_email: String,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
}

/// A request the caller received.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "IncomingFriendRequest")]
pub struct IncomingRequest {
    pub id: Uuid,
    pub from_user: Uuid,
    pub from_name: String,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
}

/// One of the caller's friends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Friend {
    pub user_id: Uuid,
    pub display_name: String,
    #[ts(type = "string")]
    pub since: DateTime<Utc>,
}

/// Someone the caller blocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BlockedUser {
    pub user_id: Uuid,
    pub display_name: String,
    #[ts(type = "string")]
    pub since: DateTime<Utc>,
}

/// The pair in friendship order (`user_a < user_b`).
#[must_use]
pub(crate) fn pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b { (a, b) } else { (b, a) }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn addresses_are_trimmed_and_lower_cased() {
        assert_eq!(
            normalize_email("  Ada@Example.COM ").unwrap_or_default(),
            "ada@example.com"
        );
        for bad in [
            "",
            "ada",
            "@example.com",
            "ada@localhost",
            "a da@example.com",
        ] {
            assert!(normalize_email(bad).is_err(), "{bad}");
        }
    }

    proptest! {
        #[test]
        fn the_pair_is_ordered_and_symmetric(a: u128, b: u128) {
            let (a, b) = (Uuid::from_u128(a), Uuid::from_u128(b));
            let (x, y) = pair(a, b);
            prop_assert!(x <= y);
            prop_assert_eq!(pair(b, a), (x, y));
        }
    }
}

#[cfg(test)]
mod nudge_tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn ak3_a_nudge_day_is_the_senders_own_day() {
        let istanbul: chrono_tz::Tz = "Europe/Istanbul".parse().unwrap_or(chrono_tz::UTC);
        // 21:30 UTC on the 26th is already the 27th in Istanbul (UTC+3).
        let late = Utc.with_ymd_and_hms(2026, 11, 26, 21, 30, 0).unwrap();
        assert_eq!(
            nudge_day(late, istanbul),
            chrono::NaiveDate::from_ymd_opt(2026, 11, 27).unwrap()
        );
        assert_eq!(
            nudge_day(late, chrono_tz::UTC),
            chrono::NaiveDate::from_ymd_opt(2026, 11, 26).unwrap()
        );
    }
}
