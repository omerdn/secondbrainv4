//! Web Push (PRD BL1, BL3, ARCHITECTURE §14, T4.2.4).
//!
//! A browser subscribes with the Push API and hands us its [`PushSubscriptionRequest`]; we keep
//! one row per browser. When a notification's channels include push (BL3), [`Notifications`]
//! queues one [`SendPush`] job per subscription, in the same transaction and once per source
//! event. The job encrypts the message for that browser (RFC 8291), signs a VAPID token
//! (RFC 8292) and posts it to the browser's push service; a subscription the service reports as
//! gone (404/410) is deleted.
//!
//! # Secrets and personal data
//! The VAPID private key comes from the server's `.env` (`VAPID_PRIVATE_KEY`) and never leaves
//! [`VapidKey`]: its `Debug` hides it. An endpoint is a capability URL, so endpoints, keys and
//! message texts are never logged; errors carry only the HTTP status.
//!
//! # SSRF
//! The server posts to the endpoint a client gave it, so only `https` endpoints of the known
//! browser push services are accepted ([`PUSH_SERVICE_HOSTS`]).
//!
//! [`Notifications`]: crate::Notifications

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kernel::validation::{FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;
use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair};
use web_push_native::{Auth, WebPushBuilder, p256};

use crate::model::NotificationPayload;
use crate::service::Notifications;

/// How long a push service keeps an undelivered message (the `TTL` header).
pub const MESSAGE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Hosts (and their subdomains) of the browsers' push services: Chrome and Edge (FCM), Firefox
/// (Mozilla autopush), Safari (Apple) and legacy Edge (WNS).
pub const PUSH_SERVICE_HOSTS: &[&str] = &[
    "fcm.googleapis.com",
    "android.googleapis.com",
    "push.services.mozilla.com",
    "push.apple.com",
    "notify.windows.com",
];

/// Longest endpoint we keep (the column allows the same).
pub const MAX_ENDPOINT_LEN: usize = 2048;

/// `GET /notifications/push/key`: the VAPID public key for `applicationServerKey`; `null` when
/// Web Push is not configured on this server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PushPublicKey {
    pub public_key: Option<String>,
}

/// The browser's keys of a subscription (base64url, as `PushSubscription.toJSON()` gives them).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

/// `PUT /notifications/push/subscriptions` body.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PushSubscriptionRequest {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
}

/// `POST /notifications/push/subscriptions/remove` body.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RemovePushSubscriptionRequest {
    pub endpoint: String,
}

fn invalid(field: &str, message: &str) -> FieldError {
    FieldError::new(field, FieldErrorCode::Invalid, message)
}

/// Whether `host` is one of [`PUSH_SERVICE_HOSTS`] or a subdomain of one.
fn is_push_service(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    PUSH_SERVICE_HOSTS
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
}

/// Checks an endpoint: `https`, a known push service, no credentials or port tricks.
fn check_endpoint(endpoint: &str) -> Result<(), FieldError> {
    if endpoint.len() > MAX_ENDPOINT_LEN {
        return Err(FieldError::new(
            "endpoint",
            FieldErrorCode::TooLong,
            "endpoint is too long",
        ));
    }
    let uri: http::Uri = endpoint
        .parse()
        .map_err(|_| invalid("endpoint", "not a URL"))?;
    let authority = uri.authority().map(http::uri::Authority::as_str);
    match (uri.scheme_str(), uri.host(), authority) {
        (Some("https"), Some(host), Some(authority))
            if authority == host && is_push_service(host) =>
        {
            Ok(())
        }
        _ => Err(invalid("endpoint", "not a browser push service")),
    }
}

fn decode(field: &str, value: &str, len: usize) -> Result<Vec<u8>, FieldError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value.trim_end_matches('='))
        .map_err(|_| invalid(field, "not base64url"))?;
    if bytes.len() == len {
        Ok(bytes)
    } else {
        Err(invalid(field, "wrong length"))
    }
}

impl PushSubscriptionRequest {
    /// Validates the endpoint and the keys (a P-256 point and a 16-byte secret).
    ///
    /// # Errors
    /// [`kernel::AppError::Validation`] naming the bad fields.
    pub fn validate(&self) -> Result<(), kernel::AppError> {
        let mut errors = ValidationErrors::new();
        errors.check(check_endpoint(&self.endpoint));
        errors.check(
            decode("keys.p256dh", &self.keys.p256dh, 65).and_then(|bytes| {
                p256::PublicKey::from_sec1_bytes(&bytes)
                    .map(|_| ())
                    .map_err(|_| invalid("keys.p256dh", "not a P-256 public key"))
            }),
        );
        errors.check(decode("keys.auth", &self.keys.auth, 16).map(|_| ()));
        errors.into_result()
    }
}

