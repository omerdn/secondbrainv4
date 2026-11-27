//! `notifications`: Notification center, push, e-mail and preferences (PRD BL1–BL3,
//! ARCHITECTURE §5.10, §14).
//!
//! - [`Notifications`]: the in-app center (T2.8.1): notifications made from outbox events, one
//!   per event and user, listed and marked read by their recipient only, and announced with a
//!   `notification.new` realtime event on the user's channel.
//! - Channel preferences per type (BL3); push (T4.2.3, T4.2.4) and e-mail build on them.
//! - [`register`]: turns other modules' events into notifications (invitations, G5).
//! - [`push`]: Web Push to the browsers a user subscribed (T4.2.4).
//! - [`email`]: invitation and friend request e-mails, queued as jobs when the recipient's
//!   channels include e-mail (BL1, BL3, T3.7.1).

pub mod email;
pub mod http;
mod model;
pub mod push;
mod repo;
mod service;
mod subscribers;
mod summary;

pub use model::{
    Channel, NOTIFICATION_PAGE, Notification, NotificationKind, NotificationList,
    NotificationPayload, NotificationPreference, NotificationsQuery, SummaryGoal, SummaryRoutine,
    SummarySession, UpdatePreferenceRequest, WeeklySummary,
};
pub use service::{NEW_EVENT, Notifications};
pub use subscribers::register;
pub use summary::summary_text;
