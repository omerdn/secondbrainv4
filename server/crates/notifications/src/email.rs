//! Invitation e-mails (PRD BL1, BL3, ADR-0022, T3.7.1).
//!
//! An item invitation, or a friend request that reached someone with an account, becomes an
//! e-mail when the recipient's "invitation" channels include e-mail (the default). The event's
//! handler only queues a [`SendEmail`] job, in its own transaction and once per event; the job
//! reads the recipient's address when it runs and sends through the shared
//! [`kernel::EmailSender`], so a provider error is retried by `jobs` without blocking anything.
//! Neither the address nor the text is ever logged: only the job's kind and id are.

use identity::{IdentityService, UserId};
use kernel::email::{EmailKind, EmailMessage, SharedEmailSender};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What an invitation e-mail says; rendered when the job runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Template {
    /// Someone invited the user to an item (G5).
    Invitation {
        from_name: String,
        item_kind: String,
        item_title: String,
        role: String,
    },
    /// Someone asked the user to be friends (AK1).
    FriendRequest { from_name: String },
}

/// A queued e-mail to one user (by id: the address is read when it is sent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendEmail {
    pub user_id: Uuid,
    pub template: Template,
}

impl jobs::Job for SendEmail {
    const KIND: &'static str = "notifications.send_email";
    /// The provider may be down for a while; retries back off (ADR-0022).
    const MAX_ATTEMPTS: u32 = 8;
}

/// The Turkish name of an item kind, with the dative suffix for "…e davet etti".
fn kind_dative(kind: &str) -> &'static str {
    match kind {
        "task" => "bir göreve",
        "routine" => "bir rutine",
        "goal" => "bir hedefe",
        "tracker" => "bir takibe",
        "notebook" => "bir deftere",
        _ => "bir nesneye",
    }
}

fn role_name(role: &str) -> &'static str {
    match role {
        "assignee" => "sorumlu",
        "watcher" => "izleyici",
        "editor" => "düzenleyici",
        "viewer" => "görüntüleyici",
        _ => "üye",
    }
}

/// The e-mail for `template`, in Turkish plain text (BL1). Links go to the web app.
#[must_use]
pub fn render(
    template: &Template,
    recipient_name: &str,
    web_url: &str,
) -> (EmailKind, String, String) {
    let web = web_url.trim_end_matches('/');
    let greeting = if recipient_name.trim().is_empty() {
        "Merhaba,".to_owned()
    } else {
        format!("Merhaba {},", recipient_name.trim())
    };
    match template {
        Template::Invitation {
            from_name,
            item_kind,
            item_title,
            role,
        } => (
            EmailKind::Invitation,
            format!("{from_name} seni {} davet etti", kind_dative(item_kind)),
            format!(
                "{greeting}\n\n{from_name} seni \"{item_title}\" için {} olarak davet etti.\n\n\
                 Daveti kabul etmek ya da reddetmek için bildirimlerine bak:\n{web}/notifications\n\n\
                 Kabul etmezsen hiçbir şey değişmez.\n\nRoutune",
                role_name(role)
            ),
        ),
        Template::FriendRequest { from_name } => (
            EmailKind::FriendRequest,
            format!("{from_name} seninle arkadaş olmak istiyor"),
            format!(
                "{greeting}\n\n{from_name} sana Routune'da arkadaşlık isteği gönderdi.\n\n\
                 İsteği kabul etmek ya da reddetmek için:\n{web}/friends\n\nRoutune"
            ),
        ),
    }
}

/// Registers the e-mail job (call from `app::run`).
pub fn register_email(
    worker: &mut jobs::Worker,
    identity: IdentityService,
    sender: SharedEmailSender,
    web_url: String,
) {
    worker.register(move |job: SendEmail, _| {
        let (identity, sender, web_url) = (identity.clone(), sender.clone(), web_url.clone());
        async move {
            let boxed =
                |err: Box<dyn std::error::Error + Send + Sync>| -> jobs::HandlerError { err };
            // Someone who deleted their account gets nothing; nothing to retry.
            let user = match identity.current_user(UserId::from_uuid(job.user_id)).await {
                Ok(user) => user,
                Err(kernel::AppError::NotFound | kernel::AppError::Unauthenticated) => {
                    return Ok(());
                }
                Err(err) => return Err(boxed(Box::new(err))),
            };
            let (kind, subject, text) = render(&job.template, &user.display_name, &web_url);
            let message = EmailMessage {
                kind,
                to: user.email,
                subject,
                text,
            };
            sender
                .send(&message)
                .await
                .map_err(|err| boxed(Box::new(err)))?;
            Ok(())
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_mail_says_who_what_and_where_in_turkish() {
        let (kind, subject, text) = render(
            &Template::Invitation {
                from_name: "Ali".into(),
                item_kind: "task".into(),
                item_title: "Rapor".into(),
                role: "assignee".into(),
            },
            "Ayşe",
            "https://app.routune.dev/",
        );
        assert_eq!(kind, EmailKind::Invitation);
        assert_eq!(subject, "Ali seni bir göreve davet etti");
        assert!(text.starts_with("Merhaba Ayşe,"));
        assert!(text.contains("\"Rapor\" için sorumlu olarak"));
        assert!(text.contains("https://app.routune.dev/notifications"));
        assert!(!text.contains("<"), "plain text only");
    }

    #[test]
    fn friend_request_mail_links_to_friends() {
        let (kind, subject, text) = render(
            &Template::FriendRequest {
                from_name: "Ali".into(),
            },
            "",
            "http://localhost:8081",
        );
        assert_eq!(kind, EmailKind::FriendRequest);
        assert_eq!(subject, "Ali seninle arkadaş olmak istiyor");
        assert!(text.starts_with("Merhaba,"));
        assert!(text.contains("http://localhost:8081/friends"));
    }

    #[test]
    fn every_kind_and_role_reads_naturally() {
        for kind in ["task", "routine", "goal", "tracker", "notebook", "page"] {
            assert!(kind_dative(kind).starts_with("bir "));
        }
        assert_eq!(role_name("editor"), "düzenleyici");
        assert_eq!(role_name("unknown"), "üye");
    }
}