/// The server's VAPID key pair (from `VAPID_PRIVATE_KEY`) and contact (`VAPID_SUBJECT`).
#[derive(Clone)]
pub struct VapidKey {
    key_pair: Arc<ES256KeyPair>,
    public_key: String,
    subject: String,
}

/// The private key is a secret: never in debug output.
impl fmt::Debug for VapidKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VapidKey")
            .field("public_key", &self.public_key)
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

/// `VAPID_PRIVATE_KEY` or `VAPID_SUBJECT` is unusable. Never contains the key.
#[derive(Debug, thiserror::Error)]
pub enum VapidKeyError {
    #[error(
        "must be a base64url P-256 private key (32 bytes); generate one with `cargo run -p notifications --example generate_vapid_key`"
    )]
    PrivateKey,
    #[error("must be a mailto: address or an https:// URL")]
    Subject,
}

impl VapidKey {
    /// Parses a base64url (unpadded) raw P-256 private key and the contact the push services
    /// may use (`mailto:` or `https://`; `http://` only for a local server).
    ///
    /// # Errors
    /// [`VapidKeyError`] when either is malformed.
    pub fn parse(private_key: &str, subject: &str) -> Result<Self, VapidKeyError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(private_key.trim().trim_end_matches('='))
            .map_err(|_| VapidKeyError::PrivateKey)?;
        if bytes.len() != 32 {
            return Err(VapidKeyError::PrivateKey);
        }
        let key_pair = ES256KeyPair::from_bytes(&bytes).map_err(|_| VapidKeyError::PrivateKey)?;
        let subject = subject.trim().to_owned();
        if !(subject.starts_with("mailto:")
            || subject.starts_with("https://")
            || subject.starts_with("http://localhost"))
        {
            return Err(VapidKeyError::Subject);
        }
        let public_key =
            URL_SAFE_NO_PAD.encode(key_pair.public_key().public_key().to_bytes_uncompressed());
        Ok(Self {
            key_pair: Arc::new(key_pair),
            public_key,
            subject,
        })
    }

    /// A new random private key, base64url (for `.env`).
    #[must_use]
    pub fn generate_private_key() -> String {
        URL_SAFE_NO_PAD.encode(ES256KeyPair::generate().to_bytes())
    }

    /// The public key for the browser's `applicationServerKey` (base64url, uncompressed point).
    #[must_use]
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// The encrypted, signed request that delivers `body` to `target`.
    ///
    /// # Errors
    /// [`PushError`] when the stored keys are unusable or signing fails.
    pub fn request(
        &self,
        target: &PushTarget,
        body: &[u8],
    ) -> Result<http::Request<Vec<u8>>, PushError> {
        let endpoint: http::Uri = target
            .endpoint
            .parse()
            .map_err(|_| PushError("bad endpoint".into()))?;
        let p256dh = decode("p256dh", &target.p256dh, 65)
            .ok()
            .and_then(|bytes| p256::PublicKey::from_sec1_bytes(&bytes).ok())
            .ok_or_else(|| PushError("bad subscription key".into()))?;
        let auth: [u8; 16] = decode("auth", &target.auth, 16)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| PushError("bad subscription secret".into()))?;
        WebPushBuilder::new(endpoint, p256dh, Auth::from(auth))
            .with_valid_duration(MESSAGE_TTL)
            .with_vapid(&self.key_pair, &self.subject)
            .build(body.to_vec())
            .map_err(|_| PushError("encryption or signing failed".into()))
    }
}

/// Where one message goes: a stored subscription.
#[derive(Clone, PartialEq, Eq)]
pub struct PushTarget {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

/// The endpoint and keys are capability secrets: never in debug output.
impl fmt::Debug for PushTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushTarget").finish_non_exhaustive()
    }
}

/// What the push service said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// Accepted for delivery.
    Delivered,
    /// The subscription no longer exists (404/410): forget it.
    Gone,
}

/// Delivery failed; worth retrying. Carries only a status or a transport error kind.
#[derive(Debug, thiserror::Error)]
#[error("web push failed: {0}")]
pub struct PushError(pub String);

/// Boxed future of [`PushSender::send`] (keeps the trait object safe).
pub type PushFuture<'a> = Pin<Box<dyn Future<Output = Result<PushOutcome, PushError>> + Send + 'a>>;

