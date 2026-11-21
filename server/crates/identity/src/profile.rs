//! Profile of the signed-in user (T0.3.7, ARCHITECTURE §5.1): display name, avatar URL,
//! time zone and language.
//!
//! Validation is pure ([`UpdateProfileRequest::validate`]); [`IdentityService::update_profile`]
//! applies the result. The avatar is only a link to an image hosted elsewhere: the product has
//! no file uploads (ADR-0013, PRD §17).

use std::str::FromStr;

use kernel::validation::{max_chars, non_blank};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Deserializer};
use ts_rs::TS;

use crate::model::{CurrentUser, MAX_DISPLAY_NAME_CHARS, UserId};
use crate::service::{IdentityService, USER_COLUMNS, UserRow, to_user};

/// Longest avatar URL.
pub const MAX_AVATAR_URL_CHARS: usize = 2048;
/// Longest language tag (RFC 5646 recommends supporting at least 35 characters).
pub const MAX_LOCALE_CHARS: usize = 35;

/// `PATCH /me`. Every field is optional; only the fields present are changed.
#[derive(Debug, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateProfileRequest {
    /// Name shown to other people (1–100 characters after trimming).
    #[serde(default)]
    #[ts(optional)]
    pub display_name: Option<String>,
    /// `https` URL of an image hosted elsewhere. `null` or `""` removes the avatar.
    #[serde(default, deserialize_with = "present")]
    #[ts(optional, type = "string | null")]
    pub avatar_url: Option<Option<String>>,
    /// IANA time zone, e.g. `Europe/Istanbul`.
    #[serde(default)]
    #[ts(optional)]
    pub timezone: Option<String>,
    /// BCP 47 language tag, e.g. `tr` or `en-US`.
    #[serde(default)]
    #[ts(optional)]
    pub locale: Option<String>,
}

/// Distinguishes an absent field (`None`, via `#[serde(default)]`) from an explicit `null`
/// (`Some(None)`).
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Validated, normalized profile changes. `None` leaves a field unchanged.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProfileChanges {
    pub display_name: Option<String>,
    /// `Some(None)` removes the avatar.
    pub avatar_url: Option<Option<String>>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
}

impl UpdateProfileRequest {
    /// Trims and checks every present field; reports all field errors together.
    pub fn validate(self) -> Result<ProfileChanges, ValidationErrors> {
        let mut errors = ValidationErrors::new();
        let mut changes = ProfileChanges::default();

        if let Some(name) = self.display_name {
            let name = name.trim().to_owned();
            let checked = non_blank("display_name", &name)
                .and_then(|()| max_chars("display_name", &name, MAX_DISPLAY_NAME_CHARS));
            errors.check(checked);
            changes.display_name = Some(name);
        }
        if let Some(avatar) = self.avatar_url {
            let avatar = avatar
                .map(|url| url.trim().to_owned())
                .filter(|url| !url.is_empty());
            if let Some(url) = &avatar {
                errors.check(check_avatar_url("avatar_url", url));
            }
            changes.avatar_url = Some(avatar);
        }
        if let Some(timezone) = self.timezone {
            match canonical_timezone("timezone", timezone.trim()) {
                Ok(tz) => changes.timezone = Some(tz),
                Err(err) => {
                    errors.check(Err(err));
                }
            }
        }
        if let Some(locale) = self.locale {
            match canonical_locale("locale", locale.trim()) {
                Ok(tag) => changes.locale = Some(tag),
                Err(err) => {
                    errors.check(Err(err));
                }
            }
        }

        if errors.is_empty() {
            Ok(changes)
        } else {
            Err(errors)
        }
    }
}

/// An absolute `https` URL with a host and no whitespace. Plain `http` is rejected so an
/// avatar never downgrades the page to mixed content.
pub fn check_avatar_url(field: &str, url: &str) -> Result<(), FieldError> {
    max_chars(field, url, MAX_AVATAR_URL_CHARS)?;
    let invalid = || FieldError::new(field, FieldErrorCode::Invalid, "must be an https URL");
    let rest = url
        .get(..8)
        .filter(|scheme| scheme.eq_ignore_ascii_case("https://"))
        .map(|_| &url[8..])
        .ok_or_else(invalid)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Credentials in the URL (`user:pass@host`) are never legitimate for an image link.
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    let bad_char = url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '\\'));
    if host.is_empty() || authority.contains('@') || bad_char {
        return Err(invalid());
    }
    Ok(())
}

