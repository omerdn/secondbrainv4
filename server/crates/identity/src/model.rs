//! Identity ids, DTOs and input validation.

use chrono::{DateTime, Utc};
use kernel::validation::{max_chars, min_chars, non_blank};
use kernel::{FieldError, FieldErrorCode, Id, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::password::{MAX_PASSWORD_CHARS, MIN_PASSWORD_CHARS};

/// Marker for user ids.
pub enum User {}
/// Id of a user.
pub type UserId = Id<User>;

/// Marker for session ids.
pub enum Session {}
/// Id of a sign-in session.
pub type SessionId = Id<Session>;

/// Longest e-mail address (RFC 5321 path limit).
pub const MAX_EMAIL_CHARS: usize = 254;
/// Longest display name.
pub const MAX_DISPLAY_NAME_CHARS: usize = 100;
/// Longest device label.
pub const MAX_DEVICE_CHARS: usize = 100;

/// How the client wants to receive the session token (ARCHITECTURE §15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ClientKind {
    /// Browser: the token is set as an `HttpOnly` cookie and never exposed to JavaScript.
    Web,
    /// Mobile or desktop app: the token is returned in the body and sent back as
    /// `Authorization: Bearer <token>`.
    Native,
}

/// `POST /auth/register`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct RegisterRequest {
    /// Sign-in address. Unique regardless of letter case.
    pub email: String,
    /// At least 8, at most 128 characters.
    pub password: String,
    /// Name shown to other people.
    pub display_name: String,
    /// See [`ClientKind`].
    pub client: ClientKind,
    /// Optional label for the session list, e.g. "iPhone".
    #[serde(default)]
    #[ts(optional)]
    pub device: Option<String>,
}

/// `POST /auth/login`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct LoginRequest {
    /// Sign-in address (any letter case).
    pub email: String,
    /// The password.
    pub password: String,
    /// See [`ClientKind`].
    pub client: ClientKind,
    /// Optional label for the session list.
    #[serde(default)]
    #[ts(optional)]
    pub device: Option<String>,
}

/// `POST /auth/email/verify`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct VerifyEmailRequest {
    /// Token from the verification link.
    pub token: String,
}

/// `POST /auth/password/forgot`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ForgotPasswordRequest {
    /// Address of the account.
    pub email: String,
}

/// `POST /auth/password/reset`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ResetPasswordRequest {
    /// Token from the reset link.
    pub token: String,
    /// New password (same rules as at registration).
    pub password: String,
}

impl ForgotPasswordRequest {
    /// Trims and length-checks the address. The shape is not checked: an unknown address is
    /// answered exactly like a known one.
    pub fn validate(self) -> Result<String, ValidationErrors> {
        let email = self.email.trim().to_owned();
        let mut errors = ValidationErrors::new();
        errors
            .check(non_blank("email", &email))
            .check(max_chars("email", &email, MAX_EMAIL_CHARS));
        if errors.is_empty() {
            Ok(email)
        } else {
            Err(errors)
        }
    }
}

impl ResetPasswordRequest {
    /// Checks the new password.
    pub fn validate(self) -> Result<(String, String), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        errors.check(check_password("password", &self.password));
        if errors.is_empty() {
            Ok((self.token, self.password))
        } else {
            Err(errors)
        }
    }
}

/// `POST /auth/google/native`: the app ran the authorization code flow with PKCE itself.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct GoogleNativeRequest {
    /// Authorization code returned to the app.
    pub code: String,
    /// PKCE verifier the app generated for this flow.
    pub code_verifier: String,
    /// Redirect URI used in the authorization request.
    pub redirect_uri: String,
    /// The platform client id (iOS or Android) used in the authorization request.
    pub client_id: String,
    /// Optional label for the session list.
    #[serde(default)]
    #[ts(optional)]
    pub device: Option<String>,
}

