//! Activity log (PRD A1, A2, T1.2.5): who did what to an item, and when. Events are written in
//! the transaction of the change. A reader sees an event only if they can view the item and, for
//! a link, its other end too (V9). Personal tags are never recorded (E2), and neither are pending
//! or declined invitations (G5): only an accepted one changes anything.

use access::Perms;
use chrono::{DateTime, Utc};
use kernel::{AppError, Page, PageRequest};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{FromRow, PgConnection};
use ts_rs::TS;
use uuid::Uuid;

use crate::invitations::InvitationRole;
use crate::service::ItemsService;
use crate::touch::{self, TouchAction};

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ActivityKind {
    Created,
    Completed,
    /// Someone became an assignee.
    Assigned,
    /// Someone became a watcher or a notebook member.
    Shared,
    Linked,
    Unlinked,
}

impl ActivityKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Completed => "completed",
            Self::Assigned => "assigned",
            Self::Shared => "shared",
            Self::Linked => "linked",
            Self::Unlinked => "unlinked",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "created" => Some(Self::Created),
            "completed" => Some(Self::Completed),
            "assigned" => Some(Self::Assigned),
            "shared" => Some(Self::Shared),
            "linked" => Some(Self::Linked),
            "unlinked" => Some(Self::Unlinked),
            _ => None,
        }
    }

    /// The event for someone joining an item with `role`.
    pub(crate) const fn for_role(role: InvitationRole) -> Self {
        match role {
            InvitationRole::Assignee => Self::Assigned,
            InvitationRole::Watcher | InvitationRole::Editor | InvitationRole::Viewer => {
                Self::Shared
            }
        }
    }
}

/// An activity event as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Activity {
    pub id: Uuid,
    /// `None` when the system made the change (V13).
    pub actor_id: Option<Uuid>,
    pub item_id: Uuid,
    pub kind: ActivityKind,
    /// The other end of a link; always an item the reader can view.
    pub related_item_id: Option<Uuid>,
    /// Who joined, for [`ActivityKind::Assigned`] and [`ActivityKind::Shared`].
    pub member_user_id: Option<Uuid>,
    pub member_role: Option<InvitationRole>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct Payload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    member_user_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    member_role: Option<InvitationRole>,
}

/// One event to record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NewActivity {
    pub actor: Option<Uuid>,
    pub item: Uuid,
    pub kind: ActivityKind,
    pub related: Option<Uuid>,
    pub member: Option<(Uuid, InvitationRole)>,
}

impl NewActivity {
    pub(crate) const fn new(actor: Option<Uuid>, item: Uuid, kind: ActivityKind) -> Self {
        Self {
            actor,
            item,
            kind,
            related: None,
            member: None,
        }
    }
}