/// The name if it is a known IANA time zone (exact spelling, as devices report it).
pub fn canonical_timezone(field: &str, name: &str) -> Result<String, FieldError> {
    chrono_tz::Tz::from_str(name)
        .map(|tz| tz.name().to_owned())
        .map_err(|_| {
            FieldError::new(
                field,
                FieldErrorCode::Invalid,
                "must be an IANA time zone, e.g. Europe/Istanbul",
            )
        })
}

/// Checks the shape of a BCP 47 tag (`language[-script][-region][-variant…]`, no private-use or
/// grandfathered forms) and returns it in canonical case: `tr`, `en-US`, `zh-Hant-TW`.
pub fn canonical_locale(field: &str, tag: &str) -> Result<String, FieldError> {
    let invalid = || {
        FieldError::new(
            field,
            FieldErrorCode::Invalid,
            "must be a BCP 47 language tag, e.g. tr or en-US",
        )
    };
    non_blank(field, tag)?;
    max_chars(field, tag, MAX_LOCALE_CHARS)?;
    let mut subtags = tag.split(['-', '_']);
    let language = subtags.next().unwrap_or_default();
    if !(2..=3).contains(&language.len()) || !language.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(invalid());
    }
    let mut out = language.to_ascii_lowercase();
    for (index, subtag) in subtags.enumerate() {
        if !(1..=8).contains(&subtag.len()) || !subtag.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(invalid());
        }
        let all_alpha = subtag.chars().all(|c| c.is_ascii_alphabetic());
        out.push('-');
        if index == 0 && subtag.len() == 4 && all_alpha {
            // Script: title case.
            let (first, rest) = subtag.split_at(1);
            out.push_str(&first.to_ascii_uppercase());
            out.push_str(&rest.to_ascii_lowercase());
        } else if subtag.len() == 2 && all_alpha {
            // Region: upper case.
            out.push_str(&subtag.to_ascii_uppercase());
        } else {
            out.push_str(&subtag.to_ascii_lowercase());
        }
    }
    Ok(out)
}