impl GoogleNativeRequest {
    /// Checks presence and length of each field.
    pub fn validate(self) -> Result<crate::oauth::google::GoogleNativeSignIn, ValidationErrors> {
        let device = clean_device(self.device);
        let mut errors = ValidationErrors::new();
        for (field, value, max) in [
            ("code", &self.code, 2048),
            ("code_verifier", &self.code_verifier, 128),
            ("redirect_uri", &self.redirect_uri, 2048),
            ("client_id", &self.client_id, 255),
        ] {
            errors
                .check(non_blank(field, value))
                .check(max_chars(field, value, max));
        }
        // RFC 7636: the verifier is 43–128 characters.
        errors.check(min_chars("code_verifier", &self.code_verifier, 43));
        if let Some(device) = &device {
            errors.check(max_chars("device", device, MAX_DEVICE_CHARS));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(crate::oauth::google::GoogleNativeSignIn {
            code: self.code,
            code_verifier: self.code_verifier,
            redirect_uri: self.redirect_uri,
            client_id: self.client_id,
            device,
        })
    }
}

/// `POST /auth/apple/native`: the iOS app's `ASAuthorizationAppleIDCredential`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct AppleNativeRequest {
    /// `authorizationCode` from the credential.
    pub code: String,
    /// The app's bundle id.
    pub client_id: String,
    /// `fullName.givenName` (Apple sends it only on the first authorization).
    #[serde(default)]
    #[ts(optional)]
    pub given_name: Option<String>,
    /// `fullName.familyName` (first authorization only).
    #[serde(default)]
    #[ts(optional)]
    pub family_name: Option<String>,
    /// Optional label for the session list.
    #[serde(default)]
    #[ts(optional)]
    pub device: Option<String>,
}

impl AppleNativeRequest {
    /// Checks presence and length of each field.
    pub fn validate(self) -> Result<crate::oauth::apple::AppleNativeSignIn, ValidationErrors> {
        let device = clean_device(self.device);
        let mut errors = ValidationErrors::new();
        errors
            .check(non_blank("code", &self.code))
            .check(max_chars("code", &self.code, 2048))
            .check(non_blank("client_id", &self.client_id))
            .check(max_chars("client_id", &self.client_id, 255));
        for (field, value) in [
            ("given_name", &self.given_name),
            ("family_name", &self.family_name),
        ] {
            if let Some(value) = value {
                errors.check(max_chars(field, value, MAX_DISPLAY_NAME_CHARS));
            }
        }
        if let Some(device) = &device {
            errors.check(max_chars("device", device, MAX_DEVICE_CHARS));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(crate::oauth::apple::AppleNativeSignIn {
            name: crate::oauth::apple::full_name(
                self.given_name.as_deref(),
                self.family_name.as_deref(),
            ),
            code: self.code,
            client_id: self.client_id,
            device,
        })
    }
}

/// The signed-in user's own account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CurrentUser {
    /// User id.
    pub id: UserId,
    /// Sign-in address.
    pub email: String,
    /// Name shown to other people.
    pub display_name: String,
    /// Avatar image URL, if set.
    pub avatar_url: Option<String>,
    /// IANA time zone, e.g. `Europe/Istanbul`.
    pub timezone: String,
    /// BCP 47 language tag, e.g. `tr`.
    pub locale: String,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
    /// Whether the current address has been verified (T0.3.3).
    pub email_verified: bool,
}

/// Body of a successful register or login.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct AuthResponse {
    /// The signed-in user.
    pub user: CurrentUser,
    /// The session token, only for [`ClientKind::Native`]. Web clients get a cookie instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub token: Option<String>,
    /// CSRF token for cookie-authenticated mutations, only for [`ClientKind::Web`] (send it as
    /// `X-CSRF-Token`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub csrf_token: Option<String>,
    /// When the session expires if it is not used. Each use extends it.
    pub expires_at: DateTime<Utc>,
}

/// One entry of `GET /auth/sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SessionInfo {
    /// Session id (for `DELETE /auth/sessions/{id}`).
    pub id: SessionId,
    /// Label given by the client at sign-in.
    pub device: Option<String>,
    /// Sign-in time.
    pub created_at: DateTime<Utc>,
    /// Last time the session was used (updated at most every few minutes).
    pub last_seen_at: DateTime<Utc>,
    /// Expiry if not used again.
    pub expires_at: DateTime<Utc>,
    /// Whether this is the session making the request.
    pub current: bool,
}

/// Validated registration input.
#[derive(Debug)]
pub struct NewAccount {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub device: Option<String>,
}

/// Checks the shape of an e-mail address. Deliverability is proven later by verification
/// (T0.3.3); this only rejects obvious mistakes.
pub fn check_email(field: &str, email: &str) -> Result<(), FieldError> {
    non_blank(field, email)?;
    max_chars(field, email, MAX_EMAIL_CHARS)?;
    let invalid = || FieldError::new(field, FieldErrorCode::Invalid, "must be an e-mail address");
    let (local, domain) = email.split_once('@').ok_or_else(invalid)?;
    let domain_ok = domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@');
    let no_spaces = !email.chars().any(char::is_whitespace);
    if local.is_empty() || !domain_ok || !no_spaces {
        return Err(invalid());
    }
    Ok(())
}

