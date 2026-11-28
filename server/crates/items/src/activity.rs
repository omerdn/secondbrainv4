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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
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
    /// Someone entered a value on a tracker (AK4, T3.5.4).
    Recorded,
}

impl ActivityKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Completed => "completed",
            Self::Assigned => "assigned",
            Self::Shared => "shared",
            Self::Linked => "linked",
            Self::Unlinked => "unlinked",
            Self::Recorded => "recorded",
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
            "recorded" => Some(Self::Recorded),
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

/// Most values in one list of an [`ActivityFilter`].
pub const ACTIVITY_FILTER_MAX_VALUES: usize = 50;

/// Narrows the activity feed (A2, T5.2.7): the `filter` of `GET /activity`, as JSON. Rows
/// narrow each other; values in a row are alternatives. It only ever narrows what the reader
/// can see.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ActivityFilter {
    /// Events on items owned by these teams (TK6).
    #[serde(default)]
    #[ts(optional)]
    pub team_ids: Option<Vec<Uuid>>,
    /// Events made by these people.
    #[serde(default)]
    #[ts(optional)]
    pub people: Option<Vec<Uuid>>,
    #[serde(default)]
    #[ts(optional)]
    pub kinds: Option<Vec<ActivityKind>>,
    /// From this moment on (included).
    #[serde(default)]
    #[ts(optional)]
    pub since: Option<DateTime<Utc>>,
    /// Before this moment.
    #[serde(default)]
    #[ts(optional)]
    pub until: Option<DateTime<Utc>>,
}

impl ActivityFilter {
    /// # Errors
    /// [`AppError::Validation`] for a list longer than [`ACTIVITY_FILTER_MAX_VALUES`] or a range
    /// that ends before it starts.
    pub fn validate(&self) -> Result<(), AppError> {
        let mut errors = kernel::ValidationErrors::new();
        for (field, len) in [
            ("team_ids", self.team_ids.as_ref().map_or(0, Vec::len)),
            ("people", self.people.as_ref().map_or(0, Vec::len)),
            ("kinds", self.kinds.as_ref().map_or(0, Vec::len)),
        ] {
            if len > ACTIVITY_FILTER_MAX_VALUES {
                errors.push(kernel::FieldError::new(
                    field,
                    kernel::FieldErrorCode::Invalid,
                    "too many values",
                ));
            }
        }
        if let (Some(since), Some(until)) = (self.since, self.until)
            && until < since
        {
            errors.push(kernel::FieldError::new(
                "until",
                kernel::FieldErrorCode::Invalid,
                "must not be before since",
            ));
        }
        errors.into_result()
    }
}

/// An empty list filters nothing, like an absent one.
fn non_empty<T: Clone>(values: Option<&Vec<T>>) -> Option<Vec<T>> {
    values.filter(|values| !values.is_empty()).cloned()
}