impl IdentityService {
    /// Applies validated profile changes to the caller's own account and returns the result.
    /// An empty change set only reads the account.
    ///
    /// Errors: [`AppError::Unauthenticated`] if the account no longer exists.
    pub async fn update_profile(
        &self,
        user_id: UserId,
        changes: ProfileChanges,
    ) -> Result<CurrentUser, AppError> {
        let (avatar_set, avatar_url) = match changes.avatar_url {
            Some(url) => (true, url),
            None => (false, None),
        };
        sqlx::query_as::<_, UserRow>(sqlx::AssertSqlSafe(format!(
            "UPDATE users SET
                 display_name = COALESCE($2, display_name),
                 avatar_url   = CASE WHEN $3 THEN $4 ELSE avatar_url END,
                 timezone     = COALESCE($5, timezone),
                 locale       = COALESCE($6, locale)
             WHERE id = $1
             RETURNING {USER_COLUMNS}"
        )))
        .bind(user_id.as_uuid())
        .bind(changes.display_name)
        .bind(avatar_set)
        .bind(avatar_url)
        .bind(changes.timezone)
        .bind(changes.locale)
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?
        .map(to_user)
        .ok_or(AppError::Unauthenticated)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn codes(errors: &ValidationErrors) -> Vec<(&str, FieldErrorCode)> {
        errors
            .errors()
            .iter()
            .map(|e| (e.field.as_str(), e.code))
            .collect()
    }

    fn parse(json: &str) -> UpdateProfileRequest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn absent_null_and_value_are_distinguished_for_avatar() {
        assert_eq!(parse("{}").avatar_url, None);
        assert_eq!(parse(r#"{"avatar_url":null}"#).avatar_url, Some(None));
        assert_eq!(
            parse(r#"{"avatar_url":"https://x.test/a.png"}"#).avatar_url,
            Some(Some("https://x.test/a.png".into()))
        );
    }

    #[test]
    fn empty_request_changes_nothing() {
        assert_eq!(parse("{}").validate().unwrap(), ProfileChanges::default());
    }

    #[test]
    fn present_fields_are_normalized() {
        let changes = parse(
            r#"{"display_name":"  Ada  ","avatar_url":"  ","timezone":" Europe/Istanbul ","locale":"en_us"}"#,
        )
        .validate()
        .unwrap();
        assert_eq!(
            changes,
            ProfileChanges {
                display_name: Some("Ada".into()),
                avatar_url: Some(None),
                timezone: Some("Europe/Istanbul".into()),
                locale: Some("en-US".into()),
            }
        );
    }

    #[test]
    fn all_field_errors_are_reported_together() {
        let err = parse(
            r#"{"display_name":"  ","avatar_url":"http://x.test/a.png","timezone":"Mars/Base","locale":"english"}"#,
        )
        .validate()
        .unwrap_err();
        assert_eq!(
            codes(&err),
            [
                ("display_name", FieldErrorCode::Required),
                ("avatar_url", FieldErrorCode::Invalid),
                ("timezone", FieldErrorCode::Invalid),
                ("locale", FieldErrorCode::Invalid),
            ]
        );
    }

    #[test]
    fn display_name_length_is_counted_in_characters() {
        let ok = "ğ".repeat(MAX_DISPLAY_NAME_CHARS);
        assert!(
            UpdateProfileRequest {
                display_name: Some(ok.clone()),
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
        let err = UpdateProfileRequest {
            display_name: Some(format!("{ok}ğ")),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(codes(&err), [("display_name", FieldErrorCode::TooLong)]);
    }

    #[test]
    fn avatar_url_must_be_https_with_a_host() {
        for good in [
            "https://cdn.example.com/a.png",
            "HTTPS://x.test",
            "https://x.test:8443/p?q=1#f",
        ] {
            assert!(check_avatar_url("a", good).is_ok(), "{good:?}");
        }
        for bad in [
            "http://x.test/a.png",
            "javascript:alert(1)",
            "data:image/png;base64,AAAA",
            "https://",
            "https:///path",
            "https://user:pw@x.test/a.png",
            "https://x.test/a b.png",
            "https://x.test/\"onerror",
            "//x.test/a.png",
            "ftp://x.test/a.png",
        ] {
            assert!(check_avatar_url("a", bad).is_err(), "{bad:?}");
        }
        let long = format!("https://x.test/{}", "a".repeat(MAX_AVATAR_URL_CHARS));
        assert_eq!(
            check_avatar_url("a", &long).unwrap_err().code,
            FieldErrorCode::TooLong
        );
    }

    #[test]
    fn only_known_iana_timezones_are_accepted() {
        assert_eq!(canonical_timezone("t", "UTC").unwrap(), "UTC");
        assert_eq!(
            canonical_timezone("t", "America/New_York").unwrap(),
            "America/New_York"
        );
        for bad in [
            "",
            "america/new_york",
            "GMT+3",
            "Europe/Atlantis",
            "Istanbul",
        ] {
            assert!(canonical_timezone("t", bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn locales_are_checked_and_canonicalized() {
        for (input, expected) in [
            ("tr", "tr"),
            ("TR", "tr"),
            ("en-us", "en-US"),
            ("zh_hant_tw", "zh-Hant-TW"),
            ("sr-Latn", "sr-Latn"),
            ("es-419", "es-419"),
        ] {
            assert_eq!(canonical_locale("l", input).unwrap(), expected, "{input:?}");
        }
        for bad in [
            "",
            "e",
            "english",
            "en-",
            "en--US",
            "tr-123456789",
            "1a",
            "t r",
        ] {
            assert!(canonical_locale("l", bad).is_err(), "{bad:?}");
        }
    }

    proptest! {
        #[test]
        fn canonical_locale_is_idempotent(tag in "[a-zA-Z]{2,3}(-[a-zA-Z0-9]{1,8}){0,3}") {
            let once = canonical_locale("l", &tag).unwrap();
            prop_assert_eq!(canonical_locale("l", &once).unwrap(), once.clone());
            prop_assert!(once.eq_ignore_ascii_case(&tag.replace('_', "-")));
        }

        #[test]
        fn avatar_check_never_panics(url in "\\PC{0,64}") {
            let _ = check_avatar_url("a", &url);
        }
    }
}
