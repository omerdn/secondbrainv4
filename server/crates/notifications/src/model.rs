//! Notification DTOs (PRD BL1–BL3).

use chrono::{DateTime, Utc};
use items::InvitationRole;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Notifications in one page of the center.
pub const NOTIFICATION_PAGE: i64 = 50;

/// What a notification is about (BL2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NotificationKind {
    Invitation,
    Assignment,
    RoutineReminder,
    Nudge,
    Reaction,
    SharingChange,
    ForgottenReminder,
    WeeklySummary,
}

impl NotificationKind {
    pub const ALL: [Self; 8] = [
        Self::Invitation,
        Self::Assignment,
        Self::RoutineReminder,
        Self::Nudge,
        Self::Reaction,
        Self::SharingChange,
        Self::ForgottenReminder,
        Self::WeeklySummary,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Invitation => "invitation",
            Self::Assignment => "assignment",
            Self::RoutineReminder => "routine_reminder",
            Self::Nudge => "nudge",
            Self::Reaction => "reaction",
            Self::SharingChange => "sharing_change",
            Self::ForgottenReminder => "forgotten_reminder",
            Self::WeeklySummary => "weekly_summary",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// The channels of a type nobody changed: e-mail for invitations, which wait for an answer,
    /// and summaries (BL1, T3.7.1).
    #[must_use]
    pub fn default_channels(self) -> Vec<Channel> {
        match self {
            Self::Invitation => vec![Channel::InApp, Channel::Push, Channel::Email],
            Self::WeeklySummary => vec![Channel::InApp, Channel::Email],
            _ => vec![Channel::InApp, Channel::Push],
        }
    }
}

/// Where a notification goes (BL1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "NotificationChannel")]
pub enum Channel {
    InApp,
    Push,
    Email,
}

impl Channel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InApp => "in_app",
            Self::Push => "push",
            Self::Email => "email",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        [Self::InApp, Self::Push, Self::Email]
            .into_iter()
            .find(|channel| channel.as_str() == value)
    }
}

/// What the center shows, by type; written when the notification is made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum NotificationPayload {
    /// Someone invited the user to an item (G5).
    Invitation {
        invitation_id: Uuid,
        item_id: Uuid,
        item_kind: String,
        item_title: String,
        from_user: Uuid,
        from_name: String,
        role: InvitationRole,
    },
    /// A routine's reminder time came (R13).
    RoutineReminder {
        routine_id: Uuid,
        title: String,
        #[ts(type = "string")]
        local_date: chrono::NaiveDate,
        /// `HH:MM` in the routine's time zone.
        time: String,
    },
    /// A friend nudged the user (AK3).
    Nudge { from_user: Uuid, from_name: String },
    /// A friend reacted to something the user completed (AK4).
    Reaction {
        activity_id: Uuid,
        item_id: Uuid,
        item_kind: String,
        item_title: String,
        from_user: Uuid,
        from_name: String,
        emoji: String,
    },
    /// Someone assigned the user to part of an item (BL2, G4, T4.3.7).
    Assignment {
        item_id: Uuid,
        item_kind: String,
        item_title: String,
        /// The subtask, when the assignment is to one.
        subtask_title: Option<String>,
        from_user: Uuid,
        from_name: String,
    },
    /// The weekly summary (OZ1).
    WeeklySummary(WeeklySummary),
}

/// A routine's week in the summary (OZ1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SummaryRoutine {
    pub routine_id: Uuid,
    pub title: String,
    /// Days of the week the routine was due and complete.
    pub done_days: u32,
    /// Days of the week the routine was due.
    pub scheduled_days: u32,
    /// The streak as the week ended (R9).
    pub streak: u32,
}

/// A goal's progress in the summary (OZ1), in whole percent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SummaryGoal {
    pub goal_id: Uuid,
    pub title: String,
    pub actual_percent: u32,
    pub expected_percent: u32,
}

/// Work session time on one item during the week (OZ1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SummarySession {
    /// `None`: time not tied to an item.
    pub item_id: Option<Uuid>,
    pub title: String,
    pub minutes: u32,
}

/// The weekly summary (OZ1): last Monday to Sunday in the user's time zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WeeklySummary {
    #[ts(type = "string")]
    pub week_start: chrono::NaiveDate,
    #[ts(type = "string")]
    pub week_end: chrono::NaiveDate,
    pub routines: Vec<SummaryRoutine>,
    pub goals: Vec<SummaryGoal>,
    pub sessions: Vec<SummarySession>,
    pub work_minutes: u32,
    /// Titles of items untouched for a while (H4), at most a few.
    pub forgotten: Vec<String>,
}

impl WeeklySummary {
    /// Whether there is anything to tell.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.routines.is_empty()
            && self.goals.is_empty()
            && self.sessions.is_empty()
            && self.forgotten.is_empty()
    }
}

impl NotificationPayload {
    #[must_use]
    pub const fn kind(&self) -> NotificationKind {
        match self {
            Self::WeeklySummary(_) => NotificationKind::WeeklySummary,
            Self::Assignment { .. } => NotificationKind::Assignment,
            Self::Invitation { .. } => NotificationKind::Invitation,
            Self::RoutineReminder { .. } => NotificationKind::RoutineReminder,
            Self::Nudge { .. } => NotificationKind::Nudge,
            Self::Reaction { .. } => NotificationKind::Reaction,
        }
    }
}

/// One notification of the center.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Notification {
    pub id: Uuid,
    pub payload: NotificationPayload,
    #[ts(type = "string | null")]
    pub read_at: Option<DateTime<Utc>>,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
}

/// `GET /notifications`: a page, newest first, and how many are unread in all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct NotificationList {
    pub notifications: Vec<Notification>,
    #[ts(type = "number")]
    pub unread: i64,
}

/// `GET /notifications` query.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct NotificationsQuery {
    /// Only notifications made before this time (the next page).
    pub before: Option<DateTime<Utc>>,
}

/// A type's channels (BL3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct NotificationPreference {
    pub kind: NotificationKind,
    pub channels: Vec<Channel>,
    /// The user has not changed it: these are the type's defaults.
    pub default: bool,
}

/// `PUT /notifications/preferences/{kind}` body; no channel at all turns the type off.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "UpdateNotificationPreferenceRequest")]
pub struct UpdatePreferenceRequest {
    pub channels: Vec<Channel>,
}
