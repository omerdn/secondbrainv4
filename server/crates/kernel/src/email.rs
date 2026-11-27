//! Outgoing e-mail (ARCHITECTURE §14, T0.3.3).
//!
//! Modules send through the [`EmailSender`] trait and never know the provider. The transactional
//! provider is Resend ([`ResendEmailSender`], ADR-0022); [`LogEmailSender`] is for development.
//!
//! # Personal data
//! A message's recipient and body are personal data. Drivers must not log them, with one
//! deliberate exception: [`LogEmailSender`] is a **local development** driver that prints the
//! body (so a developer can click the verification link). It never prints the recipient, and the
//! application refuses to start with it when `APP_ENV=production`.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// What an e-mail is for. Logged instead of the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailKind {
    /// Confirms ownership of an address.
    VerifyEmail,
    /// Lets a user choose a new password.
    ResetPassword,
    /// Someone invited the user to an item (G5, T3.7.1).
    Invitation,
    /// Someone asked the user to be friends (AK1, T3.7.1).
    FriendRequest,
}

impl EmailKind {
    /// Stable name for logs and metrics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify_email",
            Self::ResetPassword => "reset_password",
            Self::Invitation => "invitation",
            Self::FriendRequest => "friend_request",
        }
    }
}

/// A plain-text e-mail.
#[derive(Clone, PartialEq, Eq)]
pub struct EmailMessage {
    /// Purpose (for logs; never the content).
    pub kind: EmailKind,
    /// Recipient address.
    pub to: String,
    /// Subject line.
    pub subject: String,
    /// Plain-text body.
    pub text: String,
}

/// Recipient and body are personal data: never in debug output.
impl fmt::Debug for EmailMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmailMessage")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// Sending failed. The message must not contain the recipient or body.
#[derive(Debug, thiserror::Error)]
#[error("e-mail delivery failed: {0}")]
pub struct EmailError(pub String);

/// Boxed future returned by [`EmailSender::send`] (keeps the trait object safe).
pub type SendFuture<'a> = Pin<Box<dyn Future<Output = Result<(), EmailError>> + Send + 'a>>;

/// An e-mail delivery driver.
pub trait EmailSender: Send + Sync + fmt::Debug {
    /// Sends one message.
    fn send<'a>(&'a self, message: &'a EmailMessage) -> SendFuture<'a>;
}

/// Shared driver for application state.
pub type SharedEmailSender = Arc<dyn EmailSender>;

/// Local development driver: writes the subject and body to the log (`info`, target
/// `email`) instead of sending. The recipient is not logged.
#[derive(Debug, Default, Clone, Copy)]
pub struct LogEmailSender;

impl EmailSender for LogEmailSender {
    fn send<'a>(&'a self, message: &'a EmailMessage) -> SendFuture<'a> {
        Box::pin(async move {
            tracing::info!(
                target: "email",
                kind = message.kind.as_str(),
                subject = %message.subject,
                body = %message.text,
                "e-mail (log driver, not sent)"
            );
            Ok(())
        })
    }
}

/// Resend (<https://resend.com>) over its HTTP API (ADR-0022, T0.7.7).
///
/// The API key, recipient and body never reach logs or errors; a failure carries only the HTTP
/// status or the transport error kind.
#[derive(Clone)]
pub struct ResendEmailSender {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    from: String,
}

impl ResendEmailSender {
    /// Production API base URL.
    pub const API_URL: &'static str = "https://api.resend.com";

    /// A sender with the given API key and `From` header (e.g. `Routune <noreply@routune.dev>`).
    pub fn new(api_key: impl Into<String>, from: impl Into<String>) -> Self {
        Self::with_endpoint(Self::API_URL, api_key, from)
    }

    /// Same as [`Self::new`] against another base URL (tests).
    pub fn with_endpoint(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        from: impl Into<String>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            http,
            endpoint: endpoint.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            from: from.into(),
        }
    }
}

/// The API key is a secret: never in debug output.
impl fmt::Debug for ResendEmailSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResendEmailSender")
            .field("endpoint", &self.endpoint)
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

#[derive(serde::Serialize)]
struct ResendEmail<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    text: &'a str,
}

