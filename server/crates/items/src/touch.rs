//! `last_touched_at` rules (PRD H4, T1.2.6): which actions count as touching an item, for the
//! "forgotten items" list.
//!
//! Reading is not a touch. Neither is anything other members must not be able to notice: `items`
//! rows are shared, so a personal tag (E2), a link whose other end they cannot see (V9) or an
//! unanswered invitation (G5) must not move a timestamp they can read.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

/// Something that happened to an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchAction {
    Viewed,
    Edited,
    Completed,
    /// A value was entered on a tracker.
    EntryLogged,
    /// Someone accepted an invitation and joined.
    MemberJoined,
    Unarchived,
    Restored,
    Archived,
    Trashed,
    Linked,
    Unlinked,
    Tagged,
    Invited,
    InvitationDeclined,
}

impl TouchAction {
    /// Whether this action counts as touching the item.
    #[must_use]
    pub const fn is_touch(self) -> bool {
        match self {
            Self::Edited
            | Self::Completed
            | Self::EntryLogged
            | Self::MemberJoined
            | Self::Unarchived
            | Self::Restored => true,
            // Reading; leaving the active set, where H4 no longer looks.
            Self::Viewed | Self::Archived | Self::Trashed => false,
            // Must stay invisible to other members (V9, E2, G5).
            Self::Linked
            | Self::Unlinked
            | Self::Tagged
            | Self::Invited
            | Self::InvitationDeclined => false,
        }
    }
}

/// The new `last_touched_at`. It never moves backwards.
#[must_use]
pub fn touched_at(
    current: DateTime<Utc>,
    action: TouchAction,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    if action.is_touch() {
        current.max(now)
    } else {
        current
    }
}

/// Applies [`touched_at`] to an item; call it inside the transaction of the change.
pub(crate) async fn apply(
    conn: &mut PgConnection,
    item: Uuid,
    action: TouchAction,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    let current: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT last_touched_at FROM items WHERE id = $1 FOR UPDATE")
            .bind(item)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(current) = current else {
        return Ok(());
    };
    let next = touched_at(current, action, now);
    if next != current {
        sqlx::query("UPDATE items SET last_touched_at = $2 WHERE id = $1")
            .bind(item)
            .bind(next)
            .execute(conn)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use TouchAction as A;

    const ALL: [TouchAction; 14] = [
        A::Viewed,
        A::Edited,
        A::Completed,
        A::EntryLogged,
        A::MemberJoined,
        A::Unarchived,
        A::Restored,
        A::Archived,
        A::Trashed,
        A::Linked,
        A::Unlinked,
        A::Tagged,
        A::Invited,
        A::InvitationDeclined,
    ];

    fn base() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap()
    }

    #[test]
    fn only_meaningful_shared_changes_touch() {
        let touches: Vec<TouchAction> = ALL.into_iter().filter(|a| a.is_touch()).collect();
        assert_eq!(
            touches,
            [
                A::Edited,
                A::Completed,
                A::EntryLogged,
                A::MemberJoined,
                A::Unarchived,
                A::Restored
            ]
        );
    }

    #[test]
    fn a_touch_moves_the_time_and_others_keep_it() {
        let now = base() + Duration::days(3);
        for action in ALL {
            let expected = if action.is_touch() { now } else { base() };
            assert_eq!(touched_at(base(), action, now), expected, "{action:?}");
        }
    }

    #[test]
    fn it_never_moves_backwards() {
        let earlier = base() - Duration::hours(1);
        for action in ALL {
            assert_eq!(touched_at(base(), action, earlier), base(), "{action:?}");
        }
    }
}
