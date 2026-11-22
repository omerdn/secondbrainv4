//! Domain events published by `identity` (outbox, ARCHITECTURE §4.5).

use kernel::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::model::{SessionId, UserId};

/// Sessions ended before their expiry: sign-out, `DELETE /auth/sessions/{id}`, or a password
/// reset (all of the user's sessions). `realtime` closes their WebSocket connections (T0.5.6).
/// Expired sessions are not announced; holders find out on their next check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionsEnded {
    pub user_id: UserId,
    /// The ended session; `None` means every session of the user.
    pub session_id: Option<SessionId>,
}

impl DomainEvent for SessionsEnded {
    const EVENT_TYPE: &'static str = "identity.sessions_ended";
}