/// A Web Push delivery driver.
pub trait PushSender: Send + Sync + fmt::Debug {
    /// Delivers one message (the JSON of a [`PushMessage`]) to one subscription.
    fn send<'a>(&'a self, target: &'a PushTarget, body: &'a [u8]) -> PushFuture<'a>;
}

/// Shared driver for application state.
pub type SharedPushSender = Arc<dyn PushSender>;

/// Posts to the browsers' push services over HTTPS.
#[derive(Debug, Clone)]
pub struct WebPushSender {
    vapid: VapidKey,
    http: reqwest::Client,
}

impl WebPushSender {
    #[must_use]
    pub fn new(vapid: VapidKey) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            // An endpoint was checked when it was stored; never follow it elsewhere.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self { vapid, http }
    }
}

impl PushSender for WebPushSender {
    fn send<'a>(&'a self, target: &'a PushTarget, body: &'a [u8]) -> PushFuture<'a> {
        Box::pin(async move {
            // Checked again: a row may predate a narrower allowlist.
            if check_endpoint(&target.endpoint).is_err() {
                return Ok(PushOutcome::Gone);
            }
            let request = self.vapid.request(target, body)?;
            let request =
                reqwest::Request::try_from(request).map_err(|_| PushError("bad request".into()))?;
            let response = self.http.execute(request).await.map_err(|err| {
                let kind = if err.is_timeout() {
                    "timeout"
                } else if err.is_connect() {
                    "connect"
                } else {
                    "request"
                };
                PushError(format!("{kind} error"))
            })?;
            let status = response.status();
            outcome_of(status.as_u16())
        })
    }
}

/// What a push service's status means (RFC 8030 §5, §7.3).
fn outcome_of(status: u16) -> Result<PushOutcome, PushError> {
    match status {
        200..=299 => Ok(PushOutcome::Delivered),
        404 | 410 => Ok(PushOutcome::Gone),
        // A key mismatch or a malformed subscription never heals by itself.
        400 | 403 | 413 => Ok(PushOutcome::Gone),
        other => Err(PushError(format!("push service answered {other}"))),
    }
}

/// Test driver: records every message and answers [`PushOutcome::Delivered`], or
/// [`PushOutcome::Gone`] for endpoints marked gone.
#[derive(Debug, Default, Clone)]
pub struct RecordingPushSender {
    sent: Arc<std::sync::Mutex<Vec<(String, PushMessage)>>>,
    gone: Arc<std::sync::Mutex<Vec<String>>>,
}

impl RecordingPushSender {
    /// Every `(endpoint, message)` sent so far.
    #[must_use]
    pub fn sent(&self) -> Vec<(String, PushMessage)> {
        self.sent.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// From now on, `endpoint` answers 410 Gone.
    pub fn mark_gone(&self, endpoint: &str) {
        if let Ok(mut gone) = self.gone.lock() {
            gone.push(endpoint.to_owned());
        }
    }
}

impl PushSender for RecordingPushSender {
    fn send<'a>(&'a self, target: &'a PushTarget, body: &'a [u8]) -> PushFuture<'a> {
        Box::pin(async move {
            if self
                .gone
                .lock()
                .is_ok_and(|gone| gone.contains(&target.endpoint))
            {
                return Ok(PushOutcome::Gone);
            }
            let message: PushMessage =
                serde_json::from_slice(body).map_err(|_| PushError("bad body".into()))?;
            if let Ok(mut sent) = self.sent.lock() {
                sent.push((target.endpoint.clone(), message));
            }
            Ok(PushOutcome::Delivered)
        })
    }
}

/// What the service worker shows (`apps/client/public/sw.js`): a title, a line and the web
/// app path to open on click. `tag` replaces an older notification about the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushMessage {
    pub title: String,
    pub body: String,
    pub url: String,
    pub tag: String,
}

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