fn check_password(field: &str, password: &str) -> Result<(), FieldError> {
    min_chars(field, password, MIN_PASSWORD_CHARS)?;
    max_chars(field, password, MAX_PASSWORD_CHARS)
}

/// Trims an optional label; blank becomes `None`.
fn clean_device(device: Option<String>) -> Option<String> {
    device
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty())
}

impl RegisterRequest {
    /// Validates and normalizes the request (trims e-mail, name and device).
    pub fn validate(self) -> Result<NewAccount, ValidationErrors> {
        let email = self.email.trim().to_owned();
        let display_name = self.display_name.trim().to_owned();
        let device = clean_device(self.device);

        let mut errors = ValidationErrors::new();
        errors
            .check(check_email("email", &email))
            .check(check_password("password", &self.password))
            .check(non_blank("display_name", &display_name))
            .check(max_chars(
                "display_name",
                &display_name,
                MAX_DISPLAY_NAME_CHARS,
            ));
        if let Some(device) = &device {
            errors.check(max_chars("device", device, MAX_DEVICE_CHARS));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(NewAccount {
            email,
            password: self.password,
            display_name,
            device,
        })
    }
}

/// Validated login input.
#[derive(Debug)]
pub struct Credentials {
    pub email: String,
    pub password: String,
    pub device: Option<String>,
}

impl LoginRequest {
    /// Only checks lengths: a wrong-looking address simply fails to sign in.
    pub fn validate(self) -> Result<Credentials, ValidationErrors> {
        let email = self.email.trim().to_owned();
        let device = clean_device(self.device);
        let mut errors = ValidationErrors::new();
        errors
            .check(non_blank("email", &email))
            .check(max_chars("email", &email, MAX_EMAIL_CHARS))
            .check(non_blank("password", &self.password))
            .check(max_chars("password", &self.password, MAX_PASSWORD_CHARS));
        if let Some(device) = &device {
            errors.check(max_chars("device", device, MAX_DEVICE_CHARS));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Credentials {
            email,
            password: self.password,
            device,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(email: &str, password: &str, name: &str) -> RegisterRequest {
        RegisterRequest {
            email: email.into(),
            password: password.into(),
            display_name: name.into(),
            client: ClientKind::Web,
            device: Some("  ".into()),
        }
    }

    fn fields(errors: &ValidationErrors) -> Vec<(&str, FieldErrorCode)> {
        errors
            .errors()
            .iter()
            .map(|e| (e.field.as_str(), e.code))
            .collect()
    }

    #[test]
    fn valid_registration_is_normalized() {
        let account = register("  Ada@Example.test ", "12345678", " Ada ")
            .validate()
            .unwrap();
        assert_eq!(account.email, "Ada@Example.test");
        assert_eq!(account.display_name, "Ada");
        assert_eq!(account.device, None);
    }

    #[test]
    fn email_shape_is_checked() {
        for bad in [
            "",
            "ada",
            "@x.test",
            "ada@",
            "ada@x",
            "ada@.test",
            "ada@x.",
            "a da@x.test",
            "a@b@x.test",
        ] {
            assert!(check_email("email", bad).is_err(), "{bad:?}");
        }
        for good in [
            "a@x.test",
            "ada.lovelace+tag@mail.example.com",
            "çağrı@örnek.com.tr",
        ] {
            assert!(check_email("email", good).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn password_length_is_counted_in_characters() {
        let err = register("a@x.test", "ğğğğğğğ", "A").validate().unwrap_err();
        assert_eq!(fields(&err), [("password", FieldErrorCode::TooShort)]);
        assert!(register("a@x.test", "ğğğğğğğğ", "A").validate().is_ok());
        let long = "x".repeat(MAX_PASSWORD_CHARS + 1);
        let err = register("a@x.test", &long, "A").validate().unwrap_err();
        assert_eq!(fields(&err), [("password", FieldErrorCode::TooLong)]);
    }

    #[test]
    fn all_field_errors_are_reported_together() {
        let err = register("nope", "short", "  ").validate().unwrap_err();
        assert_eq!(
            fields(&err),
            [
                ("email", FieldErrorCode::Invalid),
                ("password", FieldErrorCode::TooShort),
                ("display_name", FieldErrorCode::Required),
            ]
        );
    }

    #[test]
    fn login_only_checks_presence_and_length() {
        let ok = LoginRequest {
            email: " whatever ".into(),
            password: "x".into(),
            client: ClientKind::Native,
            device: Some(" iPhone ".into()),
        }
        .validate()
        .unwrap();
        assert_eq!(ok.email, "whatever");
        assert_eq!(ok.device.as_deref(), Some("iPhone"));
    }
}