/// Writes an event; call it inside the transaction of the change.
pub(crate) async fn record(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
    event: NewActivity,
) -> sqlx::Result<()> {
    let payload = Payload {
        member_user_id: event.member.map(|(user, _)| user),
        member_role: event.member.map(|(_, role)| role),
    };
    sqlx::query(
        "INSERT INTO activity_events (id, actor_id, item_id, type, related_item_id, payload,
                                      created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(kernel::Id::<()>::new_at(now).as_uuid())
    .bind(event.actor)
    .bind(event.item)
    .bind(event.kind.as_str())
    .bind(event.related)
    .bind(Json(payload))
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// Records a link on both of its ends, each pointing at the other.
pub(crate) async fn record_link(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
    actor: Option<Uuid>,
    kind: ActivityKind,
    (source, target): (Uuid, Uuid),
) -> sqlx::Result<()> {
    for (item, other) in [(source, target), (target, source)] {
        let event = NewActivity {
            related: Some(other),
            ..NewActivity::new(actor, item, kind)
        };
        record(&mut *conn, now, event).await?;
    }
    Ok(())
}

#[derive(FromRow)]
struct ActivityRow {
    id: Uuid,
    actor_id: Option<Uuid>,
    item_id: Uuid,
    kind: String,
    related_item_id: Option<Uuid>,
    payload: Json<Payload>,
    created_at: DateTime<Utc>,
}

impl TryFrom<ActivityRow> for Activity {
    type Error = AppError;

    fn try_from(row: ActivityRow) -> Result<Self, AppError> {
        // Excluded by the table's CHECK constraint.
        let kind = ActivityKind::parse(&row.kind)
            .ok_or_else(|| internal(sqlx::Error::Decode("unknown activity type".into())))?;
        Ok(Self {
            id: row.id,
            actor_id: row.actor_id,
            item_id: row.item_id,
            kind,
            related_item_id: row.related_item_id,
            member_user_id: row.payload.0.member_user_id,
            member_role: row.payload.0.member_role,
            created_at: row.created_at,
        })
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl ItemsService {
    /// Applies a touch rule to an item, for modules that own the change (H4). Call it inside the
    /// transaction of the change.
    ///
    /// # Errors
    /// Database errors.
    pub async fn touch(
        &self,
        conn: &mut PgConnection,
        item: Uuid,
        action: TouchAction,
    ) -> sqlx::Result<()> {
        touch::apply(conn, item, action, self.clock.now()).await
    }

    /// Records that `actor` completed an item (A1), for the modules that own completion. Call it
    /// inside the transaction of the change.
    ///
    /// # Errors
    /// Database errors.
    pub async fn record_completed(
        &self,
        conn: &mut PgConnection,
        actor: Uuid,
        item: Uuid,
    ) -> sqlx::Result<()> {
        let now = self.clock.now();
        touch::apply(&mut *conn, item, TouchAction::Completed, now).await?;
        let event = NewActivity::new(Some(actor), item, ActivityKind::Completed);
        record(conn, now, event).await
    }

    /// The history of one item `actor` can view, newest first (A2).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if the item is not visible, [`AppError::Validation`] for a bad
    /// cursor, or [`AppError::Internal`].
    pub async fn item_activity(
        &self,
        actor: Uuid,
        item: Uuid,
        page: &PageRequest,
    ) -> Result<Page<Activity>, AppError> {
        self.require(actor, item, Perms::VIEW).await?;
        self.activity_page(actor, Some(item), page).await
    }

    /// Activity across every item `actor` can view, newest first; the trash is left out (A2).
    ///
    /// # Errors
    /// [`AppError::Validation`] for a bad cursor, or [`AppError::Internal`].
    pub async fn activity_feed(
        &self,
        actor: Uuid,
        page: &PageRequest,
    ) -> Result<Page<Activity>, AppError> {
        self.activity_page(actor, None, page).await
    }

    async fn activity_page(
        &self,
        actor: Uuid,
        item: Option<Uuid>,
        page: &PageRequest,
    ) -> Result<Page<Activity>, AppError> {
        let after: Option<(DateTime<Utc>, Uuid)> = page
            .cursor
            .as_ref()
            .map(kernel::Cursor::decode)
            .transpose()?;
        let visible = self
            .access
            .permissions()
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        let rows = sqlx::query_as::<_, ActivityRow>(
            "SELECT a.id, a.actor_id, a.item_id, a.type AS kind, a.related_item_id, a.payload,
                    a.created_at
             FROM activity_events a JOIN items i ON i.id = a.item_id
             WHERE a.item_id = ANY($1)
               AND ($2::uuid IS NULL AND i.deleted_at IS NULL OR a.item_id = $2)
               AND (a.related_item_id IS NULL OR a.related_item_id = ANY($1))
               AND ($3::timestamptz IS NULL OR (a.created_at, a.id) < ($3, $4))
             ORDER BY a.created_at DESC, a.id DESC
             LIMIT $5",
        )
        .bind(&visible)
        .bind(item)
        .bind(after.map(|(at, _)| at))
        .bind(after.map(|(_, id)| id))
        .bind(page.fetch_limit())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let events = rows
            .into_iter()
            .map(Activity::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(events, page.limit, |event| {
            (event.created_at, event.id)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trip() {
        for kind in [
            ActivityKind::Created,
            ActivityKind::Completed,
            ActivityKind::Assigned,
            ActivityKind::Shared,
            ActivityKind::Linked,
            ActivityKind::Unlinked,
        ] {
            assert_eq!(ActivityKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ActivityKind::parse("nope"), None);
    }

    #[test]
    fn membership_kinds() {
        assert_eq!(
            ActivityKind::for_role(InvitationRole::Assignee),
            ActivityKind::Assigned
        );
        for role in [
            InvitationRole::Watcher,
            InvitationRole::Editor,
            InvitationRole::Viewer,
        ] {
            assert_eq!(ActivityKind::for_role(role), ActivityKind::Shared);
        }
    }
}