impl EmailSender for ResendEmailSender {
    fn send<'a>(&'a self, message: &'a EmailMessage) -> SendFuture<'a> {
        Box::pin(async move {
            let response = self
                .http
                .post(format!("{}/emails", self.endpoint))
                .bearer_auth(&self.api_key)
                .json(&ResendEmail {
                    from: &self.from,
                    to: [&message.to],
                    subject: &message.subject,
                    text: &message.text,
                })
                .send()
                .await
                .map_err(|err| {
                    let kind = if err.is_timeout() {
                        "timeout"
                    } else if err.is_connect() {
                        "connect"
                    } else {
                        "request"
                    };
                    EmailError(format!("resend {kind} error"))
                })?;
            let status = response.status();
            if status.is_success() {
                tracing::info!(target: "email", kind = message.kind.as_str(), "e-mail sent");
                Ok(())
            } else {
                Err(EmailError(format!("resend answered {}", status.as_u16())))
            }
        })
    }
}

/// Test driver: records every message.
#[derive(Debug, Default, Clone)]
pub struct RecordingEmailSender {
    sent: Arc<std::sync::Mutex<Vec<EmailMessage>>>,
}

impl RecordingEmailSender {
    /// Messages sent so far, oldest first.
    pub fn sent(&self) -> Vec<EmailMessage> {
        self.sent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl EmailSender for RecordingEmailSender {
    fn send<'a>(&'a self, message: &'a EmailMessage) -> SendFuture<'a> {
        Box::pin(async move {
            self.sent
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(message.clone());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> EmailMessage {
        EmailMessage {
            kind: EmailKind::VerifyEmail,
            to: "ada@example.test".into(),
            subject: "Subject".into(),
            text: "Body https://app.test/verify-email?token=abc".into(),
        }
    }

    #[test]
    fn debug_hides_recipient_and_body() {
        let debug = format!("{:?}", message());
        assert!(!debug.contains("ada@example.test"));
        assert!(!debug.contains("token"));
        assert!(debug.contains("VerifyEmail"));
    }

    #[tokio::test]
    async fn recording_sender_keeps_messages() {
        let sender = RecordingEmailSender::default();
        let shared: SharedEmailSender = Arc::new(sender.clone());
        shared.send(&message()).await.unwrap();
        assert_eq!(sender.sent(), [message()]);
    }

    #[tokio::test]
    async fn log_sender_never_fails() {
        LogEmailSender.send(&message()).await.unwrap();
    }

    /// A fake Resend API that records the request and answers with `status`.
    async fn fake_resend(
        status: u16,
    ) -> (
        String,
        Arc<std::sync::Mutex<Option<(String, serde_json::Value)>>>,
    ) {
        use axum::http::HeaderMap;
        let seen = Arc::new(std::sync::Mutex::new(None));
        let record = seen.clone();
        let app = axum::Router::new().route(
            "/emails",
            axum::routing::post(
                move |headers: HeaderMap, axum::Json(body): axum::Json<serde_json::Value>| {
                    let record = record.clone();
                    async move {
                        let auth = headers
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_owned();
                        *record.lock().unwrap() = Some((auth, body));
                        axum::http::StatusCode::from_u16(status).unwrap()
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    #[tokio::test]
    async fn resend_sender_posts_the_message_with_the_api_key() {
        let (url, seen) = fake_resend(200).await;
        let sender = ResendEmailSender::with_endpoint(url, "re_test", "Routune <noreply@x.test>");
        sender.send(&message()).await.unwrap();
        let (auth, body) = seen.lock().unwrap().clone().unwrap();
        assert_eq!(auth, "Bearer re_test");
        assert_eq!(
            body,
            serde_json::json!({
                "from": "Routune <noreply@x.test>",
                "to": ["ada@example.test"],
                "subject": "Subject",
                "text": "Body https://app.test/verify-email?token=abc",
            })
        );
    }

    #[tokio::test]
    async fn resend_errors_carry_only_the_status() {
        let (url, _) = fake_resend(422).await;
        let sender = ResendEmailSender::with_endpoint(url, "re_secret", "noreply@x.test");
        let err = sender.send(&message()).await.unwrap_err().to_string();
        assert_eq!(err, "e-mail delivery failed: resend answered 422");
    }

    #[tokio::test]
    async fn resend_transport_errors_do_not_leak_the_message() {
        // Nothing listens on port 9 of the loopback.
        let sender =
            ResendEmailSender::with_endpoint("http://127.0.0.1:9", "re_secret", "n@x.test");
        let err = sender.send(&message()).await.unwrap_err().to_string();
        assert!(
            !err.contains("ada@example.test") && !err.contains("re_secret"),
            "{err}"
        );
    }

    #[test]
    fn resend_debug_hides_the_api_key() {
        let sender = ResendEmailSender::new("re_secret", "noreply@x.test");
        assert!(!format!("{sender:?}").contains("re_secret"));
    }
}