/// An activity event as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Activity {
    pub id: Uuid,
    /// `None` when the system made the change (V13).
    pub actor_id: Option<Uuid>,
    pub item_id: Uuid,
    /// The item's kind and title, which the reader can see (A2), for a feed (AK4).
    pub item_kind: String,
    pub item_title: String,
    pub kind: ActivityKind,
    /// The other end of a link; always an item the reader can view.
    pub related_item_id: Option<Uuid>,
    /// Who joined, for [`ActivityKind::Assigned`] and [`ActivityKind::Shared`].
    pub member_user_id: Option<Uuid>,
    pub member_role: Option<InvitationRole>,
    pub created_at: DateTime<Utc>,
    /// Emoji reactions to a completion (AK4): counts and the reader's own, never names.
    pub reactions: Vec<crate::reactions::ReactionCount>,
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
    item_kind: String,
    item_title: String,
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
            item_kind: row.item_kind,
            item_title: row.item_title,
            kind,
            related_item_id: row.related_item_id,
            member_user_id: row.payload.0.member_user_id,
            member_role: row.payload.0.member_role,
            created_at: row.created_at,
            reactions: Vec::new(),
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

    /// Records that `actor` entered a value on a tracker (A1, AK4): the tracker is touched and
    /// friends can react to the entry. Call it inside the transaction of the entry.
    ///
    /// # Errors
    /// Database errors.
    pub async fn record_entry(
        &self,
        conn: &mut PgConnection,
        actor: Uuid,
        tracker: Uuid,
    ) -> sqlx::Result<()> {
        let now = self.clock.now();
        touch::apply(&mut *conn, tracker, TouchAction::EntryLogged, now).await?;
        let event = NewActivity::new(Some(actor), tracker, ActivityKind::Recorded);
        record(conn, now, event).await
    }

    /// Records that `actor` became an assignee of an item (A1), for the modules that own
    /// assignment. Call it inside the transaction of the change.
    ///
    /// # Errors
    /// Database errors.
    pub async fn record_assigned(
        &self,
        conn: &mut PgConnection,
        actor: Uuid,
        item: Uuid,
    ) -> sqlx::Result<()> {
        let now = self.clock.now();
        touch::apply(&mut *conn, item, TouchAction::Edited, now).await?;
        let event = NewActivity::new(Some(actor), item, ActivityKind::Assigned);
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
        self.activity_page(actor, Some(item), &ActivityFilter::default(), page)
            .await
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
        self.activity_page(actor, None, &ActivityFilter::default(), page)
            .await
    }

    /// [`Self::activity_feed`] narrowed by `filter` (A2, TK6, T5.2.7).
    ///
    /// # Errors
    /// [`AppError::Validation`] for a bad filter or cursor, or [`AppError::Internal`].
    pub async fn filtered_activity(
        &self,
        actor: Uuid,
        filter: &ActivityFilter,
        page: &PageRequest,
    ) -> Result<Page<Activity>, AppError> {
        filter.validate()?;
        self.activity_page(actor, None, filter, page).await
    }

    async fn activity_page(
        &self,
        actor: Uuid,
        item: Option<Uuid>,
        filter: &ActivityFilter,
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
            "SELECT a.id, a.actor_id, a.item_id, i.kind AS item_kind, i.title AS item_title,
                    a.type AS kind, a.related_item_id, a.payload, a.created_at
             FROM activity_events a JOIN items i ON i.id = a.item_id
             WHERE a.item_id = ANY($1)
               AND ($2::uuid IS NULL AND i.deleted_at IS NULL OR a.item_id = $2)
               AND (a.related_item_id IS NULL OR a.related_item_id = ANY($1))
               AND ($3::timestamptz IS NULL OR (a.created_at, a.id) < ($3, $4))
               AND ($6::uuid[] IS NULL OR i.owner_team_id = ANY($6))
               AND ($7::uuid[] IS NULL OR a.actor_id = ANY($7))
               AND ($8::text[] IS NULL OR a.type = ANY($8))
               AND ($9::timestamptz IS NULL OR a.created_at >= $9)
               AND ($10::timestamptz IS NULL OR a.created_at < $10)
             ORDER BY a.created_at DESC, a.id DESC
             LIMIT $5",
        )
        .bind(&visible)
        .bind(item)
        .bind(after.map(|(at, _)| at))
        .bind(after.map(|(_, id)| id))
        .bind(page.fetch_limit())
        .bind(non_empty(filter.team_ids.as_ref()))
        .bind(non_empty(filter.people.as_ref()))
        .bind(non_empty(filter.kinds.as_ref()).map(|kinds| {
            kinds
                .into_iter()
                .map(|kind| kind.as_str().to_owned())
                .collect::<Vec<_>>()
        }))
        .bind(filter.since)
        .bind(filter.until)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut events = rows
            .into_iter()
            .map(Activity::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let ids: Vec<Uuid> = events.iter().map(|event| event.id).collect();
        let mut reactions = self.reactions(actor, &ids).await?;
        for event in &mut events {
            event.reactions = reactions.remove(&event.id).unwrap_or_default();
        }
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
            ActivityKind::Recorded,
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