/// The Turkish push text of a notification (BL1).
#[must_use]
pub fn message_of(payload: &NotificationPayload) -> PushMessage {
    match payload {
        NotificationPayload::Invitation {
            invitation_id,
            item_kind,
            item_title,
            from_name,
            ..
        } => PushMessage {
            title: format!("{from_name} seni {} davet etti", kind_dative(item_kind)),
            body: item_title.clone(),
            url: "/notifications".into(),
            tag: format!("invitation:{invitation_id}"),
        },
        NotificationPayload::RoutineReminder {
            routine_id,
            title,
            local_date,
            time,
        } => PushMessage {
            title: title.clone(),
            body: format!("Rutin hatırlatması · {time}"),
            url: format!("/routines/{routine_id}"),
            tag: format!("reminder:{routine_id}:{local_date}:{time}"),
        },
        NotificationPayload::Nudge {
            from_user,
            from_name,
        } => PushMessage {
            title: format!("{from_name} seni dürttü"),
            body: "Bugünkü işlerine bir göz at.".into(),
            url: "/".into(),
            tag: format!("nudge:{from_user}"),
        },
        NotificationPayload::WeeklySummary(summary) => PushMessage {
            title: "Haftalık özetin hazır".into(),
            body: format!(
                "{} – {}",
                summary.week_start.format("%d.%m"),
                summary.week_end.format("%d.%m")
            ),
            url: "/notifications".into(),
            tag: format!("weekly:{}", summary.week_start),
        },
        NotificationPayload::Reaction {
            activity_id,
            item_title,
            from_name,
            emoji,
            ..
        } => PushMessage {
            title: format!("{from_name} {emoji} ile tepki verdi"),
            body: item_title.clone(),
            url: "/notifications".into(),
            tag: format!("reaction:{activity_id}"),
        },
    }
}

/// One message to one stored subscription; queued by [`Notifications::deliver`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendPush {
    pub subscription_id: Uuid,
    pub payload: NotificationPayload,
}

impl jobs::Job for SendPush {
    const KIND: &'static str = "notifications.send_push";
    /// A push service may be down for a while; a day-old reminder is useless anyway.
    const MAX_ATTEMPTS: u32 = 5;
}

