//! Outgoing e-mail (ARCHITECTURE §14, T0.3.3).
//!
//! Modules send through the [`EmailSender`] trait and never know the provider. The transactional
//! provider is chosen later (ARCHITECTURE §14); until then only [`LogEmailSender`] exists.
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
}

impl EmailKind {
    /// Stable name for logs and metrics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify_email",
            Self::ResetPassword => "reset_password",
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
}