/// Registers the push job (call from `app::run`).
pub fn register_push(
    worker: &mut jobs::Worker,
    notifications: Notifications,
    sender: SharedPushSender,
) {
    worker.register(move |job: SendPush, _| {
        let (notifications, sender) = (notifications.clone(), sender.clone());
        async move {
            let boxed =
                |err: Box<dyn std::error::Error + Send + Sync>| -> jobs::HandlerError { err };
            // Unsubscribed (or the account is gone) since: nothing to send.
            let Some(target) = notifications
                .push_target(job.subscription_id)
                .await
                .map_err(|err| boxed(Box::new(err)))?
            else {
                return Ok(());
            };
            let body = serde_json::to_vec(&message_of(&job.payload))
                .map_err(|err| boxed(Box::new(err)))?;
            match sender
                .send(&target, &body)
                .await
                .map_err(|err| boxed(Box::new(err)))?
            {
                PushOutcome::Delivered => {
                    tracing::info!(target: "push", kind = job.payload.kind().as_str(), "push sent");
                }
                PushOutcome::Gone => {
                    notifications
                        .forget_push_subscription(job.subscription_id)
                        .await
                        .map_err(|err| boxed(Box::new(err)))?;
                    tracing::info!(target: "push", "push subscription gone; removed");
                }
            }
            Ok(())
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const ENDPOINT: &str = "https://fcm.googleapis.com/fcm/send/abc";

    fn browser() -> (p256::SecretKey, Vec<u8>, PushSubscriptionRequest) {
        use p256::elliptic_curve::sec1::ToEncodedPoint;
        let secret = p256::SecretKey::from_slice(&ES256KeyPair::generate().to_bytes()).unwrap();
        let public = secret.public_key().to_encoded_point(false);
        let auth = vec![7u8; 16];
        let request = PushSubscriptionRequest {
            endpoint: ENDPOINT.into(),
            keys: PushSubscriptionKeys {
                p256dh: URL_SAFE_NO_PAD.encode(public.as_bytes()),
                auth: URL_SAFE_NO_PAD.encode(&auth),
            },
        };
        (secret, auth, request)
    }

    #[test]
    fn only_https_push_services_are_accepted() {
        for ok in [
            ENDPOINT,
            "https://updates.push.services.mozilla.com/wpush/v2/x",
            "https://web.push.apple.com/QGx",
            "https://wns2-par02p.notify.windows.com/w/?token=x",
        ] {
            assert!(check_endpoint(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://fcm.googleapis.com/fcm/send/abc",
            "https://evil.example.com/fcm.googleapis.com",
            "https://fcm.googleapis.com.evil.com/x",
            "https://evilpush.apple.com/x",
            "https://user@fcm.googleapis.com/x",
            "https://fcm.googleapis.com:8443/x",
            "https://127.0.0.1/x",
            "not a url",
        ] {
            assert!(check_endpoint(bad).is_err(), "{bad}");
        }
        assert!(check_endpoint(&format!("{ENDPOINT}/{}", "a".repeat(MAX_ENDPOINT_LEN))).is_err());
    }

    #[test]
    fn keys_must_be_a_point_and_a_secret() {
        let (_, _, good) = browser();
        assert!(good.validate().is_ok());
        let mut bad = good.clone();
        bad.keys.p256dh = URL_SAFE_NO_PAD.encode([4u8; 65]);
        bad.keys.auth = "short".into();
        let Err(kernel::AppError::Validation(errors)) = bad.validate() else {
            panic!("expected a validation error");
        };
        let fields: Vec<_> = errors.errors().iter().map(|e| e.field.clone()).collect();
        assert_eq!(fields, ["keys.p256dh", "keys.auth"]);
    }

    #[test]
    fn a_message_is_encrypted_for_the_browser_and_signed() {
        let vapid = VapidKey::parse(
            &VapidKey::generate_private_key(),
            "mailto:noreply@example.com",
        )
        .unwrap();
        assert_eq!(
            URL_SAFE_NO_PAD.decode(vapid.public_key()).unwrap().len(),
            65
        );
        let (secret, auth, sub) = browser();
        let target = PushTarget {
            endpoint: sub.endpoint,
            p256dh: sub.keys.p256dh,
            auth: sub.keys.auth,
        };
        let request = vapid.request(&target, b"{\"title\":\"x\"}").unwrap();
        assert_eq!(request.uri(), ENDPOINT);
        assert_eq!(request.headers()["content-encoding"], "aes128gcm");
        assert_eq!(request.headers()["ttl"], "86400");
        let authorization = request.headers()["authorization"].to_str().unwrap();
        assert!(authorization.starts_with("vapid t="));
        assert!(authorization.contains(&format!("k={}", vapid.public_key())));
        let plain = web_push_native::decrypt(
            request.body().clone(),
            &secret,
            &Auth::from(<[u8; 16]>::try_from(auth).unwrap()),
        )
        .unwrap();
        assert_eq!(plain, b"{\"title\":\"x\"}");
    }

    #[test]
    fn vapid_settings_are_checked_and_the_key_is_never_printed() {
        let private = VapidKey::generate_private_key();
        assert!(VapidKey::parse("nope", "mailto:a@b.c").is_err());
        assert!(VapidKey::parse(&URL_SAFE_NO_PAD.encode([1u8; 31]), "mailto:a@b.c").is_err());
        assert!(VapidKey::parse(&private, "ftp://x").is_err());
        assert!(VapidKey::parse(&private, "http://example.com").is_err());
        assert!(VapidKey::parse(&private, "https://app.routune.dev").is_ok());
        let key = VapidKey::parse(&private, "http://localhost:8081").unwrap();
        assert!(!format!("{key:?}").contains(&private));
        let target = PushTarget {
            endpoint: ENDPOINT.into(),
            p256dh: "p".into(),
            auth: "a".into(),
        };
        assert!(!format!("{target:?}").contains("fcm"));
    }

    #[test]
    fn push_service_statuses() {
        assert_eq!(outcome_of(201).unwrap(), PushOutcome::Delivered);
        assert_eq!(outcome_of(410).unwrap(), PushOutcome::Gone);
        assert_eq!(outcome_of(404).unwrap(), PushOutcome::Gone);
        assert_eq!(outcome_of(403).unwrap(), PushOutcome::Gone);
        assert!(outcome_of(429).is_err());
        assert!(outcome_of(503).is_err());
    }

    #[test]
    fn messages_are_turkish_and_open_the_right_page() {
        let routine = Uuid::now_v7();
        let reminder = message_of(&NotificationPayload::RoutineReminder {
            routine_id: routine,
            title: "Su iç".into(),
            local_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 27).unwrap(),
            time: "09:00".into(),
        });
        assert_eq!(reminder.title, "Su iç");
        assert_eq!(reminder.body, "Rutin hatırlatması · 09:00");
        assert_eq!(reminder.url, format!("/routines/{routine}"));
        let nudge = message_of(&NotificationPayload::Nudge {
            from_user: Uuid::now_v7(),
            from_name: "Ali".into(),
        });
        assert_eq!(nudge.title, "Ali seni dürttü");
        assert_eq!(nudge.url, "/");
        let reaction = message_of(&NotificationPayload::Reaction {
            activity_id: Uuid::now_v7(),
            item_id: Uuid::now_v7(),
            item_kind: "task".into(),
            item_title: "Rapor".into(),
            from_user: Uuid::now_v7(),
            from_name: "Ali".into(),
            emoji: "🎉".into(),
        });
        assert_eq!(reaction.title, "Ali 🎉 ile tepki verdi");
        assert_eq!(reaction.body, "Rapor");
    }
}
